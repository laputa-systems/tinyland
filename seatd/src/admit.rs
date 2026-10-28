use crate::{log_error, log_info};
use rustix::fd::OwnedFd;
use std::ffi::CString;
use std::io;
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};

pub const LISTEN_BACKLOG: i32 = 16;

#[cfg(target_os = "linux")]
pub const DEFAULT_SOCKET_PATH: &str = "/run/seatd.sock";
#[cfg(not(target_os = "linux"))]
pub const DEFAULT_SOCKET_PATH: &str = "/var/run/seatd.sock";

pub fn socket_path() -> String {
    std::env::var("SEATD_SOCK").unwrap_or_else(|_| DEFAULT_SOCKET_PATH.to_string())
}

fn os_error() -> io::Error {
    io::Error::last_os_error()
}

pub fn path_stat_kind(path: &str) -> Result<libc::mode_t, io::Error> {
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

pub fn unlink_path(path: &str) -> Result<(), io::Error> {
    let cpath = CString::new(path)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "socket path contains NUL"))?;
    unsafe {
        if libc::unlink(cpath.as_ptr()) != 0 {
            return Err(os_error());
        }
    }
    Ok(())
}

pub fn open_socket(path: &str, uid: i64, gid: i64) -> Result<OwnedFd, io::Error> {
    unsafe {
        let fd = libc::socket(
            libc::AF_UNIX,
            libc::SOCK_STREAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
            0,
        );
        if fd < 0 {
            let e = os_error();
            log_error!("Could not create socket: {e}");
            return Err(e);
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
        let len = std::mem::size_of::<libc::sa_family_t>() + cpath.as_bytes().len() + 1;
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
                log_error!("Could not chown socket to uid {uid}, gid {gid}: {e}");
                libc::close(fd);
                return Err(e);
            }
        }
        Ok(OwnedFd::from_raw_fd(fd))
    }
}

pub struct PeerCreds {
    pub pid: libc::pid_t,
    pub uid: libc::uid_t,
    pub gid: libc::gid_t,
}

pub fn peer_creds(fd: RawFd) -> io::Result<PeerCreds> {
    unsafe {
        let mut cred: libc::ucred = std::mem::zeroed();
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        if libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut _ as *mut libc::c_void,
            &mut len,
        ) != 0
        {
            return Err(os_error());
        }
        Ok(PeerCreds {
            pid: cred.pid,
            uid: cred.uid,
            gid: cred.gid,
        })
    }
}

pub fn accept_client(listen_fd: RawFd) -> Option<(OwnedFd, PeerCreds)> {
    unsafe {
        let fd = libc::accept4(
            listen_fd,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
        );
        if fd < 0 {
            let e = os_error();
            log_error!("Could not accept client connection: {e}");
            return None;
        }
        match peer_creds(fd) {
            Ok(creds) => {
                log_info!(
                    "New client connected (pid: {}, uid: {}, gid: {})",
                    creds.pid,
                    creds.uid,
                    creds.gid
                );
                Some((OwnedFd::from_raw_fd(fd), creds))
            }
            Err(e) => {
                log_error!("Could not prepare new client socket: {e}");
                libc::close(fd);
                None
            }
        }
    }
}

pub fn raw_fd(fd: &OwnedFd) -> RawFd {
    fd.as_raw_fd()
}
