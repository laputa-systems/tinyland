mod cli;
mod log;
mod r#loop;

use r#loop::Server;
use std::ffi::CString;
use std::io;
use std::os::unix::io::RawFd;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const LISTEN_BACKLOG: i32 = 16;

#[cfg(target_os = "linux")]
const DEFAULT_SOCKET_PATH: &str = "/run/seatd.sock";
#[cfg(not(target_os = "linux"))]
const DEFAULT_SOCKET_PATH: &str = "/var/run/seatd.sock";

fn socket_path() -> String {
    std::env::var("SEATD_SOCK").unwrap_or_else(|_| DEFAULT_SOCKET_PATH.to_string())
}

fn os_error() -> io::Error {
    io::Error::last_os_error()
}

fn open_socket(path: &str, uid: i64, gid: i64) -> Result<RawFd, io::Error> {
    unsafe {
        let fd = libc::socket(
            libc::AF_UNIX,
            libc::SOCK_STREAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
            0,
        );
        if fd < 0 {
            return Err(os_error());
        }
        let cpath = match CString::new(path) {
            Ok(c) => c,
            Err(_) => {
                libc::close(fd);
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "socket path contains NUL"));
            }
        };
        let mut addr: libc::sockaddr_un = std::mem::zeroed();
        addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
        let bytes = cpath.as_bytes_with_nul();
        if bytes.len() > std::mem::size_of_val(&addr.sun_path) {
            libc::close(fd);
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "socket path too long"));
        }
        for (i, b) in bytes.iter().enumerate() {
            addr.sun_path[i] = *b as libc::c_char;
        }
        let len =
            std::mem::size_of::<libc::sa_family_t>() + cpath.as_bytes().len() + 1;
        let ret = libc::bind(
            fd,
            &addr as *const _ as *const libc::sockaddr,
            len as libc::socklen_t,
        );
        if ret != 0 {
            let e = os_error();
            log_error!("Could not bind socket: {e}");
            libc::close(fd);
            return Err(e);
        }
        if libc::listen(fd, LISTEN_BACKLOG) != 0 {
            let e = os_error();
            log_error!("Could not listen on socket: {e}");
            libc::close(fd);
            return Err(e);
        }
        if uid != -1 || gid != -1 {
            if libc::chmod(cpath.as_ptr(), 0o770) != 0 {
                let e = os_error();
                log_error!("Could not chmod socket: {e}");
                libc::close(fd);
                return Err(e);
            }
            if libc::chown(cpath.as_ptr(), uid as libc::uid_t, gid as libc::gid_t) != 0 {
                let e = os_error();
                log_error!(
                    "Could not chown socket to uid {uid}, gid {gid}: {e}"
                );
                libc::close(fd);
                return Err(e);
            }
        }
        Ok(fd)
    }
}

fn path_stat_kind(path: &str) -> Result<libc::mode_t, io::Error> {
    unsafe {
        let cpath = CString::new(path)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "socket path contains NUL"))?;
        let mut st: libc::stat = std::mem::zeroed();
        if libc::lstat(cpath.as_ptr(), &mut st) != 0 {
            return Err(os_error());
        }
        Ok(st.st_mode)
    }
}

fn unlink_path(path: &str) -> Result<(), io::Error> {
    let cpath = CString::new(path)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "socket path contains NUL"))?;
    unsafe {
        if libc::unlink(cpath.as_ptr()) != 0 {
            return Err(os_error());
        }
    }
    Ok(())
}

fn main() {
    let prog = std::env::args().next().unwrap_or_else(|| "seatd".to_string());
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let opts = match cli::parse(&argv, &prog) {
        Ok(o) => o,
        Err(code) => std::process::exit(code),
    };
    if opts.show_help {
        print!("{}", cli::USAGE);
        return;
    }
    if opts.show_version {
        println!("seatd version {VERSION}");
        return;
    }
    log::set_level(opts.level);

    let path = socket_path();
    match path_stat_kind(&path) {
        Err(e) if e.raw_os_error() == Some(libc::ENOENT) => {}
        Err(e) => {
            log_error!("Could not stat socket path: {e}");
            std::process::exit(1);
        }
        Ok(mode) => {
            if mode & libc::S_IFMT != libc::S_IFSOCK {
                log_error!(
                    "Non-socket file found at socket path {path}, refusing to start"
                );
                std::process::exit(1);
            } else if !opts.unlink_existing_socket {
                log_error!("Socket file found at socket path {path}, refusing to start");
                std::process::exit(1);
            } else {
                log_info!("Removing leftover socket at {path}");
                if let Err(e) = unlink_path(&path) {
                    log_error!("Could not remove leftover socket: {e}");
                    std::process::exit(1);
                }
            }
        }
    }

    let mut server = match Server::new() {
        Ok(s) => s,
        Err(e) => {
            log_error!("server_init failed: {e}");
            std::process::exit(1);
        }
    };

    let socket_fd = match open_socket(&path, opts.uid, opts.gid) {
        Ok(fd) => fd,
        Err(e) => {
            log_error!("Could not create server socket: {e}");
            server.finish();
            std::process::exit(1);
        }
    };
    server.set_listen(socket_fd);

    log_info!("seatd started");

    if opts.readiness != -1 {
        let buf: [u8; 1] = [b'\n'];
        let n = unsafe { libc::write(opts.readiness, buf.as_ptr() as *const _, 1) };
        if n != 1 {
            let e = os_error();
            log_error!("Could not write readiness signal: {e}");
        }
        unsafe {
            libc::close(opts.readiness);
        }
    }

    server.run();

    if let Err(e) = unlink_path(&path) {
        log_error!("Could not remove socket: {e}");
    }
    server.finish();
    log_info!("seatd stopped");
}
