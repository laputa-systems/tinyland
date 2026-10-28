use crate::sys::errno_now;
use rustix::fd::OwnedFd;
use std::ffi::CString;
use std::os::unix::io::{AsRawFd, FromRawFd};

#[repr(C)]
struct VtMode {
    mode: u8,
    waitv: u8,
    relsig: i16,
    acqsig: i16,
    frsig: i16,
}

#[repr(C)]
struct VtStat {
    v_active: u16,
    v_signal: u16,
    v_state: u16,
}

const VT_SETMODE: libc::c_int = 0x5602;
const VT_GETSTATE: libc::c_int = 0x5603;
const VT_RELDISP: libc::c_int = 0x5605;
const VT_ACTIVATE: libc::c_int = 0x5606;
const VT_AUTO: u8 = 0x00;
const VT_PROCESS: u8 = 0x01;
const VT_ACKACQ: libc::c_int = 0x02;
const KDSKBMODE: libc::c_int = 0x4B45;
const K_OFF: libc::c_int = 0x04;
const K_UNICODE: libc::c_int = 0x03;
const KDSETMODE: libc::c_int = 0x4B3A;
const KD_GRAPHICS: libc::c_int = 0x01;
const KD_TEXT: libc::c_int = 0x00;

pub fn terminal_open(vt: i32) -> Result<OwnedFd, i32> {
    if vt < 0 {
        return Err(libc::EINVAL);
    }
    let path = format!("/dev/tty{vt}");
    let c = CString::new(path).map_err(|_| libc::EINVAL)?;
    unsafe {
        let fd = libc::open(c.as_ptr(), libc::O_RDWR | libc::O_NOCTTY);
        if fd < 0 {
            Err(errno_now())
        } else {
            Ok(OwnedFd::from_raw_fd(fd))
        }
    }
}

fn process_mode() -> VtMode {
    VtMode {
        mode: VT_PROCESS,
        waitv: 0,
        relsig: libc::SIGUSR1 as i16,
        acqsig: libc::SIGUSR2 as i16,
        frsig: 0,
    }
}

pub fn open_vt(session: i32) -> Result<(), i32> {
    let tty = terminal_open(session)?;
    let fd = tty.as_raw_fd();
    let mode = process_mode();
    let r1 = unsafe { libc::ioctl(fd, VT_SETMODE, &mode) };
    let r2 = unsafe { libc::ioctl(fd, KDSKBMODE, K_OFF) };
    let r3 = unsafe { libc::ioctl(fd, KDSETMODE, KD_GRAPHICS) };
    if r1 < 0 {
        return Err(errno_now());
    }
    if r2 < 0 {
        return Err(errno_now());
    }
    if r3 < 0 {
        return Err(errno_now());
    }
    Ok(())
}

pub fn close_vt(session: i32) -> Result<(), i32> {
    let tty = terminal_open(session)?;
    let fd = tty.as_raw_fd();
    let mode = VtMode {
        mode: VT_AUTO,
        waitv: 0,
        relsig: 0,
        acqsig: 0,
        frsig: 0,
    };
    let r1 = unsafe { libc::ioctl(fd, VT_SETMODE, &mode) };
    let r2 = unsafe { libc::ioctl(fd, KDSKBMODE, K_UNICODE) };
    let r3 = unsafe { libc::ioctl(fd, KDSETMODE, KD_TEXT) };
    if r1 < 0 {
        return Err(errno_now());
    }
    if r2 < 0 {
        return Err(errno_now());
    }
    if r3 < 0 {
        return Err(errno_now());
    }
    Ok(())
}

pub fn switch_vt(cur_vt: i32, target: i32) -> Result<(), i32> {
    let tty = terminal_open(cur_vt)?;
    let fd = tty.as_raw_fd();
    let mode = process_mode();
    let r0 = unsafe { libc::ioctl(fd, VT_SETMODE, &mode) };
    if r0 < 0 {
        return Err(errno_now());
    }
    let r = unsafe { libc::ioctl(fd, VT_ACTIVATE, target) };
    if r < 0 {
        Err(errno_now())
    } else {
        Ok(())
    }
}

pub fn ack_vt(cur_vt: i32, release: bool) -> Result<(), i32> {
    let tty = terminal_open(cur_vt)?;
    let fd = tty.as_raw_fd();
    let arg = if release { 1 } else { VT_ACKACQ };
    let r = unsafe { libc::ioctl(fd, VT_RELDISP, arg) };
    if r < 0 {
        Err(errno_now())
    } else {
        Ok(())
    }
}

pub fn current_vt() -> Result<i32, i32> {
    let tty = terminal_open(0)?;
    let fd = tty.as_raw_fd();
    let mut st: VtStat = unsafe { std::mem::zeroed() };
    let r = unsafe { libc::ioctl(fd, VT_GETSTATE, &mut st) };
    if r < 0 {
        Err(errno_now())
    } else {
        Ok(st.v_active as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vt_terminal_open_rejects_negative() {
        assert_eq!(terminal_open(-1).unwrap_err(), libc::EINVAL);
    }

    #[test]
    fn vt_terminal_open_missing_fails_loudly() {
        let r = terminal_open(999);
        assert!(r.is_err());
        assert_ne!(r.unwrap_err(), 0);
    }

    #[test]
    fn vt_ops_fail_loudly_without_vt() {
        assert!(open_vt(999).is_err());
        assert!(close_vt(999).is_err());
        assert!(switch_vt(999, 1).is_err());
        assert!(ack_vt(999, true).is_err());
        let _ = current_vt();
    }
}
