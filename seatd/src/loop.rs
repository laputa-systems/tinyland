use crate::admit;
use crate::conn::Conn;
use crate::{log_debug, log_error, log_info};
use rustix::fd::OwnedFd;
use std::io;
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};

const WATCHED: [libc::c_int; 4] = [libc::SIGUSR1, libc::SIGUSR2, libc::SIGINT, libc::SIGTERM];

#[allow(dead_code)]
struct ClientSlot {
    conn: Conn,
    pid: libc::pid_t,
    uid: libc::uid_t,
    gid: libc::gid_t,
    dead: bool,
}

pub struct Server {
    pub running: bool,
    listen_fd: Option<OwnedFd>,
    sig_fd: Option<OwnedFd>,
    old_mask: libc::sigset_t,
    pollfds: Vec<libc::pollfd>,
    clients: Vec<ClientSlot>,
    dirty: bool,
}

fn os_error() -> io::Error {
    io::Error::last_os_error()
}

impl Server {
    pub fn new() -> io::Result<Server> {
        let (sig_raw, old): (RawFd, libc::sigset_t) = unsafe {
            let mut mask: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut mask);
            for sig in WATCHED {
                libc::sigaddset(&mut mask, sig);
            }
            let mut old: libc::sigset_t = std::mem::zeroed();
            if libc::pthread_sigmask(libc::SIG_BLOCK, &mask, &mut old) != 0 {
                return Err(os_error());
            }
            let sig_fd = libc::signalfd(
                -1,
                &mask,
                libc::SFD_NONBLOCK | libc::SFD_CLOEXEC,
            );
            if sig_fd < 0 {
                let e = os_error();
                libc::pthread_sigmask(libc::SIG_SETMASK, &old, std::ptr::null_mut());
                return Err(e);
            }
            (sig_fd, old)
        };
        Ok(Server {
            running: true,
            listen_fd: None,
            sig_fd: Some(unsafe { OwnedFd::from_raw_fd(sig_raw) }),
            old_mask: old,
            pollfds: Vec::new(),
            clients: Vec::new(),
            dirty: true,
        })
    }

    pub fn set_listen(&mut self, fd: OwnedFd) {
        self.listen_fd = Some(fd);
    }

    fn rebuild(&mut self) {
        self.pollfds.clear();
        self.pollfds.push(libc::pollfd {
            fd: self.sig_fd.as_ref().unwrap().as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        });
        if let Some(listen) = self.listen_fd.as_ref() {
            self.pollfds.push(libc::pollfd {
                fd: listen.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            });
        }
        for client in self.clients.iter() {
            let mut events = libc::POLLIN;
            if client.conn.want_flush() {
                events |= libc::POLLOUT;
            }
            self.pollfds.push(libc::pollfd {
                fd: client.conn.raw_fd(),
                events,
                revents: 0,
            });
        }
        self.dirty = false;
    }

    fn handle_signals(&mut self) {
        let sig_fd = self.sig_fd.as_ref().unwrap().as_raw_fd();
        loop {
            let mut info: libc::signalfd_siginfo = unsafe { std::mem::zeroed() };
            let n = unsafe {
                libc::read(
                    sig_fd,
                    &mut info as *mut _ as *mut libc::c_void,
                    std::mem::size_of::<libc::signalfd_siginfo>(),
                )
            };
            if n != std::mem::size_of::<libc::signalfd_siginfo>() as isize {
                break;
            }
            match info.ssi_signo as libc::c_int {
                libc::SIGINT | libc::SIGTERM => {
                    self.running = false;
                }
                libc::SIGUSR1 => {
                    log_debug!("VT release request ignored: no seat yet");
                }
                libc::SIGUSR2 => {
                    log_debug!("VT acquire ignored: no seat yet");
                }
                _ => {}
            }
        }
    }

    fn handle_listen(&mut self) {
        let listen_fd = self.listen_fd.as_ref().unwrap().as_raw_fd();
        match admit::accept_client(listen_fd) {
            Some((fd, creds)) => {
                self.clients.push(ClientSlot {
                    conn: Conn::new(fd),
                    pid: creds.pid,
                    uid: creds.uid,
                    gid: creds.gid,
                    dead: false,
                });
                self.dirty = true;
            }
            None => {}
        }
    }

    fn handle_client_writable(&mut self, idx: usize) {
        let client = &mut self.clients[idx];
        match client.conn.flush() {
            Ok(_) => {
                self.dirty = true;
            }
            Err(e) if e == libc::EAGAIN => {
                self.dirty = true;
            }
            Err(e) => {
                let err = io::Error::from_raw_os_error(e);
                log_error!("Could not flush client connection: {err}");
                client.dead = true;
            }
        }
    }

    fn handle_client_readable(&mut self, idx: usize) {
        let client = &mut self.clients[idx];
        match client.conn.read() {
            Ok(0) => {
                log_error!("Could not read client connection: zero-length read");
                client.dead = true;
            }
            Ok(_) => {
                // Frame dispatch arrives in R3; bytes accumulate until then.
            }
            Err(e) if e == libc::EAGAIN => {}
            Err(e) => {
                let err = io::Error::from_raw_os_error(e);
                log_error!("Could not read client connection: {err}");
                client.dead = true;
            }
        }
    }

    fn sweep_dead(&mut self) {
        let before = self.clients.len();
        self.clients.retain(|c| !c.dead);
        if self.clients.len() != before {
            log_info!("Client disconnected");
            self.dirty = true;
        }
    }

    pub fn poll_once(&mut self) -> io::Result<()> {
        if self.dirty {
            self.rebuild();
        }
        let n = unsafe {
            libc::poll(
                self.pollfds.as_mut_ptr(),
                self.pollfds.len() as libc::nfds_t,
                -1,
            )
        };
        if n < 0 {
            let e = os_error();
            if e.raw_os_error() == Some(libc::EINTR) {
                return Ok(());
            }
            return Err(e);
        }
        let sig_revents = self.pollfds[0].revents;
        if sig_revents & libc::POLLIN != 0 {
            self.handle_signals();
        }
        if self.listen_fd.is_some() && self.pollfds.len() > 1 {
            let listen_revents = self.pollfds[1].revents;
            if listen_revents & (libc::POLLERR | libc::POLLHUP) != 0 {
                let listen_fd = self.listen_fd.as_ref().unwrap().as_raw_fd();
                unsafe {
                    libc::shutdown(listen_fd, libc::SHUT_RDWR);
                }
                self.running = false;
                log_error!("Server socket received an error");
                return Ok(());
            }
            if listen_revents & libc::POLLIN != 0 {
                self.handle_listen();
            }
        }
        let base = if self.listen_fd.is_some() { 2 } else { 1 };
        for i in 0..self.clients.len() {
            // A client accepted during this dispatch has no pollfd yet;
            // it is picked up on the next rebuild.
            if base + i >= self.pollfds.len() {
                continue;
            }
            let revents = self.pollfds[base + i].revents;
            if revents == 0 {
                continue;
            }
            if revents & libc::POLLERR != 0 {
                log_error!("Connection error");
                self.clients[i].dead = true;
                continue;
            }
            if revents & libc::POLLHUP != 0 {
                log_info!("Client disconnected");
                self.clients[i].dead = true;
                continue;
            }
            if revents & libc::POLLOUT != 0 {
                self.handle_client_writable(i);
            }
            if revents & libc::POLLIN != 0 {
                self.handle_client_readable(i);
            }
        }
        self.sweep_dead();
        Ok(())
    }

    pub fn run(&mut self) {
        while self.running {
            if let Err(e) = self.poll_once() {
                log_error!("Poller failed: {e}");
                break;
            }
        }
    }

    pub fn finish(mut self) {
        unsafe {
            drop(self.listen_fd.take());
            drop(self.sig_fd.take());
            libc::pthread_sigmask(libc::SIG_SETMASK, &self.old_mask, std::ptr::null_mut());
        }
    }
}
