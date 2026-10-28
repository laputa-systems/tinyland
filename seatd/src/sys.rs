use rustix::fd::OwnedFd;
use std::ffi::CString;
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};

fn errno_now() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(libc::EINVAL)
}

pub fn drm_set_master(fd: RawFd) -> Result<(), i32> {
    let ret = unsafe { libc::ioctl(fd, libc::_IO(b'd' as u32, 0x1e) as _, 0) };
    if ret < 0 {
        Err(errno_now())
    } else {
        Ok(())
    }
}

pub fn drm_drop_master(fd: RawFd) -> Result<(), i32> {
    let ret = unsafe { libc::ioctl(fd, libc::_IO(b'd' as u32, 0x1f) as _, 0) };
    if ret < 0 {
        Err(errno_now())
    } else {
        Ok(())
    }
}

fn iow(type_: u8, nr: u8) -> libc::c_int {
    (0x40000000u32 | ((std::mem::size_of::<libc::c_int>() as u32) << 16) | ((type_ as u32) << 8) | nr as u32) as libc::c_int
}

pub fn evdev_revoke(fd: RawFd) -> Result<(), i32> {
    let ret = unsafe { libc::ioctl(fd, iow(b'E', 0x91), 0) };
    if ret < 0 {
        Err(errno_now())
    } else {
        Ok(())
    }
}

pub fn hidraw_revoke(fd: RawFd) -> Result<(), i32> {
    let ret = unsafe { libc::ioctl(fd, iow(b'H', 0x0d), 0) };
    if ret < 0 {
        Err(errno_now())
    } else {
        Ok(())
    }
}

pub mod vt {
    use super::*;

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
}
