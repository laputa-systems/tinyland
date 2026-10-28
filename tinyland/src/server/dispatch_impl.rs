use super::State;
use crate::input::SeatData;
use crate::shell::{ToplevelData, XdgSurfaceData};
use crate::shm::{self, ShmBufferData, ShmFormat};
use crate::surface::SurfaceData;
use std::os::unix::io::OwnedFd;
use wayland_protocols::xdg::shell::server::{xdg_popup, xdg_positioner, xdg_surface, xdg_toplevel, xdg_wm_base};
use wayland_server::protocol::{
    wl_buffer, wl_callback, wl_compositor, wl_data_device_manager, wl_data_device, wl_data_source, wl_data_offer, wl_keyboard, wl_output, wl_region, wl_seat,
    wl_shm, wl_shm_pool, wl_subcompositor, wl_subsurface, wl_surface,
};
use wayland_server::{Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource};

impl GlobalDispatch<wl_compositor::WlCompositor, ()> for State {
    fn bind(
        _state: &mut Self,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<wl_compositor::WlCompositor>,
        _global_data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<wl_compositor::WlCompositor, ()> for State {
    fn request(
        state: &mut Self,
        _client: &Client,
        _resource: &wl_compositor::WlCompositor,
        request: wl_compositor::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            wl_compositor::Request::CreateSurface { id } => {
                let surf = data_init.init(id, ());
                state.surfaces.insert(surf.id(), SurfaceData::new(surf));
            }
            wl_compositor::Request::CreateRegion { id } => {
                let region = data_init.init(id, ());
                state.regions.insert(region.id(), Vec::new());
            }
            _ => {}
        }
    }

    fn destroyed(
        _state: &mut Self,
        _client: wayland_server::backend::ClientId,
        _resource: &wl_compositor::WlCompositor,
        _data: &(),
    ) {
    }
}

impl Dispatch<wl_region::WlRegion, ()> for State {
    fn request(
        state: &mut Self,
        _client: &Client,
        resource: &wl_region::WlRegion,
        request: wl_region::Request,
        _data: &(),
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        let entry = state.regions.entry(resource.id()).or_default();
        match request {
            wl_region::Request::Add { x, y, width, height } => {
                if width > 0 && height > 0 {
                    entry.push((x, y, width, height));
                }
            }
            wl_region::Request::Subtract { .. } => {}
            wl_region::Request::Destroy => {}
            _ => {}
        }
    }

    fn destroyed(
        state: &mut Self,
        _client: wayland_server::backend::ClientId,
        resource: &wl_region::WlRegion,
        _data: &(),
    ) {
        state.regions.remove(&resource.id());
    }
}

impl Dispatch<wl_surface::WlSurface, ()> for State {
    fn request(
        state: &mut Self,
        _client: &Client,
        resource: &wl_surface::WlSurface,
        request: wl_surface::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        let id = resource.id();
        match request {
            wl_surface::Request::Attach { buffer, x, y } => {
                if let Some(s) = state.surfaces.get_mut(&id) {
                    s.pending.buffer = Some(buffer.map(|b| b.id()));
                    let _ = (x, y);
                }
            }
            wl_surface::Request::Damage { .. } | wl_surface::Request::DamageBuffer { .. } => {}
            wl_surface::Request::Frame { callback: cb_id } => {
                let cb = data_init.init(cb_id, ());
                if let Some(s) = state.surfaces.get_mut(&id) {
                    s.pending.callbacks.push(cb);
                }
            }
            wl_surface::Request::SetOpaqueRegion { .. } => {}
            wl_surface::Request::SetInputRegion { .. } => {}
            wl_surface::Request::Commit => {
                state.apply_commit(&id);
            }
            wl_surface::Request::SetBufferTransform { transform } => {
                if transform
                    != wayland_server::WEnum::Value(wl_output::Transform::Normal)
                {
                    resource.post_error(
                        wl_surface::Error::InvalidTransform as u32,
                        "only normal transform supported",
                    );
                }
            }
            wl_surface::Request::SetBufferScale { scale } => {
                if scale != 1 {
                    resource.post_error(
                        wl_surface::Error::InvalidScale as u32,
                        "only scale 1 supported",
                    );
                    return;
                }
            }
            wl_surface::Request::Offset { x, y } => {
                let _ = (x, y);
            }
            wl_surface::Request::Destroy => {}
            _ => {}
        }
    }

    fn destroyed(
        state: &mut Self,
        _client: wayland_server::backend::ClientId,
        resource: &wl_surface::WlSurface,
        _data: &(),
    ) {
        let id = resource.id();
        if let Some(s) = state.surfaces.remove(&id) {
            if let Some(parent) = s.parent {
                if let Some(parent) = state.surfaces.get_mut(&parent) { parent.stacking.retain(|child| child != &id);
                    if let Some(order) = &mut parent.pending_stacking { order.retain(|child| child != &id); } }
            }
            for seat in &mut state.seats {
                if seat.keyboard_focus.as_ref() == Some(&id) { seat.keyboard_focus = None; }
            }
            for child in s.stacking {
                if let Some(c) = state.surfaces.get_mut(&child) {
                    c.parent = None;
                }
            }
        }
        state.toplevels.retain(|_, t| t.surface != id);
        state.pending_present = true;
    }
}

impl GlobalDispatch<wl_subcompositor::WlSubcompositor, ()> for State {
    fn bind(
        _state: &mut Self,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<wl_subcompositor::WlSubcompositor>,
        _global_data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<wl_subcompositor::WlSubcompositor, ()> for State {
    fn request(
        state: &mut Self,
        _client: &Client,
        _resource: &wl_subcompositor::WlSubcompositor,
        request: wl_subcompositor::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            wl_subcompositor::Request::GetSubsurface { id, surface, parent } => {
                let surf_id = surface.id();
                let parent_id = parent.id();
                if surf_id == parent_id {
                    _resource.post_error(
                        wl_subcompositor::Error::BadSurface as u32,
                        "subsurface cannot be its own parent",
                    );
                    return;
                }
                let sub = data_init.init(id, ());
                let sub_id = sub.id();
                let mut ancestor = Some(parent_id.clone());
                let mut depth = 0;
                while let Some(id) = ancestor {
                    depth += 1;
                    if depth >= 64 {
                        _resource.post_error(wl_subcompositor::Error::BadSurface as u32, "subsurface nesting exceeds 64 levels");
                        return;
                    }
                    if id == surf_id {
                        _resource.post_error(wl_subcompositor::Error::BadSurface as u32, "subsurface parent cycle");
                        return;
                    }
                    ancestor = state.surfaces.get(&id).and_then(|surface| surface.parent.clone());
                }
                let mut subtree = vec![(surf_id.clone(), depth + 1)];
                while let Some((id, depth)) = subtree.pop() {
                    if depth > 64 {
                        _resource.post_error(wl_subcompositor::Error::BadSurface as u32, "subsurface nesting exceeds 64 levels");
                        return;
                    }
                    if let Some(surface) = state.surfaces.get(&id) {
                        subtree.extend(surface.stacking.iter().filter(|child| **child != id).map(|child| (child.clone(), depth + 1)));
                    }
                }
                let valid = match (state.surfaces.get(&surf_id), state.surfaces.get(&parent_id)) {
                    (Some(s), Some(p)) => !s.has_role && !p.is_popup,
                    _ => false,
                };
                if !valid {
                    _resource.post_error(
                        wl_subcompositor::Error::BadSurface as u32,
                        "bad parent or surface already has role",
                    );
                    return;
                }
                if let Some(s) = state.surfaces.get_mut(&surf_id) {
                    s.has_role = true;
                    s.parent = Some(parent_id.clone());
                    s.subsurface_sync = true;
                }
                if let Some(p) = state.surfaces.get_mut(&parent_id) {
                    p.stacking.push(surf_id.clone());
                    if let Some(order) = &mut p.pending_stacking { order.push(surf_id.clone()); }
                }
                state.subsurfaces.insert(sub_id, surf_id);
            }
            wl_subcompositor::Request::Destroy => {}
            _ => {}
        }
    }

    fn destroyed(
        _state: &mut Self,
        _client: wayland_server::backend::ClientId,
        _resource: &wl_subcompositor::WlSubcompositor,
        _data: &(),
    ) {
    }
}

fn restack(state: &mut State, resource: &wl_subsurface::WlSubsurface,
    id: &wayland_server::backend::ObjectId, sibling: &wayland_server::backend::ObjectId, above: bool) {
    let parent = state.surfaces.get(id).and_then(|surface| surface.parent.clone());
    let valid = parent.as_ref().and_then(|parent| state.surfaces.get(parent))
        .is_some_and(|parent| id != sibling && parent.stacking.contains(sibling));
    if !valid { resource.post_error(wl_subsurface::Error::BadSurface as u32, "restack target is not a sibling or parent"); return; }
    let parent = state.surfaces.get_mut(&parent.unwrap()).unwrap();
    let order = parent.pending_stacking.get_or_insert_with(|| parent.stacking.clone());
    order.retain(|child| child != id);
    let position = order.iter().position(|child| child == sibling).unwrap() + usize::from(above);
    order.insert(position, id.clone());
}

impl Dispatch<wl_subsurface::WlSubsurface, ()> for State {
    fn request(
        state: &mut Self,
        _client: &Client,
        resource: &wl_subsurface::WlSubsurface,
        request: wl_subsurface::Request,
        _data: &(),
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        let surf_id = match state.subsurfaces.get(&resource.id()).cloned() {
            Some(id) => id,
            None => return,
        };
        match request {
            wl_subsurface::Request::SetPosition { x, y } => {
                if let Some(s) = state.surfaces.get_mut(&surf_id) {
                    s.pending_pos = Some((x, y));
                }
            }
            wl_subsurface::Request::SetSync => {
                if let Some(s) = state.surfaces.get_mut(&surf_id) {
                    s.subsurface_sync = true;
                }
            }
            wl_subsurface::Request::SetDesync => state.desynchronize(&surf_id),
            wl_subsurface::Request::PlaceAbove { sibling } => restack(state, resource, &surf_id, &sibling.id(), true),
            wl_subsurface::Request::PlaceBelow { sibling } => restack(state, resource, &surf_id, &sibling.id(), false),
            wl_subsurface::Request::Destroy => {}
            _ => {}
        }
    }

    fn destroyed(
        state: &mut Self,
        _client: wayland_server::backend::ClientId,
        resource: &wl_subsurface::WlSubsurface,
        _data: &(),
    ) {
        if let Some(surf_id) = state.subsurfaces.remove(&resource.id()) {
            let parent = state.surfaces.get_mut(&surf_id).and_then(|surface| {
                surface.cached = None;
                surface.parent.take()
            });
            if let Some(parent) = parent {
                if let Some(parent) = state.surfaces.get_mut(&parent) { parent.stacking.retain(|child| child != &surf_id);
                    if let Some(order) = &mut parent.pending_stacking { order.retain(|child| child != &surf_id); } }
            }
            state.pending_present = true;
        }
    }
}

impl GlobalDispatch<wl_shm::WlShm, ()> for State {
    fn bind(
        _state: &mut Self,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<wl_shm::WlShm>,
        _global_data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        let shm = data_init.init(resource, ());
        shm.format(wl_shm::Format::Argb8888);
        shm.format(wl_shm::Format::Xrgb8888);
    }
}

impl Dispatch<wl_shm::WlShm, ()> for State {
    fn request(
        state: &mut Self,
        _client: &Client,
        _resource: &wl_shm::WlShm,
        request: wl_shm::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            wl_shm::Request::CreatePool { id, fd, size } => {
                let pool_size = match shm::validate_pool_size(size) {
                    Ok(n) => n,
                    Err(msg) => {
                        _resource.post_error(wl_shm::Error::InvalidFd as u32, msg.to_string());
                        return;
                    }
                };
                let file = std::fs::File::from(fd as OwnedFd);
                let pool_data = match shm::ShmPoolData::new(file, pool_size as i32) {
                    Ok(pool) => pool,
                    Err(error) => { _resource.post_error(wl_shm::Error::InvalidFd as u32, error); return; }
                };
                let pool = data_init.init(id, ());
                state.pools.insert(pool.id(), pool_data);
            }
            _ => {}
        }
    }

    fn destroyed(
        _state: &mut Self,
        _client: wayland_server::backend::ClientId,
        _resource: &wl_shm::WlShm,
        _data: &(),
    ) {
    }
}

impl Dispatch<wl_shm_pool::WlShmPool, ()> for State {
    fn request(
        state: &mut Self,
        _client: &Client,
        resource: &wl_shm_pool::WlShmPool,
        request: wl_shm_pool::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            wl_shm_pool::Request::CreateBuffer { id, offset, width, height, stride, format } => {
                let pool_id = resource.id();
                let pool_size = match state.pools.get(&pool_id) {
                    Some(p) => p.size,
                    None => {
                        resource.post_error(0u32, "unknown pool");
                        return;
                    }
                };
                let fmt = match format {
                    wayland_server::WEnum::Value(wl_shm::Format::Argb8888) => ShmFormat::Argb8888,
                    wayland_server::WEnum::Value(wl_shm::Format::Xrgb8888) => ShmFormat::Xrgb8888,
                    _ => {
                        resource.post_error(
                            wl_shm_pool::Error::InvalidFormat as u32,
                            "only ARGB8888/XRGB8888 supported".to_string(),
                        );
                        return;
                    }
                };
                if let Err(msg) = shm::validate_buffer(pool_size, offset, width, height, stride) {
                    let code = if msg.contains("format") {
                        wl_shm_pool::Error::InvalidFormat as u32
                    } else {
                        wl_shm_pool::Error::InvalidStride as u32
                    };
                    resource.post_error(code, msg.to_string());
                    return;
                }
                let buf = data_init.init(id, ());
                let buf_id = buf.id();
                state.buffers.insert(
                    buf_id,
                    (
                        buf,
                        ShmBufferData { file: state.pools[&pool_id].file.clone(), offset, width, height, stride, format: fmt },
                    ),
                );
            }
            wl_shm_pool::Request::Resize { size } => {
                if let Some(pool) = state.pools.get_mut(&resource.id()) {
                    if let Err(error) = pool.resize(size) {
                        resource.post_error(wl_shm::Error::InvalidFd as u32, error);
                    }
                }
            }
            wl_shm_pool::Request::Destroy => {}
            _ => {}
        }
    }

    fn destroyed(
        state: &mut Self,
        _client: wayland_server::backend::ClientId,
        resource: &wl_shm_pool::WlShmPool,
        _data: &(),
    ) {
        state.pools.remove(&resource.id());
    }
}

impl Dispatch<wl_buffer::WlBuffer, ()> for State {
    fn request(
        _state: &mut Self,
        _client: &Client,
        _resource: &wl_buffer::WlBuffer,
        request: wl_buffer::Request,
        _data: &(),
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            wl_buffer::Request::Destroy => {}
            _ => {}
        }
    }

    fn destroyed(
        state: &mut Self,
        _client: wayland_server::backend::ClientId,
        resource: &wl_buffer::WlBuffer,
        _data: &(),
    ) {
        state.buffers.remove(&resource.id());

    }
}

impl GlobalDispatch<wl_output::WlOutput, ()> for State {
    fn bind(
        state: &mut Self,
        _dh: &DisplayHandle,
        client: &Client,
        resource: New<wl_output::WlOutput>,
        _global_data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        let out = data_init.init(resource, ());
        out.geometry(
            0,
            0,
            0,
            0,
            wl_output::Subpixel::Unknown,
            "tinyland".to_string(),
            "virtio-gpu".to_string(),
            wl_output::Transform::Normal,
        );
        let (w, h) = state.out_size;
        out.mode(wl_output::Mode::Current | wl_output::Mode::Preferred, w as i32, h as i32, 60000);
        if out.version() >= 2 { out.scale(1); out.done(); }
        state.outputs.push((client.id(), out));
    }
}

impl Dispatch<wl_output::WlOutput, ()> for State {
    fn request(
        _state: &mut Self,
        _client: &Client,
        _resource: &wl_output::WlOutput,
        request: wl_output::Request,
        _data: &(),
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            wl_output::Request::Release => {}
            _ => {}
        }
    }

    fn destroyed(
        state: &mut Self,
        _client: wayland_server::backend::ClientId,
        resource: &wl_output::WlOutput,
        _data: &(),
    ) {
        let id = resource.id();
        state.outputs.retain(|(_, o)| o.id() != id);
    }
}

impl GlobalDispatch<wl_seat::WlSeat, ()> for State {
    fn bind(
        state: &mut Self,
        _dh: &DisplayHandle,
        client: &Client,
        resource: New<wl_seat::WlSeat>,
        _global_data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        let seat = data_init.init(resource, ());
        seat.capabilities(wl_seat::Capability::Keyboard);
        if seat.version() >= 2 { seat.name("seat0".to_string()); }
        state.seats.push(SeatData::new(seat));
        let _ = client;
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn request(
        state: &mut Self,
        client: &Client,
        resource: &wl_seat::WlSeat,
        request: wl_seat::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            wl_seat::Request::GetPointer { .. } | wl_seat::Request::GetTouch { .. } => {
                resource.post_error(wl_seat::Error::MissingCapability as u32, "seat has keyboard capability only");
            }
            wl_seat::Request::GetKeyboard { id } => {
                let kb = data_init.init(id, ());
                use std::os::fd::AsFd;
                kb.keymap(wl_keyboard::KeymapFormat::XkbV1, state.keyboard.keymap.as_fd(), state.keyboard.keymap_size);
                if kb.version() >= 4 { kb.repeat_info(25, 600); }
                if let Some(seat) = state.seats.iter_mut().find(|seat| seat.resource == *resource) {
                    seat.keyboards.push(kb);
                }
                if let Some(focus) = state.toplevels.values().find(|top| top.mapped).map(|top| top.surface.clone()) {
                    let serial = state.next_serial();
                    state.send_keyboard_enter(&focus, serial);
                }
            }
            wl_seat::Request::Release => {}
            _ => {}
        }
        let _ = client;
    }

    fn destroyed(
        _state: &mut Self,
        _client: wayland_server::backend::ClientId,
        _resource: &wl_seat::WlSeat,
        _data: &(),
    ) {
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for State {
    fn request(
        _state: &mut Self,
        _client: &Client,
        _resource: &wl_keyboard::WlKeyboard,
        request: wl_keyboard::Request,
        _data: &(),
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            wl_keyboard::Request::Release => {}
            _ => {}
        }
    }

    fn destroyed(
        state: &mut Self,
        _client: wayland_server::backend::ClientId,
        resource: &wl_keyboard::WlKeyboard,
        _data: &(),
    ) {
        let id = resource.id();
        for seat in &mut state.seats {
            seat.keyboards.retain(|k| k.id() != id);
        }
    }
}

impl State {
    pub(super) fn send_selection(&mut self) {
        let devices: Vec<_> = self.selection.devices.values().cloned().collect();
        for device in devices {
            let Some(client) = device.client() else { continue; };
            let focused = self.seats.iter().any(|seat| seat.resource.client().as_ref() == Some(&client) && seat.keyboard_focus.is_some());
            if !focused { continue; }
            let source = self.selection.selection.as_ref().and_then(|id| self.selection.sources.get(id));
            if let Some(source) = source {
                let offer = match client.create_resource::<wl_data_offer::WlDataOffer, (), State>(&self.dh, device.version(), ()) {
                    Ok(offer) => offer,
                    Err(error) => { eprintln!("clipboard offer creation: {error}"); continue; }
                };
                device.data_offer(&offer);
                for mime in &source.mime_types { offer.offer(mime.clone()); }
                device.selection(Some(&offer));
                self.selection.offers.insert(offer.id(), source.resource.clone());
            } else {
                device.selection(None);
            }
        }
    }
}

impl GlobalDispatch<wl_data_device_manager::WlDataDeviceManager, ()> for State {
    fn bind(_state: &mut Self, _dh: &DisplayHandle, _client: &Client,
        resource: New<wl_data_device_manager::WlDataDeviceManager>, _data: &(), data_init: &mut DataInit<'_, Self>) {
        data_init.init(resource, ());
    }
}

impl Dispatch<wl_data_device_manager::WlDataDeviceManager, ()> for State {
    fn request(state: &mut Self, _client: &Client, _resource: &wl_data_device_manager::WlDataDeviceManager,
        request: wl_data_device_manager::Request, _data: &(), _dh: &DisplayHandle, data_init: &mut DataInit<'_, Self>) {
        match request {
            wl_data_device_manager::Request::CreateDataSource { id } => {
                let source = data_init.init(id, ());
                state.selection.sources.insert(source.id(), crate::selection::DataSourceData { resource: source, mime_types: Vec::new() });
            }
            wl_data_device_manager::Request::GetDataDevice { id, .. } => {
                let device = data_init.init(id, ());
                state.selection.devices.insert(device.id(), device);
                state.send_selection();
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_data_source::WlDataSource, ()> for State {
    fn request(state: &mut Self, _client: &Client, resource: &wl_data_source::WlDataSource,
        request: wl_data_source::Request, _data: &(), _dh: &DisplayHandle, _data_init: &mut DataInit<'_, Self>) {
        if let wl_data_source::Request::Offer { mime_type } = request {
            if let Some(source) = state.selection.sources.get_mut(&resource.id()) { source.mime_types.push(mime_type); }
        }
    }

    fn destroyed(state: &mut Self, _client: wayland_server::backend::ClientId,
        resource: &wl_data_source::WlDataSource, _data: &()) {
        state.selection.sources.remove(&resource.id());
        if state.selection.selection.as_ref() == Some(&resource.id()) {
            state.selection.selection = None;
            state.send_selection();
        }
    }
}

impl Dispatch<wl_data_device::WlDataDevice, ()> for State {
    fn request(state: &mut Self, client: &Client, resource: &wl_data_device::WlDataDevice,
        request: wl_data_device::Request, _data: &(), _dh: &DisplayHandle, _data_init: &mut DataInit<'_, Self>) {
        match request {
            wl_data_device::Request::SetSelection { source, serial } => {
                let focused = state.seats.iter().any(|seat| seat.resource.client().as_ref() == Some(client) && seat.keyboard_focus.is_some());
                if !focused || serial > state.serial { return; }
                let next = source.map(|source| source.id());
                if next == state.selection.selection { return; }
                if let Some(old) = state.selection.selection.take().and_then(|id| state.selection.sources.get(&id)) { old.resource.cancelled(); }
                state.selection.selection = next;
                state.send_selection();
            }
            wl_data_device::Request::StartDrag { source, .. } => {
                if let Some(source) = source { source.cancelled(); }
                resource.leave();
            }
            _ => {}
        }
    }

    fn destroyed(state: &mut Self, _client: wayland_server::backend::ClientId,
        resource: &wl_data_device::WlDataDevice, _data: &()) { state.selection.devices.remove(&resource.id()); }
}

impl Dispatch<wl_data_offer::WlDataOffer, ()> for State {
    fn request(state: &mut Self, _client: &Client, resource: &wl_data_offer::WlDataOffer,
        request: wl_data_offer::Request, _data: &(), _dh: &DisplayHandle, _data_init: &mut DataInit<'_, Self>) {
        let Some(source) = state.selection.offers.get(&resource.id()).filter(|source| source.is_alive()) else { return; };
        match request {
            wl_data_offer::Request::Accept { mime_type, .. } => source.target(mime_type),
            wl_data_offer::Request::Receive { mime_type, fd } => {
                use std::os::fd::AsFd;
                source.send(mime_type, fd.as_fd());
            }
            _ => {}
        }
    }

    fn destroyed(state: &mut Self, _client: wayland_server::backend::ClientId,
        resource: &wl_data_offer::WlDataOffer, _data: &()) { state.selection.offers.remove(&resource.id()); }
}

impl GlobalDispatch<xdg_wm_base::XdgWmBase, ()> for State {
    fn bind(
        _state: &mut Self,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<xdg_wm_base::XdgWmBase>,
        _global_data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for State {
    fn request(
        state: &mut Self,
        _client: &Client,
        resource: &xdg_wm_base::XdgWmBase,
        request: xdg_wm_base::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            xdg_wm_base::Request::CreatePositioner { id } => { data_init.init(id, ()); }
            xdg_wm_base::Request::GetXdgSurface { id, surface } => {
                route_xdg_wm_base_get_surface(state, resource, id, surface, data_init);
            }
            xdg_wm_base::Request::Pong { .. } => {}
            xdg_wm_base::Request::Destroy => {}
            _ => {}
        }
    }

    fn destroyed(
        _state: &mut Self,
        _client: wayland_server::backend::ClientId,
        _resource: &xdg_wm_base::XdgWmBase,
        _data: &(),
    ) {
    }
}

pub fn handle_get_xdg_surface(
    state: &mut State,
    id: New<xdg_surface::XdgSurface>,
    surface: wl_surface::WlSurface,
    data_init: &mut DataInit<'_, State>,
) {
    let surf_id = surface.id();
    let has_role = state.surfaces.get(&surf_id).map(|s| s.has_role).unwrap_or(true);
    if has_role {
        return;
    }
    let xsurf = data_init.init(id, ());
    let xsurf_id = xsurf.id();
    state.xdg_surfaces.insert(xsurf_id, XdgSurfaceData::new(surf_id.clone()));
    if let Some(s) = state.surfaces.get_mut(&surf_id) {
        s.has_role = true;
    }
}

impl Dispatch<xdg_surface::XdgSurface, ()> for State {
    fn request(
        state: &mut Self,
        _client: &Client,
        resource: &xdg_surface::XdgSurface,
        request: xdg_surface::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            xdg_surface::Request::GetToplevel { id } => {
                let xsurf_id = resource.id();
                let surf_id = match state.xdg_surfaces.get(&xsurf_id) {
                    Some(x) => {
                        if x.has_toplevel || x.has_popup {
                            resource.post_error(
                                xdg_surface::Error::AlreadyConstructed as u32,
                                "role already assigned".to_string(),
                            );
                            return;
                        }
                        x.surface.clone()
                    }
                    None => return,
                };
                let top = data_init.init(id, ());
                let top_id = top.id();
                state.toplevels.insert(
                    top_id.clone(),
                    ToplevelData::new(top.clone(), resource.clone(), surf_id.clone()),
                );
                if let Some(x) = state.xdg_surfaces.get_mut(&xsurf_id) {
                    x.has_toplevel = true;
                }

            }
            xdg_surface::Request::GetPopup { id, parent, .. } => {
                let xsurf_id = resource.id();
                let surf_id = match state.xdg_surfaces.get(&xsurf_id) {
                    Some(x) => {
                        if x.has_toplevel || x.has_popup {
                            resource.post_error(
                                xdg_surface::Error::AlreadyConstructed as u32,
                                "role already assigned".to_string(),
                            );
                            return;
                        }
                        x.surface.clone()
                    }
                    None => return,
                };
                let _ = parent;
                let popup = data_init.init(id, ());
                if let Some(x) = state.xdg_surfaces.get_mut(&xsurf_id) {
                    x.has_popup = true;
                }
                if let Some(s) = state.surfaces.get_mut(&surf_id) {
                    s.is_popup = true;
                }
                let serial = state.next_serial();
                resource.configure(serial);
                popup.popup_done();
            }
            xdg_surface::Request::SetWindowGeometry { x: gx, y: gy, width, height } => {
                let xsurf_id = resource.id();
                if width <= 0 || height <= 0 {
                    resource.post_error(
                        xdg_surface::Error::InvalidSize as u32,
                        "non-positive geometry".to_string(),
                    );
                    return;
                }
                if let Some(x) = state.xdg_surfaces.get_mut(&xsurf_id) {
                    x.geometry = Some((gx, gy, width, height));
                }
            }
            xdg_surface::Request::AckConfigure { serial } => {
                let xsurf_id = resource.id();
                let surf_id = match state.xdg_surfaces.get(&xsurf_id) {
                    Some(x) => x.surface.clone(),
                    None => return,
                };
                let mut mapped_now = false;
                for t in state.toplevels.values_mut() {
                    if t.surface == surf_id {
                        if serial == 0 || serial != t.last_configure || serial == t.acked_configure {
                            resource.post_error(
                                xdg_surface::Error::InvalidSerial as u32,
                                "unknown configure serial".to_string(),
                            );
                            return;
                        }
                        t.acked_configure = t.acked_configure.max(serial);
                        mapped_now = true;
                    }
                }
                if mapped_now {
                    state.try_map(&surf_id);
                }
            }
            xdg_surface::Request::Destroy => {}
            _ => {}
        }
    }

    fn destroyed(
        state: &mut Self,
        _client: wayland_server::backend::ClientId,
        resource: &xdg_surface::XdgSurface,
        _data: &(),
    ) {
        let id = resource.id();
        if let Some(x) = state.xdg_surfaces.remove(&id) {
            state.toplevels.retain(|_, t| t.surface != x.surface);
            state.clear_focus(&x.surface);
            state.pending_present = true;
        }
    }
}

impl Dispatch<xdg_positioner::XdgPositioner, ()> for State {
    fn request(
        _state: &mut Self, _client: &Client, resource: &xdg_positioner::XdgPositioner,
        request: xdg_positioner::Request, _data: &(), _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            xdg_positioner::Request::SetSize { width, height } if width <= 0 || height <= 0 => {
                resource.post_error(xdg_positioner::Error::InvalidInput as u32, "positioner size must be positive");
            }
            xdg_positioner::Request::SetAnchorRect { width, height, .. } if width <= 0 || height <= 0 => {
                resource.post_error(xdg_positioner::Error::InvalidInput as u32, "anchor rectangle must be positive");
            }
            _ => {}
        }
    }
}

impl Dispatch<xdg_popup::XdgPopup, ()> for State {
    fn request(
        _state: &mut Self,
        _client: &Client,
        _resource: &xdg_popup::XdgPopup,
        _request: xdg_popup::Request,
        _data: &(),
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
    }

    fn destroyed(
        _state: &mut Self,
        _client: wayland_server::backend::ClientId,
        _resource: &xdg_popup::XdgPopup,
        _data: &(),
    ) {
    }
}

impl Dispatch<xdg_toplevel::XdgToplevel, ()> for State {
    fn request(
        _state: &mut Self,
        _client: &Client,
        _resource: &xdg_toplevel::XdgToplevel,
        request: xdg_toplevel::Request,
        _data: &(),
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            xdg_toplevel::Request::Destroy => {}
            _ => {}
        }
    }

    fn destroyed(
        state: &mut Self,
        _client: wayland_server::backend::ClientId,
        resource: &xdg_toplevel::XdgToplevel,
        _data: &(),
    ) {
        if let Some(top) = state.toplevels.remove(&resource.id()) {
            state.clear_focus(&top.surface);
            state.pending_present = true;
        }
    }
}

impl Dispatch<wl_callback::WlCallback, ()> for State {
    fn request(
        _state: &mut Self,
        _client: &Client,
        _resource: &wl_callback::WlCallback,
        _request: wl_callback::Request,
        _data: &(),
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
    }

    fn destroyed(
        _state: &mut Self,
        _client: wayland_server::backend::ClientId,
        _resource: &wl_callback::WlCallback,
        _data: &(),
    ) {
    }
}

pub fn route_xdg_wm_base_get_surface(
    state: &mut State,
    resource: &xdg_wm_base::XdgWmBase,
    id: New<xdg_surface::XdgSurface>,
    surface: wl_surface::WlSurface,
    data_init: &mut DataInit<'_, State>,
) {
    let surf_id = surface.id();
    let has_role = state.surfaces.get(&surf_id).map(|s| s.has_role).unwrap_or(true);
    if has_role {
        resource.post_error(
            xdg_wm_base::Error::Role as u32,
            "surface already has role".to_string(),
        );
        return;
    }
    handle_get_xdg_surface(state, id, surface, data_init);
}
