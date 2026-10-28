use crate::session::DeviceType;
use rustix::fd::OwnedFd;
use std::ffi::{CStr, CString};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};

fn errno_now() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(libc::EINVAL)
}

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

pub trait Effect {
    fn open_device(&mut self, path: &str, dev_type: DeviceType) -> Result<OwnedFd, i32>;
    fn activate_device(&mut self, fd: RawFd, dev_type: DeviceType) -> Result<(), i32>;
    fn deactivate_device(&mut self, fd: RawFd, dev_type: DeviceType) -> Result<(), i32>;
    fn vt_open(&mut self, session: i32) -> Result<(), i32>;
    fn vt_close(&mut self, session: i32) -> Result<(), i32>;
    fn vt_switch(&mut self, cur_vt: i32, target: i32) -> Result<(), i32>;
    fn vt_ack(&mut self, cur_vt: i32, release: bool) -> Result<(), i32>;
    fn current_vt(&mut self) -> Result<i32, i32>;
}

pub fn classify_linux(path: &str) -> Option<DeviceType> {
    if path.starts_with("/dev/input/event") {
        Some(DeviceType::Evdev)
    } else if path.starts_with("/dev/dri/") {
        Some(DeviceType::Drm)
    } else if path.starts_with("/dev/hidraw") {
        Some(DeviceType::Hidraw)
    } else {
        None
    }
}

pub fn canonicalize(path: &str) -> Result<String, i32> {
    let c = CString::new(path).map_err(|_| libc::ENOENT)?;
    unsafe {
        let resolved = libc::realpath(c.as_ptr(), std::ptr::null_mut());
        if resolved.is_null() {
            return Err(errno_now());
        }
        let s = CStr::from_ptr(resolved).to_string_lossy().into_owned();
        libc::free(resolved as *mut libc::c_void);
        Ok(s)
    }
}

fn drm_set_master(fd: RawFd) -> Result<(), i32> {
    let ret = unsafe { libc::ioctl(fd, libc::_IO(b'd' as u32, 0x1e) as _, 0) };
    if ret < 0 {
        Err(errno_now())
    } else {
        Ok(())
    }
}

fn drm_drop_master(fd: RawFd) -> Result<(), i32> {
    let ret = unsafe { libc::ioctl(fd, libc::_IO(b'd' as u32, 0x1f) as _, 0) };
    if ret < 0 {
        Err(errno_now())
    } else {
        Ok(())
    }
}

fn evdev_revoke(fd: RawFd) -> Result<(), i32> {
    let op = 0x40000000u32 | ((std::mem::size_of::<libc::c_int>() as u32) << 16) | ((b'E' as u32) << 8) | 0x91;
    let ret = unsafe { libc::ioctl(fd, op as _, 0) };
    if ret < 0 {
        Err(errno_now())
    } else {
        Ok(())
    }
}

fn hidraw_revoke(fd: RawFd) -> Result<(), i32> {
    let op = 0x40000000u32 | ((std::mem::size_of::<libc::c_int>() as u32) << 16) | ((b'H' as u32) << 8) | 0x0d;
    let ret = unsafe { libc::ioctl(fd, op as _, 0) };
    if ret < 0 {
        Err(errno_now())
    } else {
        Ok(())
    }
}

pub struct RealEffect;

impl RealEffect {
    fn terminal_open(vt: i32) -> Result<OwnedFd, i32> {
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
}

impl Effect for RealEffect {
    fn open_device(&mut self, path: &str, dev_type: DeviceType) -> Result<OwnedFd, i32> {
        let c = CString::new(path).map_err(|_| libc::ENOENT)?;
        unsafe {
            let fd = libc::open(
                c.as_ptr(),
                libc::O_RDWR | libc::O_NOCTTY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            );
            if fd < 0 {
                return Err(errno_now());
            }
            if dev_type == DeviceType::Drm {
                if drm_set_master(fd).is_err() {
                    crate::log_error!("Could not make device fd drm master, continuing");
                }
            }
            Ok(OwnedFd::from_raw_fd(fd))
        }
    }

    fn activate_device(&mut self, fd: RawFd, dev_type: DeviceType) -> Result<(), i32> {
        match dev_type {
            DeviceType::Drm => {
                if drm_set_master(fd).is_err() {
                    crate::log_error!("Could not make device fd drm master, continuing");
                }
                Ok(())
            }
            DeviceType::Evdev | DeviceType::Hidraw => Err(libc::EINVAL),
            DeviceType::Wscons => Ok(()),
        }
    }

    fn deactivate_device(&mut self, fd: RawFd, dev_type: DeviceType) -> Result<(), i32> {
        match dev_type {
            DeviceType::Drm => drm_drop_master(fd),
            DeviceType::Evdev => evdev_revoke(fd),
            DeviceType::Hidraw => hidraw_revoke(fd),
            DeviceType::Wscons => Ok(()),
        }
    }

    fn vt_open(&mut self, session: i32) -> Result<(), i32> {
        let tty = Self::terminal_open(session)?;
        let fd = tty.as_raw_fd();
        let mode = VtMode {
            mode: VT_PROCESS,
            waitv: 0,
            relsig: libc::SIGUSR1 as i16,
            acqsig: libc::SIGUSR2 as i16,
            frsig: 0,
        };
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

    fn vt_close(&mut self, session: i32) -> Result<(), i32> {
        let tty = Self::terminal_open(session)?;
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

    fn vt_switch(&mut self, cur_vt: i32, target: i32) -> Result<(), i32> {
        let tty = Self::terminal_open(cur_vt)?;
        let fd = tty.as_raw_fd();
        let mode = VtMode {
            mode: VT_PROCESS,
            waitv: 0,
            relsig: libc::SIGUSR1 as i16,
            acqsig: libc::SIGUSR2 as i16,
            frsig: 0,
        };
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

    fn vt_ack(&mut self, cur_vt: i32, release: bool) -> Result<(), i32> {
        let tty = Self::terminal_open(cur_vt)?;
        let fd = tty.as_raw_fd();
        let arg = if release { 1 } else { VT_ACKACQ };
        let r = unsafe { libc::ioctl(fd, VT_RELDISP, arg) };
        if r < 0 {
            Err(errno_now())
        } else {
            Ok(())
        }
    }

    fn current_vt(&mut self) -> Result<i32, i32> {
        let tty = Self::terminal_open(0)?;
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

pub struct NullEffect {
    pub cur_vt: i32,
    pub calls: Vec<String>,
}

impl NullEffect {
    pub fn new(cur_vt: i32) -> NullEffect {
        NullEffect {
            cur_vt,
            calls: Vec::new(),
        }
    }

    pub fn null_fd() -> OwnedFd {
        unsafe {
            let fd = libc::open(
                c"/dev/null".as_ptr(),
                libc::O_RDWR | libc::O_CLOEXEC,
            );
            assert!(fd >= 0);
            OwnedFd::from_raw_fd(fd)
        }
    }
}

impl Effect for NullEffect {
    fn open_device(&mut self, path: &str, _dev_type: DeviceType) -> Result<OwnedFd, i32> {
        self.calls.push(format!("open:{path}"));
        Ok(Self::null_fd())
    }

    fn activate_device(&mut self, _fd: RawFd, _dev_type: DeviceType) -> Result<(), i32> {
        self.calls.push("activate".to_string());
        Ok(())
    }

    fn deactivate_device(&mut self, _fd: RawFd, _dev_type: DeviceType) -> Result<(), i32> {
        self.calls.push("deactivate".to_string());
        Ok(())
    }

    fn vt_open(&mut self, session: i32) -> Result<(), i32> {
        self.calls.push(format!("vt_open:{session}"));
        Ok(())
    }

    fn vt_close(&mut self, session: i32) -> Result<(), i32> {
        self.calls.push(format!("vt_close:{session}"));
        Ok(())
    }

    fn vt_switch(&mut self, cur_vt: i32, target: i32) -> Result<(), i32> {
        self.calls.push(format!("vt_switch:{cur_vt}->{target}"));
        self.cur_vt = target;
        Ok(())
    }

    fn vt_ack(&mut self, cur_vt: i32, release: bool) -> Result<(), i32> {
        self.calls.push(format!("vt_ack:{cur_vt}:{}", release as u8));
        Ok(())
    }

    fn current_vt(&mut self) -> Result<i32, i32> {
        Ok(self.cur_vt)
    }
}

pub fn path_bytes(path: &std::ffi::OsStr) -> &[u8] {
    path.as_bytes()
}
