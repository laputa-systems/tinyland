use crate::log_error;
use rustix::fd::OwnedFd;
use std::io;
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};

pub struct Readiness(OwnedFd);

impl Readiness {
    pub fn acquire(fd: RawFd) -> io::Result<Readiness> {
        if unsafe { libc::fcntl(fd, libc::F_GETFD) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Readiness(unsafe { OwnedFd::from_raw_fd(fd) }))
    }

    pub fn notify(self) {
        let n = unsafe { libc::write(self.0.as_raw_fd(), b"\n".as_ptr().cast(), 1) };
        if n != 1 {
            let e = io::Error::last_os_error();
            log_error!("Could not write readiness signal: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::io::IntoRawFd;

    fn pipe() -> (OwnedFd, OwnedFd) {
        let mut fds = [0; 2];
        assert_eq!(unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) }, 0);
        unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) }
    }

    fn read_one(fd: &OwnedFd) -> isize {
        let mut b = [0u8; 1];
        unsafe { libc::read(fd.as_raw_fd(), b.as_mut_ptr().cast(), 1) }
    }

    #[test]
    fn notify_writes_one_byte_and_closes() {
        let (r, w) = pipe();
        Readiness::acquire(w.into_raw_fd()).unwrap().notify();
        assert_eq!(read_one(&r), 1);
        assert_eq!(read_one(&r), 0);
    }

    #[test]
    fn closed_descriptor_is_rejected() {
        let err = Readiness::acquire(i32::MAX).err().expect("closed descriptor accepted");
        assert_eq!(err.raw_os_error(), Some(libc::EBADF));
    }
}
