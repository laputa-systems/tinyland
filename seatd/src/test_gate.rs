use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

// fork() copies every descriptor in the process until the child execs, so a
// child launched while another test thread owns a listener keeps that
// listener connectable after the thread drops it.
static GATE: RwLock<()> = RwLock::new(());

pub fn listeners() -> RwLockReadGuard<'static, ()> {
    GATE.read().unwrap_or_else(|e| e.into_inner())
}

pub fn child_launch() -> RwLockWriteGuard<'static, ()> {
    GATE.write().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admit::{open_socket, probe_live, unlink_path, ExistingSocket};
    use std::os::unix::process::CommandExt;
    use std::process::Command;
    use std::sync::mpsc;

    fn pipe() -> (i32, i32) {
        let mut fds = [0; 2];
        assert_eq!(unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) }, 0);
        (fds[0], fds[1])
    }

    fn poll_in(fd: i32, ms: i32) -> bool {
        let mut p = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        unsafe { libc::poll(&mut p, 1, ms) == 1 }
    }

    #[test]
    fn child_launch_never_overlaps_a_listener_that_is_being_dropped() {
        let mut path = std::env::temp_dir();
        path.push(format!("seatd-gate-{}.sock", std::process::id()));
        let path = path.to_string_lossy().into_owned();
        let _ = std::fs::remove_file(&path);
        let (forked_r, forked_w) = pipe();
        let (_hold_r, hold_w) = pipe();
        let (listening_tx, listening_rx) = mpsc::channel();
        let scenario = {
            let path = path.clone();
            std::thread::spawn(move || {
                let _held = listeners();
                let fd = open_socket(&path, ExistingSocket::ReclaimStale, -1, -1).unwrap();
                listening_tx.send(()).unwrap();
                poll_in(forked_r, 300);
                drop(fd);
                let live = probe_live(&path);
                let _ = unlink_path(&path);
                let _ = std::fs::remove_file(format!("{path}.lock"));
                live
            })
        };
        listening_rx.recv().unwrap();
        let mut cmd = Command::new("true");
        unsafe {
            cmd.pre_exec(move || {
                libc::write(forked_w, b"x".as_ptr().cast(), 1);
                let mut p = libc::pollfd { fd: hold_w, events: libc::POLLIN, revents: 0 };
                libc::poll(&mut p, 1, 600);
                Ok(())
            });
        }
        let launch = child_launch();
        assert!(cmd.status().unwrap().success());
        drop(launch);
        assert!(!scenario.join().unwrap(), "a child launched during the scenario held the dropped listener");
        unsafe {
            libc::close(forked_r);
            libc::close(forked_w);
            libc::close(_hold_r);
            libc::close(hold_w);
        }
    }
}
