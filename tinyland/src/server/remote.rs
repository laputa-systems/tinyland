use super::*;
use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::time::{Duration, Instant};
use tinyland_remote::{Button, CompositorMessage, HostMessage, Size};

const DEADLINE: Duration = Duration::from_secs(5);
const MAX_SOURCES: usize = 64;
const WRITE_CHUNK: usize = 16 * 1024;

pub(super) struct Transport {
    input: File,
    output: File,
    incoming: Vec<u8>,
    incoming_deadline: Option<Instant>,
    outgoing: Vec<u8>,
    written: usize,
    deadline: Option<Instant>,
    serial: u64,
    flight: Option<u64>,
    sources: BTreeSet<u32>,
}

impl Transport {
    #[cfg(test)]
    pub(super) fn test_transport() -> Self {
        Self { input: File::open("/dev/null").unwrap(), output: File::options().write(true).open("/dev/null").unwrap(), incoming: Vec::new(), incoming_deadline: None, outgoing: Vec::new(), written: 0, deadline: None, serial: 0, flight: None, sources: BTreeSet::new() }
    }

    fn new(size: Size) -> Result<Self, String> {
        let input = File::from(std::io::stdin().as_fd().try_clone_to_owned().map_err(|e| e.to_string())?);
        let output = File::from(std::io::stdout().as_fd().try_clone_to_owned().map_err(|e| e.to_string())?);
        for fd in [input.as_fd(), output.as_fd()] {
            let flags = rustix::fs::fcntl_getfl(fd).map_err(|e| e.to_string())?;
            rustix::fs::fcntl_setfl(fd, flags | rustix::fs::OFlags::NONBLOCK).map_err(|e| e.to_string())?;
        }
        let mut outgoing = Vec::new();
        tinyland_remote::write_compositor(&mut outgoing, &CompositorMessage::Ready { size }, None).map_err(|e| e.to_string())?;
        Ok(Self { input, output, incoming: Vec::new(), incoming_deadline: None, outgoing, written: 0, deadline: Some(Instant::now() + DEADLINE), serial: 0, flight: None, sources: BTreeSet::new() })
    }

    pub(super) fn queue_frame(&mut self, state: &mut State, size: Size) -> Result<(), String> {
        self.serial = self.serial.checked_add(1).ok_or("frame serial exhausted")?;
        let pixels = state.submit_frame();
        self.outgoing.clear();
        tinyland_remote::write_compositor(&mut self.outgoing, &CompositorMessage::Frame { serial: self.serial, pixels }, Some(size)).map_err(|e| e.to_string())?;
        self.written = 0;
        self.flight = Some(self.serial);
        self.deadline = Some(Instant::now() + DEADLINE);
        Ok(())
    }

    pub(super) fn write(&mut self) -> Result<(), String> {
        while self.written < self.outgoing.len() {
            let end = self.outgoing.len().min(self.written + WRITE_CHUNK);
            match self.output.write(&self.outgoing[self.written..end]) {
                Ok(0) => return Err("remote output closed".into()),
                Ok(n) => self.written += n,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(format!("remote output: {e}")),
            }
        }
        if self.written == self.outgoing.len() {
            self.outgoing.clear(); self.written = 0;
            if self.flight.is_none() { self.deadline = None; }
        }
        Ok(())
    }

    fn read(&mut self, state: &mut State, size: Size) -> Result<(), String> {
        let mut buf = [0u8; 16384];
        match self.input.read(&mut buf) {
            Ok(0) => return Err("remote input closed".into()),
            Ok(n) => {
                if self.incoming.is_empty() { self.incoming_deadline = Some(Instant::now() + DEADLINE); }
                self.incoming.extend_from_slice(&buf[..n]);
            },
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::Interrupted => return Ok(()),
            Err(e) => return Err(format!("remote input: {e}")),
        }
        loop {
            if self.incoming.len() < 9 { break; }
            let header: &[u8; 9] = self.incoming[..9].try_into().unwrap();
            let message_size = tinyland_remote::host_message_size(header).map_err(|e| format!("remote header: {e}"))?;
            if self.incoming.len() < message_size { break; }
            let msg = tinyland_remote::read_host(&mut &self.incoming[..message_size]).map_err(|e| format!("remote message: {e}"))?.ok_or("missing remote message")?;
            self.incoming.drain(..message_size);
            self.handle(state, size, msg)?;
        }
        if self.incoming.is_empty() { self.incoming_deadline = None; }
        Ok(())
    }

    pub(super) fn handle(&mut self, state: &mut State, size: Size, msg: HostMessage) -> Result<(), String> {
        if let HostMessage::Presented { serial } = msg {
            if self.flight != Some(serial) || !self.outgoing.is_empty() { return Err("unexpected presentation acknowledgment".into()); }
            self.flight = None; self.deadline = None;
            state.complete_frame(now_ms());
            return Ok(());
        }
        let source = match &msg {
            HostMessage::Key { source, .. } | HostMessage::Pointer { source, .. } | HostMessage::Button { source, .. } | HostMessage::Wheel { source, .. } | HostMessage::ResetInput { source } | HostMessage::Clipboard { source, .. } => *source,
            HostMessage::Presented { .. } => unreachable!(),
        };
        if matches!(msg, HostMessage::ResetInput { .. }) {
            state.input_lost(InputId(source)); self.sources.remove(&source); return Ok(());
        }
        if !self.sources.contains(&source) && self.sources.len() >= MAX_SOURCES { return Err("too many remote input sources".into()); }
        self.sources.insert(source);
        let frame = match msg {
            HostMessage::Clipboard { text, .. } => {
                state.selection.replace(None);
                state.selection.host = Some(Arc::from(text.into_bytes()));
                state.send_selection();
                return Ok(());
            }
            HostMessage::Key { code, pressed, .. } if code <= 0x2ff => { state.input_event(InputId(source), InputEvent::Key { code, down: pressed }); return Ok(()); }
            HostMessage::Pointer { x, y, .. } if x < size.width() && y < size.height() => PointerFrame { motion: Some(input::Motion::Absolute { x: f64::from(x) / f64::from(size.width().saturating_sub(1).max(1)), y: f64::from(y) / f64::from(size.height().saturating_sub(1).max(1)) }), ..Default::default() },
            HostMessage::Button { button, pressed, .. } => PointerFrame { buttons: vec![(match button { Button::Left => 0x110, Button::Right => 0x111, Button::Middle => 0x112, Button::Back => 0x116, Button::Forward => 0x115 }, pressed)], ..Default::default() },
            HostMessage::Wheel { horizontal, vertical, .. } if (-120..=120).contains(&horizontal) && (-120..=120).contains(&vertical) => PointerFrame { wheel: (vertical, horizontal), ..Default::default() },
            _ => return Err("unsupported remote input".into()),
        };
        state.input_event(InputId(source), InputEvent::Pointer(frame));
        Ok(())
    }
}

pub fn run(config: &ServerConfig<'_>) -> i32 {
    match run_session(config) { Ok(()) => 0, Err(error) => { eprintln!("remote session: {error}"); 1 } }
}

fn run_session(config: &ServerConfig<'_>) -> Result<(), String> {
    let size = Size::new(config.width, config.height).ok_or("invalid remote size")?;
    let runtime_dir = validate_runtime_dir()?;
    let mut display = Display::<State>::new().map_err(|e| format!("display: {e}"))?;
    let mut dh = display.handle();
    create_globals(&dh);
    let socket_name = config.socket_name.to_str().ok_or("socket name must be UTF-8")?;
    let socket = ListeningSocket::bind(socket_name).map_err(|e| format!("socket: {e:?}"))?;
    let keyboard = Keyboard::new()?;
    let mut state = State::new(dh.clone(), (size.width(), size.height()), keyboard);
    state.has_pointer = true;
    let mut transport = Transport::new(size)?;
    let mut app = spawn_app(config.mode, &runtime_dir, socket_name, config.command).map_err(|e| format!("spawn: {e}"))?;
    eprintln!("TINYLAND_COMPOSITOR_READY socket={socket_name} size={}x{} path=remote", size.width(), size.height());
    let display_fd = display.as_fd().try_clone_to_owned().map_err(|e| e.to_string())?;
    let socket_fd = socket.as_fd().try_clone_to_owned().map_err(|e| e.to_string())?;
    let result = 'session: loop {
        if config.shutdown.signal().is_some() { break Ok(()); }
        if transport.deadline.is_some_and(|deadline| Instant::now() >= deadline) || transport.incoming_deadline.is_some_and(|deadline| Instant::now() >= deadline) { break Err("remote transport timed out".into()); }
        let timeout = rustix::time::Timespec { tv_sec: 0, tv_nsec: 100_000_000 };
        let mut fds = vec![rustix::event::PollFd::new(&display_fd, rustix::event::PollFlags::IN), rustix::event::PollFd::new(&socket_fd, rustix::event::PollFlags::IN), rustix::event::PollFd::new(&transport.input, rustix::event::PollFlags::IN)];
        if !transport.outgoing.is_empty() { fds.push(rustix::event::PollFd::new(&transport.output, rustix::event::PollFlags::OUT)); }
        fds.extend(state.selection.poll_fds());
        match rustix::event::poll(&mut fds, Some(&timeout)) {
            Ok(_) => {}, Err(e) if e == rustix::io::Errno::INTR => continue, Err(e) => break Err(format!("poll: {e}")),
        }
        drop(fds);
        state.selection.service_transfers();
        if let Err(e) = transport.write().and_then(|_| transport.read(&mut state, size)) { break Err(e); }
        if let Err(e) = app.reap() { break Err(e); }
        if app.exited() { break Ok(()); }
        loop {
            match socket.accept() {
                Ok(Some(stream)) => { if let Err(e) = dh.insert_client(stream, Arc::new(UdsClientData)) { eprintln!("insert client: {e}"); } },
                Ok(None) => break,
                Err(e) => break 'session Err(format!("accept: {e}")),
            }
        }
        if let Err(e) = display.dispatch_clients(&mut state) { break Err(format!("dispatch: {e}")); }
        if state.pending_present && transport.flight.is_none() && transport.outgoing.is_empty() {
            if let Err(e) = transport.queue_frame(&mut state, size) { break Err(e); }
        }
        if let Err(e) = display.flush_clients() { break Err(format!("flush: {e}")); }
    };
    state.suspend_input();
    result
}
