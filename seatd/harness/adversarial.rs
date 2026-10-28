use seatd::conn::{Conn, CONNECTION_BUFFER_SIZE, MAX_FDS};
use std::os::unix::io::{AsRawFd, FromRawFd, OwnedFd};

fn pair() -> (Conn, Conn) {
    unsafe {
        let mut fds = [0; 2];
        assert_eq!(
            libc::socketpair(
                libc::AF_UNIX,
                libc::SOCK_STREAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
                0,
                fds.as_mut_ptr()
            ),
            0
        );
        (
            Conn::new(OwnedFd::from_raw_fd(fds[0])),
            Conn::new(OwnedFd::from_raw_fd(fds[1])),
        )
    }
}

fn dev_null() -> OwnedFd {
    unsafe {
        let fd = libc::open(
            c"/dev/null".as_ptr(),
            libc::O_RDWR | libc::O_CLOEXEC,
        );
        assert!(fd >= 0);
        OwnedFd::from_raw_fd(fd)
    }
}

fn fd_is_open(fd: i32) -> bool {
    unsafe { libc::fcntl(fd, libc::F_GETFD) >= 0 }
}

#[test]
fn wire_fragmented_header_reassembles() {
    let (mut a, mut b) = pair();
    a.put(&[1u8, 0, 0, 0]).unwrap();
    a.flush().unwrap();
    for _ in 0..4 {
        let n = b.read().unwrap();
        assert!(n <= 4);
        if b.pending() == 4 {
            break;
        }
    }
    assert_eq!(b.pending(), 4);
    let mut hdr = [0u8; 4];
    b.get(&mut hdr).unwrap();
    assert_eq!(hdr, [1u8, 0, 0, 0]);
}

#[test]
fn wire_coalesced_frames_parse_in_order() {
    let (mut a, mut b) = pair();
    a.put(&[1u8, 0, 0, 0, 7u8, 0, 0, 0]).unwrap();
    a.flush().unwrap();
    b.read().unwrap();
    assert_eq!(b.pending(), 8);
    let mut first = [0u8; 4];
    let mut second = [0u8; 4];
    b.get(&mut first).unwrap();
    b.get(&mut second).unwrap();
    assert_eq!(first, [1u8, 0, 0, 0]);
    assert_eq!(second, [7u8, 0, 0, 0]);
}

#[test]
fn wire_empty_read_is_clean_eof() {
    let (a, mut b) = pair();
    drop(a);
    assert_eq!(b.read().unwrap(), 0);
    assert_eq!(b.pending(), 0);
}

#[test]
fn wire_oversized_put_fails_loudly() {
    let (mut a, _b) = pair();
    assert_eq!(a.put(&[0u8; CONNECTION_BUFFER_SIZE + 1]), Err(libc::EOVERFLOW));
    let exact = [0u8; CONNECTION_BUFFER_SIZE];
    a.put(&exact).unwrap();
}

#[test]
fn wire_malformed_short_get_is_eagain() {
    let (mut a, mut b) = pair();
    a.put(&[9u8]).unwrap();
    a.flush().unwrap();
    b.read().unwrap();
    let mut hdr = [0u8; 4];
    assert_eq!(b.get(&mut hdr), Err(libc::EAGAIN));
    assert_eq!(b.pending(), 1);
}

#[test]
fn wire_fd_round_trip_exactly_once() {
    let (mut a, mut b) = pair();
    let null = dev_null();
    let null_raw = null.as_raw_fd();
    a.put_fd(null).unwrap();
    a.put(&[0xABu8]).unwrap();
    a.flush().unwrap();
    assert!(!fd_is_open(null_raw), "queued fd must be closed after flush");
    b.read().unwrap();
    assert_eq!(b.queued_fds(), 1);
    let got = b.get_fd().unwrap();
    let mut data = [0u8; 1];
    b.get(&mut data).unwrap();
    assert_eq!(data, [0xAB]);
    assert!(matches!(b.get_fd(), Err(libc::EAGAIN)));
    let got_raw = got.as_raw_fd();
    assert!(fd_is_open(got_raw));
    drop(got);
    assert!(!fd_is_open(got_raw));
}

#[test]
fn wire_unexpected_fds_have_one_owner() {
    let (mut a, mut b) = pair();
    let f1 = dev_null();
    let f2 = dev_null();
    let r1 = f1.as_raw_fd();
    let r2 = f2.as_raw_fd();
    a.put_fd(f1).unwrap();
    a.put_fd(f2).unwrap();
    a.put(&[1u8]).unwrap();
    a.flush().unwrap();
    assert!(!fd_is_open(r1));
    assert!(!fd_is_open(r2));
    b.read().unwrap();
    assert_eq!(b.queued_fds(), 2);
    let g1 = b.get_fd().unwrap();
    let g2 = b.get_fd().unwrap();
    assert!(fd_is_open(g1.as_raw_fd()));
    assert!(fd_is_open(g2.as_raw_fd()));
}

#[test]
fn wire_fd_queue_overflow_fails_loudly() {
    let (mut a, _b) = pair();
    for _ in 0..MAX_FDS {
        a.put_fd(dev_null()).unwrap();
    }
    assert_eq!(a.put_fd(dev_null()), Err(libc::EOVERFLOW));
}

#[test]
fn wire_receiver_fd_overflow_closes_extras() {
    let (mut a, mut b) = pair();
    for _ in 0..MAX_FDS {
        a.put_fd(dev_null()).unwrap();
        a.put(&[0u8]).unwrap();
        a.flush().unwrap();
        b.read().unwrap();
    }
    assert_eq!(b.pending(), MAX_FDS);
    assert_eq!(b.queued_fds(), MAX_FDS);
    let extra = dev_null();
    let extra_raw = extra.as_raw_fd();
    a.put_fd(extra).unwrap();
    a.put(&[0u8]).unwrap();
    a.flush().unwrap();
    assert!(!fd_is_open(extra_raw));
    assert_eq!(b.read(), Err(libc::EOVERFLOW));
    let mut drained = 0;
    while b.get_fd().is_ok() {
        drained += 1;
    }
    assert_eq!(drained, MAX_FDS);
}

#[test]
fn wire_partial_write_keeps_fd_exactly_once() {
    unsafe {
        let mut fds = [0; 2];
        assert_eq!(
            libc::socketpair(
                libc::AF_UNIX,
                libc::SOCK_STREAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
                0,
                fds.as_mut_ptr()
            ),
            0
        );
        let mut a = Conn::new(OwnedFd::from_raw_fd(fds[0]));
        let mut b = Conn::new(OwnedFd::from_raw_fd(fds[1]));
        let small: libc::c_int = 1024;
        libc::setsockopt(
            b.raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_RCVBUF,
            &small as *const _ as *const libc::c_void,
            std::mem::size_of::<libc::c_int>() as u32,
        );
        libc::setsockopt(
            a.raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_SNDBUF,
            &small as *const _ as *const libc::c_void,
            std::mem::size_of::<libc::c_int>() as u32,
        );
        let payload = [0x55u8; CONNECTION_BUFFER_SIZE];
        a.put(&payload).unwrap();
        let null = dev_null();
        let null_raw = null.as_raw_fd();
        a.put_fd(null).unwrap();
        let mut sent_total = 0usize;
        for _ in 0..200 {
            match a.flush() {
                Ok(n) => {
                    sent_total += n;
                    if sent_total >= CONNECTION_BUFFER_SIZE {
                        break;
                    }
                }
                Err(e) if e == libc::EAGAIN => {
                    let _ = b.read();
                }
                Err(e) => panic!("unexpected flush error {e}"),
            }
        }
        assert_eq!(sent_total, CONNECTION_BUFFER_SIZE);
        assert!(!fd_is_open(null_raw), "fd consumed exactly once on first send");
        loop {
            match b.read() {
                Ok(_) => {
                    if b.pending() >= CONNECTION_BUFFER_SIZE {
                        break;
                    }
                }
                Err(e) if e == libc::EAGAIN => break,
                Err(e) => panic!("unexpected read error {e}"),
            }
        }
        let mut out = [0u8; CONNECTION_BUFFER_SIZE];
        b.get(&mut out).unwrap();
        assert_eq!(out, payload);
        let mut fd_count = 0;
        while b.get_fd().is_ok() {
            fd_count += 1;
        }
        assert_eq!(fd_count, 1, "ancillary rights must not be resent on retry");
    }
}
