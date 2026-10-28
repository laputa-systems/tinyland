use crate::input::{self, Keyboard, SeatData};
use crate::output::DrmOutput;
use crate::selection::SelectionState;
use crate::shell::{ToplevelData, XdgSurfaceData};
use crate::shm::{self, ShmBufferData, ShmFormat, ShmPoolData};
use crate::surface::{SurfaceData, SurfaceUpdate};
use std::collections::HashMap;
use std::os::unix::io::AsFd;
use std::sync::Arc;
use wayland_protocols::xdg::shell::server::xdg_wm_base;
use wayland_server::backend::{ClientData, ClientId, ObjectId};
use wayland_server::protocol::{
    wl_buffer, wl_compositor, wl_data_device_manager, wl_output, wl_seat, wl_shm,
    wl_subcompositor,
};
use wayland_server::{Client, Display, DisplayHandle, ListeningSocket, Resource};

struct UdsClientData;

impl ClientData for UdsClientData {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, reason: wayland_server::backend::DisconnectReason) {
        eprintln!("TINYLAND_CLIENT_DISCONNECTED {reason:?}");
    }
}

pub struct State {
    dh: DisplayHandle,
    out_size: (u32, u32),
    serial: u32,
    keyboard: Keyboard,
    surfaces: HashMap<ObjectId, SurfaceData>,
    toplevels: HashMap<ObjectId, ToplevelData>,
    xdg_surfaces: HashMap<ObjectId, XdgSurfaceData>,
    subsurfaces: HashMap<ObjectId, ObjectId>,
    pools: HashMap<ObjectId, ShmPoolData>,
    buffers: HashMap<ObjectId, (wl_buffer::WlBuffer, ShmBufferData)>,
    seats: Vec<SeatData>,
    outputs: Vec<(ClientId, wl_output::WlOutput)>,
    regions: HashMap<ObjectId, Vec<(i32, i32, i32, i32)>>,
    selection: SelectionState,
    pending_present: bool,
    mapped_logged: bool,
    active_toplevel: Option<ObjectId>,
    toplevel_order: Vec<ObjectId>,
    client_bytes: HashMap<ClientId, usize>,
    total_bytes: usize,
}

pub const MAX_PER_CLIENT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_TOTAL_BYTES: usize = 256 * 1024 * 1024;
pub const MAX_CALLBACKS_PER_SURFACE: usize = 32;
pub const MAX_MIME_TYPES_PER_SOURCE: usize = 32;
pub const MAX_SURFACES_PER_CLIENT: usize = 128;
pub const MAX_TOPLEVELS_PER_CLIENT: usize = 32;
pub const MAX_BUFFERS_PER_CLIENT: usize = 64;
pub const MAX_POOLS_PER_CLIENT: usize = 16;
pub const MAX_SOURCES_PER_CLIENT: usize = 32;
pub const MAX_DEVICES_PER_CLIENT: usize = 16;
pub const MAX_SEATS_PER_CLIENT: usize = 8;
pub const MAX_OUTPUTS_PER_CLIENT: usize = 8;
pub const MAX_REGIONS_PER_CLIENT: usize = 32;
pub const MAX_REGION_RECTS: usize = 128;

impl State {
    fn new(dh: DisplayHandle, out_size: (u32, u32), keyboard: Keyboard) -> Self {
        Self {
        dh,
        out_size,
        serial: 0,
        keyboard,
        surfaces: HashMap::new(),
        toplevels: HashMap::new(),
        xdg_surfaces: HashMap::new(),
        subsurfaces: HashMap::new(),
        pools: HashMap::new(),
        buffers: HashMap::new(),
        seats: Vec::new(),
        outputs: Vec::new(),
        regions: HashMap::new(),
        selection: SelectionState::default(),
        pending_present: false,
        mapped_logged: false,
        active_toplevel: None,
        toplevel_order: Vec::new(),
        client_bytes: HashMap::new(),
        total_bytes: 0,
        }
    }

    fn next_serial(&mut self) -> u32 {
        self.serial = self.serial.wrapping_add(1).max(1);
        self.serial
    }

    fn surface_client_id(&self, surf_id: &ObjectId) -> Option<ClientId> {
        self.surfaces
            .get(surf_id)
            .and_then(|surface| surface.resource.client())
            .map(|client| client.id())
    }

    fn current_bytes(&self, surf_id: &ObjectId) -> usize {
        self.surfaces
            .get(surf_id)
            .and_then(|surface| surface.pixels.as_ref())
            .map(|pixels| pixels.bytes.len())
            .unwrap_or(0)
    }

    fn cached_bytes(&self, surf_id: &ObjectId) -> usize {
        self.surfaces
            .get(surf_id)
            .and_then(|surface| surface.cached.as_ref())
            .and_then(|cached| cached.pixels.as_ref())
            .and_then(|pixels| pixels.as_ref())
            .map(|pixels| pixels.bytes.len())
            .unwrap_or(0)
    }

    fn charge_bytes(&mut self, client: ClientId, bytes: usize) -> Result<(), String> {
        let current = self.client_bytes.get(&client).copied().unwrap_or(0);
        if current.saturating_add(bytes) > MAX_PER_CLIENT_BYTES {
            return Err("client exceeds per-client memory budget".into());
        }
        if self.total_bytes.saturating_add(bytes) > MAX_TOTAL_BYTES {
            return Err("compositor exceeds global memory budget".into());
        }
        self.client_bytes.insert(client, current + bytes);
        self.total_bytes += bytes;
        Ok(())
    }

    fn release_bytes(&mut self, client: ClientId, bytes: usize) {
        if let Some(current) = self.client_bytes.get_mut(&client) {
            *current = current.saturating_sub(bytes);
            if *current == 0 {
                self.client_bytes.remove(&client);
            }
        }
        self.total_bytes = self.total_bytes.saturating_sub(bytes);
    }

    fn count_client_surfaces(&self, client: &ClientId) -> usize {
        self.surfaces
            .keys()
            .filter(|id| {
                self.dh
                    .get_client((*id).clone())
                    .map(|c| c.id() == *client)
                    .unwrap_or(false)
            })
            .count()
    }

    fn count_client_toplevels(&self, client: &ClientId) -> usize {
        self.toplevels
            .values()
            .filter(|top| {
                self.dh
                    .get_client(top.surface.clone())
                    .map(|c| c.id() == *client)
                    .unwrap_or(false)
            })
            .count()
    }

    fn count_client_buffers(&self, client: &ClientId) -> usize {
        self.buffers
            .keys()
            .filter(|id| {
                self.dh
                    .get_client((*id).clone())
                    .map(|c| c.id() == *client)
                    .unwrap_or(false)
            })
            .count()
    }

    fn count_client_pools(&self, client: &ClientId) -> usize {
        self.pools
            .keys()
            .filter(|id| {
                self.dh
                    .get_client((*id).clone())
                    .map(|c| c.id() == *client)
                    .unwrap_or(false)
            })
            .count()
    }

    fn count_client_sources(&self, client: &Client) -> usize {
        let target = client.id();
        self.selection
            .sources
            .values()
            .filter(|source| {
                source
                    .resource
                    .client()
                    .map(|c| c.id() == target)
                    .unwrap_or(false)
            })
            .count()
    }

    fn count_client_devices(&self, client: &Client) -> usize {
        let target = client.id();
        self.selection
            .devices
            .values()
            .filter(|device| device.client().map(|c| c.id() == target).unwrap_or(false))
            .count()
    }

    fn synchronized(&self, surf_id: &ObjectId) -> bool {
        let mut cursor = surf_id;
        while let Some(surface) = self.surfaces.get(cursor) {
            if surface.parent.is_some() && surface.subsurface_sync { return true; }
            match &surface.parent { Some(parent) => cursor = parent, None => break }
        }
        false
    }

    fn apply_commit(&mut self, surf_id: &ObjectId) {
        let (pending, pending_stacking) = match self.surfaces.get_mut(surf_id) {
            Some(surface) => (std::mem::take(&mut surface.pending), surface.pending_stacking.take()),
            None => return,
        };
        if let Some(top_id) = self.toplevel_for_surface(surf_id) {
            let top = &self.toplevels[&top_id];
            if matches!(pending.buffer, Some(Some(_))) && (top.last_configure == 0 || top.acked_configure != top.last_configure) {
                top.xdg_surface.post_error(3u32, "buffer committed before configure ack");
                return;
            }
            if top.last_configure == 0 && !matches!(pending.buffer, Some(Some(_))) {
                let serial = self.next_serial();
                let top = self.toplevels.get_mut(&top_id).unwrap();
                use wayland_protocols::xdg::shell::server::xdg_toplevel::State;
                top.resource.configure(self.out_size.0 as i32, self.out_size.1 as i32,
                    [State::Fullscreen, State::Activated].into_iter().flat_map(|state| (state as u32).to_ne_bytes()).collect());
                top.xdg_surface.configure(serial);
                top.last_configure = serial;
            }
        }
        let client_id = self.surface_client_id(surf_id);
        let old_cached = self.cached_bytes(surf_id);
        let old_current = self.current_bytes(surf_id);
        let is_sync = self.synchronized(surf_id);
        let current_callbacks = self.surfaces.get(surf_id).map(|s| s.callbacks.len()).unwrap_or(0);
        let cached_callbacks = self.surfaces.get(surf_id).and_then(|s| s.cached.as_ref()).map(|c| c.callbacks.len()).unwrap_or(0);
        if pending.callbacks.len() + current_callbacks + cached_callbacks > MAX_CALLBACKS_PER_SURFACE {
            self.surfaces[surf_id].resource.post_error(0u32, "too many outstanding frame callbacks");
            return;
        }
        let pixels = match pending.buffer {
            Some(Some(id)) => {
                let needed = match self.buffers.get(&id) {
                    Some((_, data)) => (data.width as usize).checked_mul(data.height as usize).and_then(|n| n.checked_mul(4)),
                    None => None,
                };
                let Some((buffer, data)) = self.buffers.get(&id) else {
                    self.surfaces[surf_id].resource.post_error(0u32, "attached buffer was destroyed before commit");
                    return;
                };
                let needed = match needed {
                    Some(n) => n,
                    None => {
                        self.surfaces[surf_id].resource.post_error(0u32, "buffer size overflow");
                        return;
                    }
                };
                if let Some(client) = client_id.clone() {
                    let old = if is_sync { old_cached } else { old_current };
                    let net = needed as isize - old as isize;
                    if net > 0
                        && (self.client_bytes.get(&client).copied().unwrap_or(0).saturating_add(net as usize) > MAX_PER_CLIENT_BYTES
                            || self.total_bytes.saturating_add(net as usize) > MAX_TOTAL_BYTES)
                    {
                        self.surfaces[surf_id].resource.post_error(0u32, "client exceeds memory budget");
                        return;
                    }
                }
                match shm::read_rows(data) {
                    Ok(pixels) => {
                        buffer.release();
                        if let Some(client) = client_id.clone() {
                            let old = if is_sync { old_cached } else { old_current };
                            let net = pixels.bytes.len() as isize - old as isize;
                            if net > 0 {
                                if self.charge_bytes(client, net as usize).is_err() {
                                    self.surfaces[surf_id].resource.post_error(0u32, "client exceeds memory budget");
                                    return;
                                }
                            } else if net < 0 {
                                self.release_bytes(client, (-net) as usize);
                            }
                        }
                        Some(Some(pixels))
                    }
                    Err(error) => { self.surfaces[surf_id].resource.post_error(0u32, error); return; }
                }
            }
            Some(None) => {
                if let Some(client) = client_id.clone() {
                    let old = if is_sync { old_cached } else { old_current };
                    if old > 0 {
                        self.release_bytes(client, old);
                    }
                }
                Some(None)
            }
            None => None,
        };
        let update = SurfaceUpdate { pixels, callbacks: pending.callbacks };
        if self.synchronized(surf_id) {
            self.surfaces.get_mut(surf_id).unwrap().cached.get_or_insert_with(SurfaceUpdate::default).merge(update);
            if let Some(order) = pending_stacking {
                self.surfaces.get_mut(surf_id).unwrap().cached_stacking = Some(order);
            }
            let children: Vec<ObjectId> = self.surfaces.get(surf_id).map(|s| s.stacking.clone()).unwrap_or_default();
            for child_id in children.into_iter().filter(|id| id != surf_id) {
                if let Some(child) = self.surfaces.get_mut(&child_id) {
                    if let Some(pos) = child.pending_pos.take() {
                        child.cached_pos = Some(pos);
                    }
                }
            }
            return;
        }
        {
            let surface = self.surfaces.get_mut(surf_id).unwrap();
            surface.apply(update);
            if let Some(order) = pending_stacking {
                surface.stacking = order;
                surface.cached_stacking = None;
            } else if let Some(order) = surface.cached_stacking.take() {
                surface.stacking = order;
            }
        }
        self.flush_applied(surf_id);
        if let Some(top_id) = self.toplevel_for_surface(surf_id) {
            if self.surfaces[surf_id].pixels.is_none() {
                let top = self.toplevels.get_mut(&top_id).unwrap();
                if top.mapped {
                    top.mapped = false;
                    top.last_configure = 0;
                    top.acked_configure = 0;
                    self.toplevel_order.retain(|order| order != &top_id);
                    self.clear_focus(surf_id);
                }
            }
            self.try_map(surf_id);
        }
        self.pending_present = true;
    }

    fn desynchronize(&mut self, id: &ObjectId) {
        let Some(surface) = self.surfaces.get_mut(id) else { return; };
        surface.subsurface_sync = false;
        if self.synchronized(id) { return; }
        let client = self.surface_client_id(id);
        let old_current = self.current_bytes(id);
        let had_pixel_change = self
            .surfaces
            .get(id)
            .and_then(|s| s.cached.as_ref())
            .is_some_and(|cached| cached.pixels.is_some());
        let cached = self.surfaces.get_mut(id).unwrap().cached.take();
        let cached_stacking = self.surfaces.get_mut(id).unwrap().cached_stacking.take();
        let had_cached = cached.is_some() || cached_stacking.is_some();
        if let Some(update) = cached {
            self.surfaces.get_mut(id).unwrap().apply(update);
        }
        if let Some(order) = cached_stacking {
            if let Some(surface) = self.surfaces.get_mut(id) {
                surface.stacking = order;
            }
        }
        if had_cached {
            if had_pixel_change {
                if let Some(client) = client {
                    if old_current > 0 {
                        self.release_bytes(client, old_current);
                    }
                }
            }
            self.flush_cached_descendants(id);
            self.pending_present = true;
        }
    }

    fn flush_applied(&mut self, parent: &ObjectId) {
        let children: Vec<ObjectId> = self.surfaces.get(parent).map(|s| s.stacking.clone()).unwrap_or_default();
        for child_id in children.into_iter().filter(|id| id != parent) {
            let client = self.surface_client_id(&child_id);
            let old_current = self.current_bytes(&child_id);
            let had_pixel_change = self
                .surfaces
                .get(&child_id)
                .and_then(|s| s.cached.as_ref())
                .is_some_and(|cached| cached.pixels.is_some());
            if let Some(child) = self.surfaces.get_mut(&child_id) {
                if let Some(pos) = child.cached_pos.take() { child.subsurface_pos = pos; }
                if let Some(pos) = child.pending_pos.take() { child.subsurface_pos = pos; }
                if let Some(update) = child.cached.take() { child.apply(update); }
                if let Some(order) = child.cached_stacking.take() { child.stacking = order; }
            }
            if had_pixel_change {
                if let Some(client) = client {
                    if old_current > 0 {
                        self.release_bytes(client, old_current);
                    }
                }
            }
            self.flush_cached_descendants(&child_id);
        }
    }

    fn flush_cached_descendants(&mut self, parent: &ObjectId) {
        let children: Vec<ObjectId> = self.surfaces.get(parent).map(|s| s.stacking.clone()).unwrap_or_default();
        for child_id in children.into_iter().filter(|id| id != parent) {
            let client = self.surface_client_id(&child_id);
            let old_current = self.current_bytes(&child_id);
            let had_pixel_change = self
                .surfaces
                .get(&child_id)
                .and_then(|s| s.cached.as_ref())
                .is_some_and(|cached| cached.pixels.is_some());
            if let Some(child) = self.surfaces.get_mut(&child_id) {
                if let Some(update) = child.cached.take() { child.apply(update); }
                if let Some(pos) = child.cached_pos.take() { child.subsurface_pos = pos; }
                if let Some(order) = child.cached_stacking.take() { child.stacking = order; }
            }
            if had_pixel_change {
                if let Some(client) = client {
                    if old_current > 0 {
                        self.release_bytes(client, old_current);
                    }
                }
            }
            self.flush_cached_descendants(&child_id);
        }
    }

    fn toplevel_for_surface(&self, surf_id: &ObjectId) -> Option<ObjectId> {
        self.toplevels
            .iter()
            .find(|(_, t)| &t.surface == surf_id)
            .map(|(id, _)| id.clone())
    }

    fn try_map(&mut self, surf_id: &ObjectId) {
        let top_id = match self.toplevel_for_surface(surf_id) {
            Some(id) => id,
            None => return,
        };
        let (has_buffer, acked_ok) = match (self.surfaces.get(surf_id), self.toplevels.get(&top_id))
        {
            (Some(s), Some(t)) => (
                s.pixels.is_some(),
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
        let client_id = self.client_of_surface(surf_id);
        if let Some(s) = self.surfaces.get(surf_id) {
            for (cid, out) in &self.outputs {
                if Some(cid) == client_id.as_ref() {
                    s.resource.enter(out);
                }
            }
        }
        self.set_active(top_id);
        self.pending_present = true;
        if !self.mapped_logged {
            self.mapped_logged = true;
            eprintln!("TINYLAND_SURFACE_MAPPED");
        }
    }

    fn client_of_surface(&self, surf_id: &ObjectId) -> Option<ClientId> {
        self.dh.get_client(surf_id.clone()).map(|c| c.id()).ok()
    }

    fn active_surface(&self) -> Option<ObjectId> {
        self.active_toplevel
            .as_ref()
            .and_then(|top| self.toplevels.get(top))
            .map(|top| top.surface.clone())
    }

    fn set_active(&mut self, top_id: ObjectId) {
        if self.active_toplevel.as_ref() == Some(&top_id) {
            return;
        }
        let old_surface = self.active_surface();
        let serial = self.next_serial();
        if let Some(old) = old_surface {
            for seat in &mut self.seats {
                if seat.keyboard_focus.as_ref() == Some(&old) {
                    if let Some(surface) = self.surfaces.get(&old) {
                        for keyboard in &seat.keyboards {
                            keyboard.leave(serial, &surface.resource);
                        }
                    }
                    seat.keyboard_focus = None;
                }
            }
        }
        self.active_toplevel = Some(top_id.clone());
        self.toplevel_order.retain(|id| id != &top_id);
        self.toplevel_order.push(top_id.clone());
        let Some(new_surface) = self.active_surface() else { return };
        let Some(surface) = self.surfaces.get(&new_surface) else { return };
        let new_client = surface.resource.client();
        let [depressed, latched, locked, group] = self.keyboard.modifiers();
        for seat in &mut self.seats {
            if seat.resource.client() != new_client {
                continue;
            }
            seat.keyboard_focus = Some(new_surface.clone());
            for keyboard in &seat.keyboards {
                keyboard.enter(serial, &surface.resource, Vec::new());
                keyboard.modifiers(serial, depressed, latched, locked, group);
            }
        }
        self.send_selection();
    }

    fn deactivate_surface(&mut self, surf_id: &ObjectId) {
        let serial = self.next_serial();
        for seat in &mut self.seats {
            if seat.keyboard_focus.as_ref() == Some(surf_id) {
                if let Some(surface) = self.surfaces.get(surf_id) {
                    for keyboard in &seat.keyboards {
                        keyboard.leave(serial, &surface.resource);
                    }
                }
                seat.keyboard_focus = None;
            }
        }
        let is_active = self.active_surface().as_ref() == Some(surf_id);
        if !is_active {
            return;
        }
        if let Some(top) = self
            .toplevels
            .iter()
            .find(|(_, t)| &t.surface == surf_id)
            .map(|(id, _)| id.clone())
        {
            self.toplevel_order.retain(|id| id != &top);
            self.active_toplevel = None;
        } else {
            self.active_toplevel = None;
        }
        let next = self
            .toplevel_order
            .iter()
            .rev()
            .find(|id| {
                self.toplevels
                    .get(*id)
                    .is_some_and(|top| top.mapped)
            })
            .cloned()
            .or_else(|| {
                let mut mapped: Vec<ObjectId> = self
                    .toplevels
                    .iter()
                    .filter(|(_, t)| t.mapped)
                    .map(|(id, _)| id.clone())
                    .collect();
                mapped.sort_by_key(|id| id.protocol_id());
                mapped.into_iter().next()
            });
        if let Some(next) = next {
            self.set_active(next);
        }
        self.send_selection();
    }

    fn clear_focus(&mut self, id: &ObjectId) {
        self.deactivate_surface(id);
    }

    fn key(&mut self, code: u16, down: bool) {
        let serial = self.next_serial();
        let [depressed, latched, locked, group] = self.keyboard.update(code, down);
        let Some(focused) = self.active_surface() else { return };
        for seat in &self.seats {
            if seat.keyboard_focus.as_ref() != Some(&focused) {
                continue;
            }
            for keyboard in &seat.keyboards {
                keyboard.key(serial, now_ms(), code as u32, if down {
                    wayland_server::protocol::wl_keyboard::KeyState::Pressed
                } else { wayland_server::protocol::wl_keyboard::KeyState::Released });
                keyboard.modifiers(serial, depressed, latched, locked, group);
            }
        }
    }

    fn compose(&self) -> Vec<u8> {
        let (w, h) = self.out_size;
        let mut fb = vec![0; w as usize * h as usize * 4];
        if let Some(surface) = self.active_surface() {
            self.blit_surface(&surface, 0, 0, &mut fb);
        }
        fb
    }

    fn presented(&mut self, time: u32) {
        let Some(surface) = self.active_surface() else { return };
        let mut stack = vec![surface];
        while let Some(id) = stack.pop() {
            if let Some(surface) = self.surfaces.get_mut(&id) {
                if surface.pixels.is_none() { continue; }
                for callback in surface.callbacks.drain(..) { callback.done(time); }
                stack.extend(surface.stacking.iter().filter(|child| **child != id).cloned());
            }
        }
    }

    fn blit_surface(&self, surf_id: &ObjectId, ox: i32, oy: i32, fb: &mut [u8]) {
        let Some(surface) = self.surfaces.get(surf_id) else { return; };
        let x = ox.saturating_add(surface.subsurface_pos.0);
        let y = oy.saturating_add(surface.subsurface_pos.1);
        let Some(pixels) = &surface.pixels else { return; };
        for id in &surface.stacking {
            if id == surf_id {
                blit_rows(fb, self.out_size.0, self.out_size.1, &pixels.bytes,
                    pixels.width, pixels.height, x, y, pixels.format);
            } else { self.blit_surface(id, x, y, fb); }
        }
    }
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
        let dy = oy as i64 + sy as i64;
        if dy < 0 || dy >= out_h as i64 {
            continue;
        }
        for sx in 0..src_w as i32 {
            let dx = ox as i64 + sx as i64;
            if dx < 0 || dx >= out_w as i64 {
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
                    for channel in 0..3 {
                        fb[d + channel] = (rows[s + channel] as u32
                            + fb[d + channel] as u32 * (255 - a) / 255).min(255) as u8;
                    }
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

pub fn run(
    mode: &str,
    device: &std::path::Path,
    width: u32,
    height: u32,
    socket_name: &std::ffi::OsString,
    command: &[std::ffi::OsString],
    shutdown: &crate::shutdown::Shutdown,
) -> i32 {
    let runtime_dir = match validate_runtime_dir() {
        Ok(dir) => dir,
        Err(error) => {
            eprintln!("runtime dir: {error}");
            return 1;
        }
    };

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

    let socket_name_str = socket_name.to_string_lossy().into_owned();
    let socket = match ListeningSocket::bind(socket_name_str.as_str()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("socket bind failed: {e:?}");
            return 1;
        }
    };
    let sock_name = socket
        .socket_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or(socket_name_str.clone());
    eprintln!("TINYLAND_SOCKET {sock_name:?}");

    let keyboard = match Keyboard::new() {
        Ok(keyboard) => keyboard,
        Err(error) => { eprintln!("keyboard init: {error}"); return 1; }
    };
    let mut devices = match input::discover_keyboards() {
        Ok(devices) => devices,
        Err(error) => { eprintln!("keyboard discovery: {error}"); return 1; }
    };
    let mut state = State::new(dh.clone(), (w, h), keyboard);

    let mut app = match spawn_app(mode, runtime_dir.as_str(), sock_name.as_str(), command) {
        Ok(app) => app,
        Err(error) => { eprintln!("foot spawn failed: {error}"); return 1; }
    };
    eprintln!("TINYLAND_COMPOSITOR_READY socket={sock_name} size={w}x{h} path={}", output.mode_path());

    let display_fd = display.as_fd().try_clone_to_owned().expect("clone display fd");
    let socket_fd = (&socket).as_fd().try_clone_to_owned().expect("clone socket fd");
    let base_timeout = rustix::time::Timespec { tv_sec: 0, tv_nsec: 500_000_000 };
    loop {
        if shutdown.signal().is_some() { return 0; }
        let poll_timeout = match output.flip_deadline() {
            Some(deadline) => {
                let now = std::time::Instant::now();
                if now >= deadline {
                    base_timeout
                } else {
                    let remaining = deadline - now;
                    if remaining.as_nanos() < 500_000_000 {
                        rustix::time::Timespec::try_from(remaining).unwrap_or(base_timeout)
                    } else {
                        base_timeout
                    }
                }
            }
            None => base_timeout,
        };
        let mut fds = vec![
            rustix::event::PollFd::new(&display_fd, rustix::event::PollFlags::IN),
            rustix::event::PollFd::new(&socket_fd, rustix::event::PollFlags::IN),
            rustix::event::PollFd::new(&output, rustix::event::PollFlags::IN),
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
        if output.has_pending_flip() {
            match output.poll_flip() {
                Ok(Some(time)) => {
                    state.pending_present = false;
                    state.presented(time);
                }
                Ok(None) => {}
                Err(error) => {
                    if shutdown.signal().is_some() { return 0; }
                    eprintln!("present failed: {error}");
                    return 1;
                }
            }
        }
        for device in &mut devices {
            match input::read_keys(device) {
                Ok(keys) => for (code, down) in keys { state.key(code, down); },
                Err(error) => { eprintln!("{error}"); return 1; }
            }
        }
        if let Err(error) = app.reap() { eprintln!("{error}"); return 1; }
        if app.exited() {
            eprintln!("TINYLAND_SESSION_END primary client exited");
            return 0;
        }
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
        if state.pending_present && !output.has_pending_flip() {
            let pixels = state.compose();
            if let Err(error) = output.queue_present(&pixels, w, h) {
                if shutdown.signal().is_some() { return 0; }
                eprintln!("present failed: {error}");
                return 1;
            }
        }
        if let Err(e) = display.flush_clients() {
            eprintln!("flush failed: {e}");
            return 1;
        }
    }
}

fn validate_runtime_dir() -> Result<String, String> {
    let dir = std::env::var("XDG_RUNTIME_DIR").map_err(|_| "XDG_RUNTIME_DIR is not set".to_string())?;
    let path = std::path::Path::new(&dir);
    if !path.is_absolute() {
        return Err("XDG_RUNTIME_DIR is not absolute".into());
    }
    let metadata = std::fs::metadata(path).map_err(|e| format!("stat XDG_RUNTIME_DIR: {e}"))?;
    if !metadata.is_dir() {
        return Err("XDG_RUNTIME_DIR is not a directory".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let euid = rustix::process::getuid().as_raw();
        if metadata.uid() != euid {
            return Err(format!("XDG_RUNTIME_DIR is not owned by uid {euid}"));
        }
        let mode = metadata.mode() & 0o777;
        if mode != 0o700 {
            eprintln!("TINYLAND_RUNTIME_DIR_PERMS mode={mode:o} expected=700");
        }
    }
    Ok(dir)
}

struct App(Option<std::process::Child>);

impl App {
    fn exited(&self) -> bool {
        self.0.is_none()
    }

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

fn spawn_app(mode: &str, runtime_dir: &str, socket_name: &str, command: &[std::ffi::OsString]) -> std::io::Result<App> {
    if !command.is_empty() {
        eprintln!("TINYLAND_SPAWN_COMMAND command={command:?}");
        let child = std::process::Command::new(&command[0])
            .args(&command[1..])
            .env("XDG_RUNTIME_DIR", runtime_dir)
            .env("WAYLAND_DISPLAY", socket_name)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::inherit())
            .spawn()?;
        return Ok(App(Some(child)));
    }
    let test_arg = if mode == "wayland-pixels" { "wayland-pixels" } else { "smoke" };
    eprintln!("TINYLAND_SPAWN_FOOT mode={test_arg} (legacy fallback)");
    let child = std::process::Command::new("foot")
        .arg("/usr/local/bin/guest-test")
        .arg(test_arg)
        .env("XDG_RUNTIME_DIR", runtime_dir)
        .env("WAYLAND_DISPLAY", socket_name)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::inherit())
        .spawn()?;
    Ok(App(Some(child)))
}

mod dispatch_impl;

#[cfg(test)]
mod tests;
