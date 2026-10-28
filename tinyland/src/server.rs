use crate::input::{self, Keyboard, SeatData};
use crate::output::DrmOutput;
use crate::selection::SelectionState;
use crate::shell::{ToplevelData, XdgSurfaceData};
use crate::shm::{self, ShmBufferData, ShmFormat, ShmPoolData};
use crate::surface::SurfaceData;
use std::collections::{HashMap, HashSet};
use std::os::unix::io::AsFd;
use std::sync::Arc;
use wayland_protocols::xdg::shell::server::xdg_wm_base;
use wayland_server::backend::{ClientData, ClientId, ObjectId};
use wayland_server::protocol::{
    wl_buffer, wl_callback, wl_compositor, wl_data_device_manager, wl_output, wl_seat, wl_shm,
    wl_subcompositor, wl_surface,
};
use wayland_server::{Display, DisplayHandle, ListeningSocket, Resource};

struct UdsClientData;

impl ClientData for UdsClientData {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, reason: wayland_server::backend::DisconnectReason) {
        eprintln!("TINYLAND_CLIENT_DISCONNECTED {reason:?}");
    }
}

pub struct State {
    dh: DisplayHandle,
    output: DrmOutput,
    out_size: (u32, u32),
    serial: u32,
    keyboard: Keyboard,
    surfaces: HashMap<ObjectId, SurfaceData>,
    toplevels: HashMap<ObjectId, ToplevelData>,
    xdg_surfaces: HashMap<ObjectId, XdgSurfaceData>,
    popups: HashSet<ObjectId>,
    subsurfaces: HashMap<ObjectId, ObjectId>,
    pools: HashMap<ObjectId, ShmPoolData>,
    buffers: HashMap<ObjectId, (wl_buffer::WlBuffer, ShmBufferData)>,
    seats: Vec<SeatData>,
    outputs: Vec<(ClientId, wl_output::WlOutput)>,
    selection: SelectionState,
    regions: HashMap<ObjectId, Vec<(i32, i32, i32, i32)>>,
    pending_present: bool,
    mapped_logged: bool,
    ready_logged: bool,
}

impl State {
    fn next_serial(&mut self) -> u32 {
        self.serial = self.serial.wrapping_add(1).max(1);
        self.serial
    }

    fn find_surface_by_resource(&self, resource: &wl_surface::WlSurface) -> Option<ObjectId> {
        let id = resource.id();
        if self.surfaces.contains_key(&id) {
            Some(id)
        } else {
            None
        }
    }

    fn apply_commit(&mut self, surf_id: &ObjectId) {
        let (pending, parent, sync, children) = match self.surfaces.get(surf_id) {
            Some(s) => (
                s.pending.clone(),
                s.parent.clone(),
                s.subsurface_sync,
                s.children.clone(),
            ),
            None => return,
        };
        if parent.is_some() && sync {
            return;
        }
        if let Some(s) = self.surfaces.get_mut(surf_id) {
            if let Some(buf) = pending.buffer {
                s.current.buffer = buf;
            }
            s.current.scale = pending.scale;
            s.pending.buffer = None;
            s.pending.damage_full = false;
        }
        for child in children {
            if let Some(c) = self.surfaces.get(&child) {
                if c.parent.as_ref() == Some(surf_id) && c.subsurface_sync {
                    self.apply_commit(&child);
                }
            }
        }
        if self.is_mapped_toplevel(surf_id) {
            self.pending_present = true;
        } else if self.toplevel_for_surface(surf_id).is_some() {
            self.try_map(surf_id);
        } else if parent.is_some() {
            self.pending_present = true;
        }
    }

    fn toplevel_for_surface(&self, surf_id: &ObjectId) -> Option<ObjectId> {
        self.toplevels
            .iter()
            .find(|(_, t)| &t.surface == surf_id)
            .map(|(id, _)| id.clone())
    }

    fn is_mapped_toplevel(&self, surf_id: &ObjectId) -> bool {
        self.toplevels
            .values()
            .any(|t| &t.surface == surf_id && t.mapped)
    }

    fn try_map(&mut self, surf_id: &ObjectId) {
        let top_id = match self.toplevel_for_surface(surf_id) {
            Some(id) => id,
            None => return,
        };
        let (has_buffer, acked_ok) = match (self.surfaces.get(surf_id), self.toplevels.get(&top_id))
        {
            (Some(s), Some(t)) => (
                s.current.buffer.is_some(),
                t.acked_configure >= t.last_configure && t.last_configure > 0,
            ),
            _ => return,
        };
        if !has_buffer || !acked_ok {
            return;
        }
        if let Some(t) = self.toplevels.get_mut(&top_id) {
            if t.mapped {
                return;
            }
            t.mapped = true;
        }
        let serial = self.next_serial();
        let client_id = self.client_of_surface(surf_id);
        if let Some(s) = self.surfaces.get(surf_id) {
            for (cid, out) in &self.outputs {
                if Some(cid) == client_id.as_ref() {
                    s.resource.enter(out);
                }
            }
        }
        self.send_pointer_enter(surf_id, serial);
        self.send_keyboard_enter(surf_id, serial);
        self.pending_present = true;
        if !self.mapped_logged {
            self.mapped_logged = true;
            eprintln!("TINYLAND_SURFACE_MAPPED");
        }
    }

    fn client_of_surface(&mut self, surf_id: &ObjectId) -> Option<ClientId> {
        self.dh.get_client(surf_id.clone()).map(|c| c.id()).ok()
    }

    fn send_pointer_enter(&mut self, surf_id: &ObjectId, serial: u32) {
        let resource = match self.surfaces.get(surf_id) {
            Some(s) => s.resource.clone(),
            None => return,
        };
        for seat in &mut self.seats {
            for ptr in &seat.pointers {
                ptr.enter(serial, &resource, 0.0, 0.0);
                ptr.frame();
            }
        }
    }

    fn send_keyboard_enter(&mut self, surf_id: &ObjectId, serial: u32) {
        let surface = &self.surfaces[surf_id].resource;
        let [depressed, latched, locked, group] = self.keyboard.modifiers();
        for seat in &mut self.seats {
            if seat.resource.client() != surface.client() { continue; }
            seat.keyboard_focus = Some(surf_id.clone());
            for keyboard in &seat.keyboards {
                keyboard.enter(serial, surface, Vec::new());
                keyboard.modifiers(serial, depressed, latched, locked, group);
            }
        }
    }

    fn key(&mut self, code: u16, down: bool) {
        let serial = self.next_serial();
        let [depressed, latched, locked, group] = self.keyboard.update(code, down);
        for seat in &self.seats {
            if seat.keyboard_focus.is_none() { continue; }
            for keyboard in &seat.keyboards {
                keyboard.key(serial, now_ms(), code as u32, if down {
                    wayland_server::protocol::wl_keyboard::KeyState::Pressed
                } else { wayland_server::protocol::wl_keyboard::KeyState::Released });
                keyboard.modifiers(serial, depressed, latched, locked, group);
            }
        }
    }

    fn present_if_needed(&mut self) {
        if !self.pending_present {
            return;
        }
        self.pending_present = false;
        let top_surface = match self
            .toplevels
            .values()
            .find(|t| t.mapped)
            .map(|t| t.surface.clone())
        {
            Some(s) => s,
            None => return,
        };
        let (w, h) = self.out_size;
        let mut fb = vec![0u8; w as usize * h as usize * 4];
        let mut used_buffers: Vec<ObjectId> = Vec::new();
        let mut fired: Vec<wl_callback::WlCallback> = Vec::new();
        self.blit_surface(&top_surface, 0, 0, w, h, &mut fb, &mut used_buffers, &mut fired);
        if output_present(&mut self.output, &fb, w, h).is_err() {
            return;
        }
        let time = now_ms();
        for cb in fired {
            cb.done(time);
        }
        for buf_id in used_buffers {
            if let Some((res, _)) = self.buffers.get(&buf_id) {
                res.release();
            }
        }
    }

    fn blit_surface(
        &mut self,
        surf_id: &ObjectId,
        ox: i32,
        oy: i32,
        out_w: u32,
        out_h: u32,
        fb: &mut [u8],
        used: &mut Vec<ObjectId>,
        fired: &mut Vec<wl_callback::WlCallback>,
    ) {
        let (buf_id, children, pos) = match self.surfaces.get_mut(surf_id) {
            Some(s) => {
                let mut cbs = std::mem::take(&mut s.callbacks);
                fired.append(&mut cbs);
                (s.current.buffer.clone(), s.children.clone(), s.subsurface_pos)
            }
            None => return,
        };
        if let Some(id) = buf_id {
            let rows = self.buffers.get(&id).and_then(|(_, d)| {
                self.pools.get(&d.pool).and_then(|p| {
                    shm::read_rows(&p.mmap, d)
                        .ok()
                        .map(|r| (r, d.format, d.width, d.height))
                })
            });
            let rows = match rows {
                Some(v) => v,
                None => {
                    log::warn!("present: missing pool or buffer");
                    return;
                }
            };
            if rows.2 > 0 {
                blit_rows(
                    fb,
                    out_w,
                    out_h,
                    &rows.0,
                    rows.2 as u32,
                    rows.3 as u32,
                    ox + pos.0,
                    oy + pos.1,
                    rows.1,
                );
                if !used.contains(&id) {
                    used.push(id);
                }
            }
        }
        for child in children {
            let (cx, cy) = match self.surfaces.get(&child) {
                Some(c) => c.subsurface_pos,
                None => continue,
            };
            let _ = (cx, cy);
            self.blit_surface(&child, ox + pos.0, oy + pos.1, out_w, out_h, fb, used, fired);
        }
    }
}

fn output_present(output: &mut DrmOutput, fb: &[u8], w: u32, h: u32) -> Result<(), String> {
    output.present_pixels(fb, w, h)
}

fn blit_rows(
    fb: &mut [u8],
    out_w: u32,
    out_h: u32,
    rows: &[u8],
    src_w: u32,
    src_h: u32,
    ox: i32,
    oy: i32,
    format: ShmFormat,
) {
    for sy in 0..src_h as i32 {
        let dy = oy + sy;
        if dy < 0 || dy >= out_h as i32 {
            continue;
        }
        for sx in 0..src_w as i32 {
            let dx = ox + sx;
            if dx < 0 || dx >= out_w as i32 {
                continue;
            }
            let s = (sy as usize * src_w as usize + sx as usize) * 4;
            let d = (dy as usize * out_w as usize + dx as usize) * 4;
            match format {
                ShmFormat::Xrgb8888 => {
                    fb[d] = rows[s];
                    fb[d + 1] = rows[s + 1];
                    fb[d + 2] = rows[s + 2];
                    fb[d + 3] = 0xFF;
                }
                ShmFormat::Argb8888 => {
                    let a = rows[s + 3] as u32;
                    fb[d] = ((rows[s] as u32 * a / 255) & 0xFF) as u8;
                    fb[d + 1] = ((rows[s + 1] as u32 * a / 255) & 0xFF) as u8;
                    fb[d + 2] = ((rows[s + 2] as u32 * a / 255) & 0xFF) as u8;
                    fb[d + 3] = 0xFF;
                }
            }
        }
    }
}

fn now_ms() -> u32 {
    use rustix::time::{clock_gettime, ClockId};
    let ts = clock_gettime(ClockId::Monotonic);
    (ts.tv_sec as u64 * 1000 + ts.tv_nsec as u64 / 1_000_000) as u32
}

pub fn run(mode: &str, device: &std::path::Path, width: u32, height: u32) -> i32 {
    let mut output = match DrmOutput::open(device, width, height) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("display init failed: {e}");
            return 1;
        }
    };
    let (w, h) = output.size();
    if output.fill_solid(0, 0, 0).is_err() {
        eprintln!("initial clear failed");
        return 1;
    }
    eprintln!("TINYLAND_DRM_OK size={w}x{h} path={}", output.mode_path());

    let mut display = match Display::<State>::new() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("wayland display init failed: {e}");
            return 1;
        }
    };
    let mut dh = display.handle();
    dh.create_global::<State, wl_compositor::WlCompositor, ()>(4, ());
    dh.create_global::<State, wl_subcompositor::WlSubcompositor, ()>(1, ());
    dh.create_global::<State, wl_shm::WlShm, ()>(1, ());
    dh.create_global::<State, wl_output::WlOutput, ()>(2, ());
    dh.create_global::<State, wl_seat::WlSeat, ()>(5, ());
    dh.create_global::<State, wl_data_device_manager::WlDataDeviceManager, ()>(3, ());
    dh.create_global::<State, xdg_wm_base::XdgWmBase, ()>(1, ());

    let socket = match ListeningSocket::bind("wayland-0") {
        Ok(s) => s,
        Err(e) => {
            eprintln!("socket bind failed: {e:?}");
            return 1;
        }
    };
    let sock_name = socket.socket_name().map(|s| s.to_string_lossy().into_owned());
    eprintln!("TINYLAND_SOCKET {sock_name:?}");
    eprintln!("TINYLAND_COMPOSITOR_READY");

    let keyboard = match Keyboard::new() {
        Ok(keyboard) => keyboard,
        Err(error) => { eprintln!("keyboard init: {error}"); return 1; }
    };
    let mut devices = match input::discover_keyboards() {
        Ok(devices) => devices,
        Err(error) => { eprintln!("keyboard discovery: {error}"); return 1; }
    };
    let mut state = State {
        dh: dh.clone(),
        output,
        out_size: (w, h),
        serial: 0,
        keyboard,
        surfaces: HashMap::new(),
        toplevels: HashMap::new(),
        xdg_surfaces: HashMap::new(),
        popups: HashSet::new(),
        subsurfaces: HashMap::new(),
        pools: HashMap::new(),
        buffers: HashMap::new(),
        seats: Vec::new(),
        outputs: Vec::new(),
        selection: SelectionState::default(),
        regions: HashMap::new(),
        pending_present: false,
        mapped_logged: false,
        ready_logged: false,
    };

    let mut app = match spawn_app(mode) {
        Ok(app) => app,
        Err(error) => { eprintln!("foot spawn failed: {error}"); return 1; }
    };

    let display_fd = display.as_fd().try_clone_to_owned().expect("clone display fd");
    let socket_fd = (&socket).as_fd().try_clone_to_owned().expect("clone socket fd");
    let poll_timeout = rustix::time::Timespec { tv_sec: 0, tv_nsec: 500_000_000 };
    loop {
        let mut fds = vec![
            rustix::event::PollFd::new(&display_fd, rustix::event::PollFlags::IN),
            rustix::event::PollFd::new(&socket_fd, rustix::event::PollFlags::IN),
        ];
        for device in &devices {
            fds.push(rustix::event::PollFd::new(device, rustix::event::PollFlags::IN));
        }
        match rustix::event::poll(&mut fds, Some(&poll_timeout)) {
            Ok(_) => {}
            Err(e) => {
                if e == rustix::io::Errno::INTR {
                    continue;
                }
                eprintln!("poll failed: {e}");
                return 1;
            }
        }
        drop(fds);
        for device in &mut devices {
            match input::read_keys(device) {
                Ok(keys) => for (code, down) in keys { state.key(code, down); },
                Err(error) => { eprintln!("{error}"); return 1; }
            }
        }
        if let Err(error) = app.reap() { eprintln!("{error}"); return 1; }
        loop {
            match socket.accept() {
                Ok(Some(stream)) => {
                    if let Err(e) = dh.insert_client(stream, Arc::new(UdsClientData)) {
                        eprintln!("insert client failed: {e}");
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    eprintln!("accept failed: {e}");
                    break;
                }
            }
        }
        if let Err(e) = display.dispatch_clients(&mut state) {
            eprintln!("dispatch failed: {e}");
            return 1;
        }
        state.present_if_needed();
        if let Err(e) = display.flush_clients() {
            eprintln!("flush failed: {e}");
            return 1;
        }
    }
}

struct App(Option<std::process::Child>);

impl App {
    fn reap(&mut self) -> Result<(), String> {
        if let Some(child) = &mut self.0 {
            if let Some(status) = child.try_wait().map_err(|e| format!("foot wait: {e}"))? {
                eprintln!("TINYLAND_CHILD_EXIT status={status}");
                self.0 = None;
                if !status.success() { return Err(format!("foot exited: {status}")); }
            }
        }
        Ok(())
    }
}

impl Drop for App {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            if let Err(error) = child.kill() { eprintln!("foot cleanup kill: {error}"); }
            if let Err(error) = child.wait() { eprintln!("foot cleanup wait: {error}"); }
        }
    }
}

fn spawn_app(mode: &str) -> std::io::Result<App> {
    let test_arg = if mode == "wayland-pixels" { "wayland-pixels" } else { "smoke" };
    eprintln!("TINYLAND_SPAWN_FOOT mode={test_arg}");
    let child = std::process::Command::new("foot")
        .arg("/usr/local/bin/guest-test")
        .arg(test_arg)
        .env("XDG_RUNTIME_DIR", "/tmp")
        .env("WAYLAND_DISPLAY", "wayland-0")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::inherit())
        .spawn()?;
    Ok(App(Some(child)))
}

mod dispatch_impl;
