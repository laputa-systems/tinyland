use super::*;
use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::time::{Duration, Instant};
use tinyland_remote::{Button, CompositorMessage, HostMessage, Size};

const DEADLINE: Duration = Duration::from_secs(5);
/// The virtual display refresh: surfaces are presented and their frame callbacks completed at most this often.
const REFRESH: Duration = Duration::from_micros(16_667);
/// The shortest time between two frames sent to the host; presentations in between accumulate their damage.
const SHIP_INTERVAL: Duration = Duration::from_micros(33_333);
/// Output rectangles per frame sent to the host; more are sent as their bounding box.
const MAX_SHIP_RECTS: usize = 16;
const MAX_SOURCES: usize = 64;
const WRITE_CHUNK: usize = 64 * 1024;

pub(super) struct Transport {
    input: File,
    output: File,
    incoming: Vec<u8>,
    incoming_deadline: Option<Instant>,
    outgoing: Vec<u8>,
    written: usize,
    deadline: Option<Instant>,
    serial: u64,
    /// The frame awaiting acknowledgment, and whether its acknowledgment completes the submitted callbacks.
    flight: Option<(u64, bool)>,
    sources: BTreeSet<u32>,
    last_present: Option<Instant>,
    last_ship: Option<Instant>,
    /// Output damage presented but not yet sent to the host.
    unsent: crate::damage::Damage,
}

impl Transport {
    #[cfg(test)]
    pub(super) fn test_transport() -> Self {
        Self { input: File::open("/dev/null").unwrap(), output: File::options().write(true).open("/dev/null").unwrap(), incoming: Vec::new(), incoming_deadline: None, outgoing: Vec::new(), written: 0, deadline: None, serial: 0, flight: None, sources: BTreeSet::new(), last_present: None, last_ship: None, unsent: Default::default() }
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
        Ok(Self { input, output, incoming: Vec::new(), incoming_deadline: None, outgoing, written: 0, deadline: Some(Instant::now() + DEADLINE), serial: 0, flight: None, sources: BTreeSet::new(), last_present: None, last_ship: None, unsent: Default::default() })
    }

    /// The encoded bytes waiting to be written to the host.
    #[cfg(test)]
    pub(super) fn sent(&self) -> &[u8] { &self.outgoing[self.written..] }

    /// Presents the scene now and sends everything unsent, completing the callbacks on acknowledgment.
    #[cfg(test)]
    pub(super) fn queue_frame(&mut self, state: &mut State, size: Size) -> Result<(), String> {
        self.present(state);
        self.ship(state, size, true, Instant::now())
    }

    /// Advances presentation at `now`: presents a dirty scene at most once per refresh and sends accumulated
    /// damage at most once per ship interval. Returns how long until the next step is due, if one is waiting.
    pub(super) fn tick(&mut self, state: &mut State, size: Size, now: Instant) -> Result<Option<Duration>, String> {
        if self.flight.is_some() || !self.outgoing.is_empty() { return Ok(None); }
        let ship_at = self.last_ship.map_or(now, |at| at + SHIP_INTERVAL);
        if state.pending_present {
            let present_at = self.last_present.map_or(now, |at| at + REFRESH);
            if now < present_at { return Ok(Some(present_at - now)); }
            self.last_present = Some(now);
            self.present(state);
            if self.unsent.is_full() || !self.unsent.rects().is_empty() {
                if now >= ship_at { return self.ship(state, size, true, now).map(|()| None); }
                state.complete_frame(now_ms());
                return Ok(Some(ship_at - now));
            }
            state.complete_frame(now_ms());
            return Ok(None);
        }
        if self.unsent.is_full() || !self.unsent.rects().is_empty() {
            if now >= ship_at { return self.ship(state, size, false, now).map(|()| None); }
            return Ok(Some(ship_at - now));
        }
        Ok(None)
    }

    fn present(&mut self, state: &mut State) {
        for (x0, y0, x1, y1) in state.submit_damage() {
            self.unsent.add(crate::positioner::Rect { x: x0 as i32, y: y0 as i32, width: (x1 - x0) as i32, height: (y1 - y0) as i32 });
        }
    }

    fn ship(&mut self, state: &State, size: Size, callbacks: bool, now: Instant) -> Result<(), String> {
        self.serial = self.serial.checked_add(1).ok_or("frame serial exhausted")?;
        let rects: Vec<tinyland_remote::Rect> = crate::damage::output_rects(&self.unsent, size.width(), size.height(), MAX_SHIP_RECTS).into_iter()
            .map(|(x0, y0, x1, y1)| tinyland_remote::Rect { x: x0, y: y0, width: x1 - x0, height: y1 - y0 }).collect();
        self.unsent.clear();
        self.outgoing.clear();
        tinyland_remote::write_damage(&mut self.outgoing, self.serial, size, &state.framebuffer, &rects).map_err(|e| e.to_string())?;
        self.written = 0;
        self.flight = Some((self.serial, callbacks));
        self.last_ship = Some(now);
        self.deadline = Some(now + DEADLINE);
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
            let Some((_, callbacks)) = self.flight.filter(|(flight, _)| *flight == serial && self.outgoing.is_empty()) else {
                return Err("unexpected presentation acknowledgment".into());
            };
            self.flight = None; self.deadline = None;
            if callbacks { state.complete_frame(now_ms()); }
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
    let mut wake: Option<Duration> = None;
    let result = 'session: loop {
        if config.shutdown.signal().is_some() { break Ok(()); }
        if transport.deadline.is_some_and(|deadline| Instant::now() >= deadline) || transport.incoming_deadline.is_some_and(|deadline| Instant::now() >= deadline) { break Err("remote transport timed out".into()); }
        let wait = wake.unwrap_or(Duration::from_millis(100)).min(Duration::from_millis(100));
        let timeout = rustix::time::Timespec { tv_sec: 0, tv_nsec: i64::from(wait.subsec_nanos()) };
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
        wake = match transport.tick(&mut state, size, Instant::now()) { Ok(wake) => wake, Err(e) => break Err(e) };
        if let Err(e) = display.flush_clients() { break Err(format!("flush: {e}")); }
    };
    state.suspend_input();
    result
}
