// R3: raw-protocol differential. Replays identical scripts against the C
// oracle daemon and the Rust port and asserts byte-identical transcripts
// (opcodes, bodies, fd presence). Device-fd transfer needs real devices and
// stays in N2; transport-level FD semantics are covered in R1.
use seatd::conn::Conn;
use seatd::wire::*;
use std::os::unix::io::{FromRawFd, OwnedFd};
use std::process::{Child, Command, Stdio};

fn oracle_bin() -> String {
    ".work/tmp/oracle/seatd".to_string()
}

fn rust_bin() -> String {
    env!("CARGO_BIN_EXE_seatd").to_string()
}

fn unlink(path: &str) {
    unsafe {
        let c = std::ffi::CString::new(path).unwrap();
        libc::unlink(c.as_ptr());
    }
}

fn spawn(bin: &str, sock: &str) -> Child {
    unlink(sock);
    let mut cmd = Command::new(bin);
    cmd.arg("-l")
        .arg("silent")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("SEATD_VTBOUND", "0")
        .env("SEATD_SOCK", sock);
    let child = cmd.spawn().expect("spawn daemon");
    for _ in 0..50 {
        let c = unsafe {
            let cstr = std::ffi::CString::new(sock).unwrap();
            libc::open(cstr.as_ptr(), libc::O_PATH | libc::O_CLOEXEC)
        };
        if c >= 0 {
            unsafe {
                libc::close(c);
            }
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    child
}

fn connect(sock: &str) -> Conn {
    unsafe {
        let cstr = std::ffi::CString::new(sock).unwrap();
        let mut addr: libc::sockaddr_un = std::mem::zeroed();
        addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
        let bytes = sock.as_bytes();
        for (i, b) in bytes.iter().enumerate() {
            addr.sun_path[i] = *b as libc::c_char;
        }
        let len = std::mem::size_of::<libc::sa_family_t>() + bytes.len() + 1;
        let fd = libc::socket(
            libc::AF_UNIX,
            libc::SOCK_STREAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
            0,
        );
        assert!(fd >= 0);
        let mut tries = 0;
        loop {
            let r = libc::connect(
                fd,
                &addr as *const _ as *const libc::sockaddr,
                len as libc::socklen_t,
            );
            if r == 0 {
                break;
            }
            let e = std::io::Error::last_os_error().raw_os_error().unwrap();
            tries += 1;
            assert!(tries < 50 && (e == libc::ENOENT || e == libc::ECONNREFUSED), "connect {e}");
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        Conn::new(OwnedFd::from_raw_fd(fd))
    }
}

fn poll_in(conn: &Conn, ms: i32) -> bool {
    unsafe {
        let mut pfd = libc::pollfd {
            fd: conn.raw_fd(),
            events: libc::POLLIN | libc::POLLHUP | libc::POLLERR,
            revents: 0,
        };
        libc::poll(&mut pfd, 1, ms) > 0
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Msg {
    opcode: u16,
    body: Vec<u8>,
    had_fd: bool,
}

struct ScriptConn {
    conn: Conn,
}

impl ScriptConn {
    fn send(&mut self, opcode: u16, body: &[u8]) {
        self.conn
            .put(&Header { opcode, size: body.len() as u16 }.encode())
            .unwrap();
        self.conn.put(body).unwrap();
        for _ in 0..100 {
            match self.conn.flush() {
                Ok(_) => return,
                Err(e) if e == libc::EAGAIN => {
                    assert!(poll_in(&self.conn, 2000));
                }
                Err(e) => panic!("flush {e}"),
            }
        }
        panic!("flush never completed");
    }

    fn recv(&mut self) -> Option<Msg> {
        for _ in 0..100 {
            if self.conn.pending() >= 4 {
                break;
            }
            if !poll_in(&self.conn, 2000) {
                panic!("no response");
            }
            match self.conn.read() {
                Ok(0) => return None,
                Ok(_) => {}
                Err(e) if e == libc::EAGAIN => {}
                Err(e) => panic!("read {e}"),
            }
            if self.conn.pending() == 0 {
                let mut pfd = unsafe {
                    let mut p = libc::pollfd {
                        fd: self.conn.raw_fd(),
                        events: libc::POLLHUP | libc::POLLERR,
                        revents: 0,
                    };
                    p
                };
                unsafe {
                    libc::poll(&mut pfd, 1, 0);
                }
                if pfd.revents & (libc::POLLHUP | libc::POLLERR) != 0 {
                    match self.conn.read() {
                        Ok(0) => return None,
                        _ => {}
                    }
                }
            }
        }
        let mut raw = [0u8; 4];
        if self.conn.get(&mut raw).is_err() {
            return None;
        }
        let hdr = Header::decode(&raw);
        for _ in 0..100 {
            if self.conn.pending() >= hdr.size as usize {
                break;
            }
            assert!(poll_in(&self.conn, 2000));
            match self.conn.read() {
                Ok(0) => panic!("eof mid-frame"),
                Ok(_) => {}
                Err(e) if e == libc::EAGAIN => {}
                Err(e) => panic!("read {e}"),
            }
        }
        let mut body = vec![0u8; hdr.size as usize];
        self.conn.get(&mut body).unwrap();
        let had_fd = self.conn.get_fd().is_ok();
        Some(Msg { opcode: hdr.opcode, body, had_fd })
    }

    fn recv_all(&mut self, n: usize) -> Vec<Msg> {
        (0..n).map(|_| self.recv().expect("expected message")).collect()
    }

    fn expect_eof(&mut self) {
        for _ in 0..100 {
            match self.conn.read() {
                Ok(0) => return,
                Ok(_) => {
                    if self.conn.pending() > 0 {
                        panic!("data after expected eof");
                    }
                }
                Err(e) if e == libc::EAGAIN => {
                    if !poll_in(&self.conn, 2000) {
                        panic!("neither data nor eof");
                    }
                }
                Err(e) => panic!("read {e}"),
            }
            let mut pfd = libc::pollfd {
                fd: self.conn.raw_fd(),
                events: libc::POLLHUP | libc::POLLERR,
                revents: 0,
            };
            unsafe {
                libc::poll(&mut pfd, 1, 0);
            }
            if pfd.revents & (libc::POLLHUP | libc::POLLERR) != 0 {
                match self.conn.read() {
                    Ok(0) => return,
                    _ => {}
                }
            }
        }
        panic!("eof never arrived");
    }
}

fn run_scripts(sock: &str) -> Vec<Vec<Msg>> {
    let mut out = Vec::new();

    // D1: ping lifecycle around one seat.
    {
        let mut c = ScriptConn { conn: connect(sock) };
        c.send(CLIENT_PING, &[]);
        let mut t = vec![c.recv().unwrap()];
        c.send(CLIENT_OPEN_SEAT, &[]);
        t.extend(c.recv_all(2));
        c.send(CLIENT_PING, &[]);
        t.push(c.recv().unwrap());
        c.send(CLIENT_CLOSE_SEAT, &[]);
        t.push(c.recv().unwrap());
        c.send(CLIENT_PING, &[]);
        t.push(c.recv().unwrap());
        out.push(t);
    }
    // D2: error codes.
    {
        let mut c = ScriptConn { conn: connect(sock) };
        c.send(CLIENT_OPEN_SEAT, &[]);
        let mut t = c.recv_all(2);
        let mut path = 10u16.to_ne_bytes().to_vec();
        path.extend_from_slice(b"/dev/null\0");
        c.send(CLIENT_OPEN_DEVICE, &path);
        t.push(c.recv().unwrap());
        c.send(CLIENT_CLOSE_DEVICE, &99i32.to_ne_bytes());
        t.push(c.recv().unwrap());
        c.send(CLIENT_SWITCH_SESSION, &0i32.to_ne_bytes());
        t.push(c.recv().unwrap());
        c.send(CLIENT_SWITCH_SESSION, &99i32.to_ne_bytes());
        t.push(c.recv().unwrap());
        c.send(CLIENT_DISABLE_SEAT, &[]);
        t.push(c.recv().unwrap());
        c.send(CLIENT_CLOSE_SEAT, &[]);
        t.push(c.recv().unwrap());
        out.push(t);
    }
    // D3: two-client switch event ordering.
    {
        let mut a = ScriptConn { conn: connect(sock) };
        let mut b = ScriptConn { conn: connect(sock) };
        let mut t = Vec::new();
        a.send(CLIENT_OPEN_SEAT, &[]);
        t.extend(a.recv_all(2));
        b.send(CLIENT_OPEN_SEAT, &[]);
        t.extend(b.recv_all(1));
        a.send(CLIENT_SWITCH_SESSION, &2i32.to_ne_bytes());
        t.extend(a.recv_all(2));
        a.send(CLIENT_DISABLE_SEAT, &[]);
        t.extend(a.recv_all(1));
        t.extend(b.recv_all(1));
        b.send(CLIENT_CLOSE_SEAT, &[]);
        t.extend(b.recv_all(1));
        a.send(CLIENT_CLOSE_SEAT, &[]);
        t.extend(a.recv_all(1));
        out.push(t);
    }
    // D4: malformed frames destroy the connection.
    for (opcode, body) in [
        (999u16, vec![]),
        (CLIENT_OPEN_SEAT, vec![1u8]),
        (CLIENT_OPEN_DEVICE, vec![9u8, 0]),
        (CLIENT_CLOSE_DEVICE, vec![1u8, 2]),
        (CLIENT_CLOSE_SEAT, vec![]),
    ] {
        if opcode == CLIENT_CLOSE_SEAT {
            let mut c = ScriptConn { conn: connect(sock) };
            c.send(CLIENT_CLOSE_SEAT, &[]);
            c.expect_eof();
            out.push(vec![]);
            continue;
        }
        let mut c = ScriptConn { conn: connect(sock) };
        c.send(opcode, &body);
        c.expect_eof();
        out.push(vec![]);
    }
    // D5: double open destroys (after serving the first).
    {
        let mut c = ScriptConn { conn: connect(sock) };
        c.send(CLIENT_OPEN_SEAT, &[]);
        let mut t = c.recv_all(2);
        c.send(CLIENT_OPEN_SEAT, &[]);
        c.expect_eof();
        out.push(t);
    }
    out
}

fn check_pair() {
    assert!(std::path::Path::new(&oracle_bin()).exists(),
        "missing C oracle; rebuild with harness/libseat_diff.sh");
    let rust_sock = ".work/tmp/diff-rust.sock";
    let oracle_sock = ".work/tmp/oracle.sock";
    let mut rust = spawn(&rust_bin(), rust_sock);
    let rust_t = run_scripts(rust_sock);
    rust.kill().ok();
    rust.wait().ok();
    let mut oracle = spawn(&oracle_bin(), oracle_sock);
    let oracle_t = run_scripts(oracle_sock);
    oracle.kill().ok();
    oracle.wait().ok();
    assert_eq!(rust_t.len(), oracle_t.len());
    assert_eq!(rust_t[0].len(), 6, "D1 must carry six messages");
    assert!(rust_t[1].iter().any(|m| m.opcode == SERVER_ERROR));
    assert!(rust_t[2].len() >= 7, "D3 switch flow must carry events");
    for (i, (r, o)) in rust_t.iter().zip(oracle_t.iter()).enumerate() {
        assert_eq!(r, o, "script {i} transcript differs");
    }
}

#[test]
fn differential_transcripts_match_oracle() {
    check_pair();
}
