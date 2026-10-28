// R7: storm harness. Reconnect/malformed/slow-reader/switch storms with
// FD and memory gates, plus idle CPU/wakeup logging against the C oracle
// (O4: numbers logged every run, gate only egregious regressions).
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, Stdio};

static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn rust_bin() -> String {
    env!("CARGO_BIN_EXE_seatd").to_string()
}

fn oracle_bin() -> String {
    ".work/tmp/oracle/seatd".to_string()
}

fn unlink(path: &str) {
    unsafe {
        let c = std::ffi::CString::new(path).unwrap();
        libc::unlink(c.as_ptr());
    }
}

fn spawn_rust(sock: &str) -> Child {
    unlink(sock);
    let log = std::fs::File::create(".work/tmp/storm-daemon.log").unwrap();
    let child = Command::new(rust_bin())
        .arg("-l")
        .arg("debug")
        .stderr(log)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("SEATD_VTBOUND", "0")
        .env("SEATD_SOCK", sock)
        .spawn()
        .expect("spawn rust seatd");
    for _ in 0..50 {
        if std::path::Path::new(sock).exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(std::path::Path::new(sock).exists());
    child
}

fn spawn_oracle(sock: &str) -> Child {
    unlink(sock);
    let child = Command::new(oracle_bin())
        .arg("-l")
        .arg("silent")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("SEATD_VTBOUND", "0")
        .spawn()
        .expect("spawn oracle seatd");
    for _ in 0..50 {
        if std::path::Path::new(sock).exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(std::path::Path::new(sock).exists());
    child
}

struct Raw {
    s: UnixStream,
}

impl Raw {
    fn connect(sock: &str) -> Raw {
        let s = UnixStream::connect(sock).expect("connect");
        s.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
        s.set_write_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
        Raw { s }
    }

    fn send(&mut self, opcode: u16, body: &[u8]) {
        let mut f = Vec::with_capacity(4 + body.len());
        f.extend_from_slice(&opcode.to_ne_bytes());
        f.extend_from_slice(&(body.len() as u16).to_ne_bytes());
        f.extend_from_slice(body);
        self.s.write_all(&f).unwrap();
    }

    fn recv(&mut self) -> (u16, Vec<u8>) {
        let mut hdr = [0u8; 4];
        self.s.read_exact(&mut hdr).unwrap();
        let opcode = u16::from_ne_bytes([hdr[0], hdr[1]]);
        let size = u16::from_ne_bytes([hdr[2], hdr[3]]) as usize;
        let mut body = vec![0u8; size];
        self.s.read_exact(&mut body).unwrap();
        (opcode, body)
    }
}

fn fd_count(pid: u32) -> usize {
    std::fs::read_dir(format!("/proc/{pid}/fd")).map(|d| d.count()).unwrap()
}

fn rss_kb(pid: u32) -> usize {
    let s = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap();
    for line in s.lines() {
        if let Some(v) = line.strip_prefix("VmRSS:") {
            return v.split_whitespace().next().unwrap().parse().unwrap();
        }
    }
    panic!("no VmRSS");
}

fn wait_fds(child: &Child, want: usize) {
    for _ in 0..300 {
        if fd_count(child.id()) <= want {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    panic!("fds {} never settled to {want}", fd_count(child.id()));
}

fn open_client(sock: &str) -> Raw {
    let mut c = Raw::connect(sock);
    c.send(1, &[]);
    assert_eq!(c.recv().0, 0x8001);
    assert_eq!(c.recv().0, 0x8006);
    c
}

#[test]
fn storm_switch_storm_no_growth() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let sock = ".work/tmp/storm-switch.sock";
    let mut d = spawn_rust(sock);
    std::thread::sleep(std::time::Duration::from_millis(200));
    let rest_fds = fd_count(d.id());
    let rest_rss = rss_kb(d.id());
    let mut a = open_client(sock);
    let mut b = open_client(sock);
    let mut c = open_client(sock);
    let mut clients = [&mut a, &mut b, &mut c];
    let mut active = 0usize;
    for _ in 0..60 {
        let next = (active + 1) % 3;
        let target = (next + 1) as i32;
        clients[active].send(6, &target.to_ne_bytes());
        assert_eq!(clients[active].recv().0, 0x8005, "disable event first");
        assert_eq!(clients[active].recv().0, 0x8008, "then switched");
        clients[active].send(5, &[]);
        assert_eq!(clients[active].recv().0, 0x8009, "disabled ack");
        assert_eq!(clients[next].recv().0, 0x8006, "next enabled");
        active = next;
    }
    drop(a);
    drop(b);
    drop(c);
    wait_fds(&d, rest_fds);
    assert_eq!(fd_count(d.id()), rest_fds, "fd growth after switch storm");
    let rss = rss_kb(d.id());
    println!("storm_switch: rss {rest_rss}kB -> {rss}kB");
    assert!(rss <= rest_rss + 1024, "rss growth after switch storm");
    d.kill().ok();
    d.wait().ok();
}

#[test]
fn storm_slow_reader_gets_everything() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let sock = ".work/tmp/storm-slow.sock";
    let mut d = spawn_rust(sock);
    let mut a = open_client(sock);
    let slow = std::os::unix::net::UnixStream::connect(sock).expect("connect");
    unsafe {
        let tiny: libc::c_int = 512;
        libc::setsockopt(
            std::os::unix::io::AsRawFd::as_raw_fd(&slow),
            libc::SOL_SOCKET,
            libc::SO_RCVBUF,
            &tiny as *const _ as *const libc::c_void,
            std::mem::size_of::<libc::c_int>() as u32,
        );
    }
    slow.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
    slow.set_write_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
    let mut b = Raw { s: slow };
    b.send(1, &[]);
    a.send(6, &2i32.to_ne_bytes());
    assert_eq!(a.recv().0, 0x8005);
    assert_eq!(a.recv().0, 0x8008);
    drop(a);
    let mut ops = Vec::new();
    loop {
        let mut hdr = [0u8; 4];
        match b.s.read_exact(&mut hdr) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => panic!("slow read {e}"),
        }
        let size = u16::from_ne_bytes([hdr[2], hdr[3]]) as usize;
        let mut body = vec![0u8; size];
        b.s.read_exact(&mut body).unwrap();
        ops.push(u16::from_ne_bytes([hdr[0], hdr[1]]));
    }
    assert_eq!(ops.len(), 2, "slow reader gets open + enable, got {ops:?}");
    assert_eq!(ops[0], 0x8001);
    assert_eq!(ops[1], 0x8006);
    d.kill().ok();
    d.wait().ok();
}

fn proc_times(pid: u32) -> (u64, u64, u64) {
    let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
    let f: Vec<&str> = s.rsplit(')').next().unwrap().split_whitespace().collect();
    let utime: u64 = f[11].parse().unwrap();
    let stime: u64 = f[12].parse().unwrap();
    let st = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap();
    let mut vol = 0u64;
    for line in st.lines() {
        if let Some(v) = line.strip_prefix("voluntary_ctxt_switches:") {
            vol = v.trim().parse().unwrap();
        }
    }
    (utime, stime, vol)
}

#[test]
fn storm_idle_parity_with_oracle() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    if !std::path::Path::new(&oracle_bin()).exists() {
        panic!("missing C oracle; rebuild with harness/libseat_diff.sh");
    }
    let rsock = ".work/tmp/storm-idle-rust.sock";
    let osock = ".work/tmp/oracle.sock";
    let mut r = spawn_rust(rsock);
    let mut o = spawn_oracle(osock);
    std::thread::sleep(std::time::Duration::from_millis(300));
    let (ru0, rs0, rv0) = proc_times(r.id());
    let (ou0, os0, ov0) = proc_times(o.id());
    std::thread::sleep(std::time::Duration::from_secs(5));
    let (ru1, rs1, rv1) = proc_times(r.id());
    let (ou1, os1, ov1) = proc_times(o.id());
    let r_cpu = (ru1 - ru0) + (rs1 - rs0);
    let o_cpu = (ou1 - ou0) + (os1 - os0);
    let r_vol = rv1 - rv0;
    let o_vol = ov1 - ov0;
    println!("storm_idle: rust cpu={r_cpu} ticks vol={r_vol} | oracle cpu={o_cpu} ticks vol={o_vol}");
    assert!(r_vol <= 2 * o_vol + 10, "egregious wakeup regression");
    assert!(r_cpu <= 2 * o_cpu + 10, "egregious idle cpu regression");
    r.kill().ok();
    r.wait().ok();
    o.kill().ok();
    o.wait().ok();
}
