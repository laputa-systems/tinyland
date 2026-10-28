//! Brokered seat devices through the system libseat client, seatd-only.
//!
//! libseat.so.1 is opened at runtime so the static musl binary keeps no
//! client-library link dependency. LIBSEAT_BACKEND must select seatd;
//! any other backend, a missing library, or a lost daemon is a hard
//! startup or runtime error, never a silent direct-open fallback.
//!
//! This is the only module in src/ that uses unsafe. The surface is one
//! dlopen handle, seven resolved symbols, and two listener callbacks.
//! All raw pointers stay inside Session; Session is !Send and !Sync.

use core::ffi::{c_char, c_int, c_void};
use std::os::fd::{AsFd, BorrowedFd, FromRawFd, OwnedFd};
use std::sync::atomic::{AtomicBool, Ordering};

const RTLD_NOW: c_int = 2;

#[repr(C)]
struct Libseat {
    _opaque: [u8; 0],
}

#[repr(C)]
struct SeatListener {
    enable_seat: Option<unsafe extern "C" fn(*mut Libseat, *mut c_void)>,
    disable_seat: Option<unsafe extern "C" fn(*mut Libseat, *mut c_void)>,
}

extern "C" {
    fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> c_int;
    fn dlerror() -> *const c_char;
}

type OpenSeatFn = unsafe extern "C" fn(*const SeatListener, *mut c_void) -> *mut Libseat;
type IntFn = unsafe extern "C" fn(*mut Libseat) -> c_int;
type OpenDeviceFn = unsafe extern "C" fn(*mut Libseat, *const c_char, *mut c_int) -> c_int;
type CloseDeviceFn = unsafe extern "C" fn(*mut Libseat, c_int) -> c_int;
type GetFdFn = unsafe extern "C" fn(*mut Libseat) -> c_int;
type DispatchFn = unsafe extern "C" fn(*mut Libseat, c_int) -> c_int;
type SeatNameFn = unsafe extern "C" fn(*mut Libseat) -> *const c_char;

#[derive(Clone, Copy, Debug)]
struct Fns {
    open_seat: OpenSeatFn,
    disable_seat: IntFn,
    close_seat: IntFn,
    open_device: OpenDeviceFn,
    close_device: CloseDeviceFn,
    get_fd: GetFdFn,
    dispatch: DispatchFn,
    seat_name: SeatNameFn,
}

#[derive(Debug)]
struct ListenerState {
    enabled: AtomicBool,
    disabled: AtomicBool,
}

unsafe extern "C" fn enable_cb(_seat: *mut Libseat, userdata: *mut c_void) {
    // SAFETY: userdata is our ListenerState, alive while the session is
    // open; this callback only runs during dispatch on the owning thread.
    let state = &*(userdata as *const ListenerState);
    state.enabled.store(true, Ordering::SeqCst);
}

unsafe extern "C" fn disable_cb(_seat: *mut Libseat, userdata: *mut c_void) {
    // SAFETY: same contract as enable_cb.
    let state = &*(userdata as *const ListenerState);
    state.disabled.store(true, Ordering::SeqCst);
}

fn last_error(context: &str) -> String {
    format!("{context}: {}", std::io::Error::last_os_error())
}

fn dl_error(context: &str) -> String {
    // SAFETY: dlerror returns a valid string or null; copy it immediately.
    let detail = unsafe {
        let p = dlerror();
        if p.is_null() {
            "unknown loader error".to_string()
        } else {
            core::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
        }
    };
    format!("{context}: {detail}")
}

fn cstr(bytes: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(bytes.len() + 1);
    v.extend_from_slice(bytes);
    v.push(0);
    v
}

/// An open libseat session. Owns the dlopen handle, the listener state,
/// and the session pointer in drop order: seat closes before its state.
#[derive(Debug)]
pub struct Session {
    seat: *mut Libseat,
    fns: Fns,
    state: Box<ListenerState>,
    lib: *mut c_void,
    fd: OwnedFd,
}

// Session holds raw pointers, so it is !Send and !Sync by default and
// stays confined to its owning thread.

impl Drop for Session {
    fn drop(&mut self) {
        // Best effort: the daemon may already be gone.
        // SAFETY: seat was returned by open_seat and not yet closed.
        unsafe {
            (self.fns.close_seat)(self.seat);
            dlclose(self.lib);
        }
    }
}

impl Session {
    /// Require the seatd backend, load libseat, and open the seat.
    pub fn open() -> Result<Self, String> {
        match std::env::var("LIBSEAT_BACKEND") {
            Ok(selected) if selected == "seatd" => {}
            other => {
                return Err(format!(
                    "LIBSEAT_BACKEND must be seatd for brokered devices, got {other:?}"
                ));
            }
        }
        // SAFETY: NUL-terminated literal; RTLD_NOW resolves everything now.
        let lib = unsafe { dlopen(c"libseat.so.1".as_ptr(), RTLD_NOW) };
        if lib.is_null() {
            return Err(dl_error("dlopen libseat.so.1"));
        }
        let fns: Fns;
        // SAFETY: NUL-terminated literals; each symbol is checked before use.
        unsafe {
            let load = |name: &core::ffi::CStr| -> Result<*mut c_void, String> {
                let p = dlsym(lib, name.as_ptr());
                if p.is_null() {
                    return Err(dl_error(&format!("dlsym {}", name.to_string_lossy())));
                }
                Ok(p)
            };
            let table = Fns {
                open_seat: core::mem::transmute::<*mut c_void, OpenSeatFn>(
                    load(c"libseat_open_seat")?,
                ),
                disable_seat: core::mem::transmute::<*mut c_void, IntFn>(
                    load(c"libseat_disable_seat")?,
                ),
                close_seat: core::mem::transmute::<*mut c_void, IntFn>(
                    load(c"libseat_close_seat")?,
                ),
                open_device: core::mem::transmute::<*mut c_void, OpenDeviceFn>(
                    load(c"libseat_open_device")?,
                ),
                close_device: core::mem::transmute::<*mut c_void, CloseDeviceFn>(
                    load(c"libseat_close_device")?,
                ),
                get_fd: core::mem::transmute::<*mut c_void, GetFdFn>(
                    load(c"libseat_get_fd")?,
                ),
                dispatch: core::mem::transmute::<*mut c_void, DispatchFn>(
                    load(c"libseat_dispatch")?,
                ),
                seat_name: core::mem::transmute::<*mut c_void, SeatNameFn>(
                    load(c"libseat_seat_name")?,
                ),
            };
            fns = table;
        }
        let mut state = Box::new(ListenerState {
            enabled: AtomicBool::new(false),
            disabled: AtomicBool::new(false),
        });
        let listener = SeatListener {
            enable_seat: Some(enable_cb),
            disable_seat: Some(disable_cb),
        };
        // SAFETY: listener outlives the call; state outlives the session.
        let seat = unsafe {
            (fns.open_seat)(
                &listener,
                (&mut *state as *mut ListenerState).cast::<c_void>(),
            )
        };
        if seat.is_null() {
            // SAFETY: seat was never opened; only the library handle leaks
            // on this path, closed here.
            unsafe {
                dlclose(lib);
            }
            return Err(last_error("libseat_open_seat"));
        }
        // SAFETY: get_fd returns a borrowed daemon fd; dup our own copy.
        let raw = unsafe { (fns.get_fd)(seat) };
        if raw < 0 {
            unsafe {
                (fns.close_seat)(seat);
                dlclose(lib);
            }
            return Err(last_error("libseat_get_fd"));
        }
        let fd = unsafe { BorrowedFd::borrow_raw(raw) }
            .try_clone_to_owned()
            .map_err(|e| format!("dup seat fd: {e}"))?;
        let name: String;
        // SAFETY: seat is open; the name is owned by the session.
        unsafe {
            let p = (fns.seat_name)(seat);
            name = if p.is_null() {
                "?".into()
            } else {
                core::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
            };
        }
        eprintln!("TINYLAND_SEAT_OPEN seat={name}");
        Ok(Self {
            seat,
            fns,
            state,
            lib,
            fd,
        })
    }

    /// Borrow the duplicated session fd for the shared poll set.
    pub fn poll_fd(&self) -> BorrowedFd<'_> {
        self.fd.as_fd()
    }

    /// Dispatch pending seat events without waiting. Daemon loss is fatal.
    pub fn dispatch(&self) -> Result<(), String> {
        // SAFETY: seat is open; timeout 0 never blocks.
        let n = unsafe { (self.fns.dispatch)(self.seat, 0) };
        if n < 0 {
            return Err(last_error("libseat_dispatch"));
        }
        Ok(())
    }

    /// True after disable_seat until devices are dropped and acknowledged.
    pub fn take_disabled(&self) -> bool {
        self.state.disabled.swap(false, Ordering::SeqCst)
    }

    /// True after enable_seat until the session reopens its devices.
    pub fn take_enabled(&self) -> bool {
        self.state.enabled.swap(false, Ordering::SeqCst)
    }

    /// Acknowledge a disable event after all seat devices are dropped.
    pub fn acknowledge_disable(&self) -> Result<(), String> {
        // SAFETY: seat is open; caller dropped every device first.
        let r = unsafe { (self.fns.disable_seat)(self.seat) };
        if r != 0 {
            return Err(last_error("libseat_disable_seat"));
        }
        Ok(())
    }

    /// Open one brokered device. The returned fd is owned by the caller;
    /// pass the id to close_device when the fd is dropped or revoked.
    pub fn open_device(&self, path: &std::path::Path) -> Result<(OwnedFd, i32), String> {
        let raw = path.as_os_str().as_encoded_bytes();
        if raw.contains(&0) {
            return Err(format!("device path has NUL: {}", path.display()));
        }
        let arg = cstr(raw);
        let mut out: c_int = -1;
        // SAFETY: arg is NUL-terminated; out receives the new fd.
        let id = unsafe {
            (self.fns.open_device)(
                self.seat,
                arg.as_ptr() as *const c_char,
                &mut out,
            )
        };
        if id < 0 {
            return Err(format!(
                "libseat_open_device {}: {}",
                path.display(),
                std::io::Error::last_os_error()
            ));
        }
        if out < 0 {
            return Err(format!("libseat returned no fd for {}", path.display()));
        }
        // SAFETY: libseat transfers ownership of the new fd to the caller.
        let fd = unsafe { OwnedFd::from_raw_fd(out) };
        Ok((fd, id))
    }

    /// Release the broker side of a device whose fd was already dropped.
    pub fn close_device(&self, id: i32, origin: &str) -> Result<(), String> {
        // SAFETY: id came from open_device on this session.
        let r = unsafe { (self.fns.close_device)(self.seat, id) };
        if r != 0 {
            return Err(last_error(&format!("libseat_close_device {origin}")));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn backend_is_required_to_be_seatd() {
        let _guard = ENV_LOCK.lock().unwrap();
        let saved = std::env::var_os("LIBSEAT_BACKEND");
        unsafe {
            std::env::remove_var("LIBSEAT_BACKEND");
        }
        assert!(Session::open().unwrap_err().contains("LIBSEAT_BACKEND"));
        unsafe {
            std::env::set_var("LIBSEAT_BACKEND", "builtin");
        }
        assert!(Session::open().unwrap_err().contains("LIBSEAT_BACKEND"));
        match saved {
            Some(v) => unsafe { std::env::set_var("LIBSEAT_BACKEND", v) },
            None => unsafe { std::env::remove_var("LIBSEAT_BACKEND") },
        }
    }

    #[test]
    fn open_without_library_is_a_hard_error() {
        let _guard = ENV_LOCK.lock().unwrap();
        if dlopen_present() {
            return;
        }
        let saved = std::env::var_os("LIBSEAT_BACKEND");
        unsafe {
            std::env::set_var("LIBSEAT_BACKEND", "seatd");
        }
        let err = Session::open().unwrap_err();
        assert!(err.contains("libseat"), "unexpected: {err}");
        match saved {
            Some(v) => unsafe { std::env::set_var("LIBSEAT_BACKEND", v) },
            None => unsafe { std::env::remove_var("LIBSEAT_BACKEND") },
        }
    }

    fn dlopen_present() -> bool {
        // SAFETY: literal only; never dereferenced.
        let probe = c"libseat.so.1";
        unsafe {
            let h = dlopen(probe.as_ptr(), RTLD_NOW);
            if h.is_null() {
                return false;
            }
            dlclose(h);
            true
        }
    }
}
