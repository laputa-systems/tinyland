use super::popup::MAX_TREE_DEPTH;
use super::{now_ms, State};
use crate::input::{Motion, PointerFrame};
use crate::positioner::clip_to_i32;
use wayland_server::backend::{ClientId, ObjectId};
use wayland_server::protocol::wl_pointer;
use wayland_server::Resource;

pub const WHEEL_STEP: f64 = 10.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cursor {
    pub client: ClientId,
    pub surface: Option<ObjectId>,
    pub hotspot: (i32, i32),
}

#[derive(Debug)]
pub struct PointerState {
    pub position: (f64, f64),
    pub pressed: Vec<u16>,
    pub focus: Option<ObjectId>,
    pub enter_serial: u32,
    pub cursor: Option<Cursor>,
}

impl PointerState {
    pub fn centered(out_size: (u32, u32)) -> Self {
        PointerState {
            position: (f64::from(out_size.0) / 2.0, f64::from(out_size.1) / 2.0),
            pressed: Vec::new(),
            focus: None,
            enter_serial: 0,
            cursor: None,
        }
    }
}

impl State {
    fn hit_in_tree(&self, id: &ObjectId, origin: (i64, i64), x: f64, y: f64, depth: usize) -> Option<ObjectId> {
        if depth >= MAX_TREE_DEPTH {
            return None;
        }
        let surface = self.surfaces.get(id)?;
        let pixels = surface.pixels.as_ref()?;
        for child in surface.stacking.iter().rev() {
            if child == id {
                let local = (x - origin.0 as f64, y - origin.1 as f64);
                let inside = local.0 >= 0.0 && local.1 >= 0.0 && local.0 < f64::from(pixels.width) && local.1 < f64::from(pixels.height);
                if inside && surface.input_region.as_ref().is_none_or(|region| region.contains(local.0, local.1)) {
                    return Some(id.clone());
                }
            } else if let Some(position) = self.surfaces.get(child).map(|surface| surface.subsurface_pos) {
                let child_origin = (origin.0 + i64::from(position.0), origin.1 + i64::from(position.1));
                if let Some(hit) = self.hit_in_tree(child, child_origin, x, y, depth + 1) {
                    return Some(hit);
                }
            }
        }
        None
    }

    pub(super) fn surface_at(&self, x: f64, y: f64) -> Option<ObjectId> {
        let active = self.active_surface()?;
        let popups = self.live_popups_of(&active);
        popups
            .into_iter()
            .rev()
            .chain(std::iter::once(active))
            .find_map(|root| {
                let origin = self.surface_origin_wide(&root)?;
                self.hit_in_tree(&root, origin, x, y, 0)
            })
    }

    pub(super) fn pointers_of(&self, surf_id: &ObjectId) -> Vec<wl_pointer::WlPointer> {
        let Some(client) = self.surfaces.get(surf_id).and_then(|surface| surface.resource.client()) else {
            return Vec::new();
        };
        self.seats
            .iter()
            .filter(|seat| seat.resource.client().as_ref() == Some(&client))
            .flat_map(|seat| seat.pointers.iter().cloned())
            .collect()
    }

    fn local_position(&self, surf_id: &ObjectId) -> (f64, f64) {
        let (ox, oy) = self.surface_origin_wide(surf_id).unwrap_or((0, 0));
        (self.pointer.position.0 - ox as f64, self.pointer.position.1 - oy as f64)
    }

    fn pointer_frame_to(&self, surf_id: &ObjectId) {
        for pointer in self.pointers_of(surf_id) {
            if pointer.version() >= 5 {
                pointer.frame();
            }
        }
    }

    pub(super) fn enter_pointer(&mut self, pointer: &wl_pointer::WlPointer) {
        let Some(focus) = self.pointer.focus.clone() else { return };
        let Some(surface) = self.surfaces.get(&focus).map(|surface| surface.resource.clone()) else { return };
        if pointer.client() != surface.client() {
            return;
        }
        let (sx, sy) = self.local_position(&focus);
        pointer.enter(self.pointer.enter_serial, &surface, sx, sy);
        if pointer.version() >= 5 {
            pointer.frame();
        }
    }

    pub(super) fn refocus_pointer(&mut self) -> bool {
        if !self.has_pointer {
            return false;
        }
        let focus_alive = self.pointer.focus.as_ref().is_some_and(|focus| self.surfaces.contains_key(focus));
        if !focus_alive {
            self.pointer.focus = None;
        }
        if !self.pointer.pressed.is_empty() && focus_alive {
            return false;
        }
        let (x, y) = self.pointer.position;
        let target = self.surface_at(x, y);
        if target == self.pointer.focus {
            return false;
        }
        let serial = self.next_serial();
        if let Some(old) = self.pointer.focus.take() {
            if let Some(surface) = self.surfaces.get(&old).map(|surface| surface.resource.clone()) {
                for pointer in self.pointers_of(&old) {
                    pointer.leave(serial, &surface);
                }
                self.pointer_frame_to(&old);
            }
        }
        self.pointer.cursor = None;
        self.pointer.enter_serial = serial;
        if let Some(new) = target {
            self.pointer.focus = Some(new.clone());
            for pointer in self.pointers_of(&new) {
                self.enter_pointer(&pointer);
            }
        }
        self.pending_present = true;
        true
    }

    pub(super) fn set_cursor(&mut self, pointer: &wl_pointer::WlPointer, serial: u32, surface: Option<ObjectId>, hotspot: (i32, i32)) {
        let Some(client) = pointer.client().map(|client| client.id()) else { return };
        let focused_client = self.pointer.focus.as_ref().and_then(|focus| self.client_of_surface(focus));
        if focused_client.as_ref() != Some(&client) || serial != self.pointer.enter_serial {
            return;
        }
        if let Some(id) = &surface {
            let Some(data) = self.surfaces.get_mut(id) else { return };
            if data.has_role && !data.is_cursor {
                pointer.post_error(wl_pointer::Error::Role as u32, "cursor surface already has another role");
                return;
            }
            data.has_role = true;
            data.is_cursor = true;
        }
        self.pointer.cursor = Some(Cursor { client, surface, hotspot });
        self.pending_present = true;
    }

    pub(super) fn cursor_placement(&self) -> Option<(ObjectId, (i32, i32))> {
        let cursor = self.pointer.cursor.as_ref()?;
        let focus = self.pointer.focus.as_ref()?;
        if self.client_of_surface(focus).as_ref() != Some(&cursor.client) {
            return None;
        }
        let surface = cursor.surface.clone()?;
        self.surfaces.get(&surface)?.pixels.as_ref()?;
        let x = self.pointer.position.0.floor() as i64 - i64::from(cursor.hotspot.0);
        let y = self.pointer.position.1.floor() as i64 - i64::from(cursor.hotspot.1);
        Some((surface, (clip_to_i32(x), clip_to_i32(y))))
    }

    pub(super) fn pointer_frame(&mut self, frame: PointerFrame) {
        if !self.has_pointer {
            return;
        }
        let time = now_ms();
        let (width, height) = self.out_size;
        let max = (f64::from(width.max(1) - 1), f64::from(height.max(1) - 1));
        if let Some(motion) = frame.motion {
            let (x, y) = match motion {
                Motion::Absolute { x, y } => (x * max.0, y * max.1),
                Motion::Relative { dx, dy } => (self.pointer.position.0 + dx, self.pointer.position.1 + dy),
            };
            self.pointer.position = (x.clamp(0.0, max.0), y.clamp(0.0, max.1));
            if !self.refocus_pointer() {
                if let Some(focus) = self.pointer.focus.clone() {
                    let (sx, sy) = self.local_position(&focus);
                    for pointer in self.pointers_of(&focus) {
                        pointer.motion(time, sx, sy);
                    }
                }
            }
            if self.pointer.cursor.is_some() {
                self.pending_present = true;
            }
        }
        for (button, down) in frame.buttons {
            if down == self.pointer.pressed.contains(&button) {
                continue;
            }
            if down {
                if self.pointer.pressed.is_empty() {
                    self.dismiss_popups_outside_pointer();
                    self.refocus_pointer();
                }
                self.pointer.pressed.push(button);
            } else {
                self.pointer.pressed.retain(|pressed| *pressed != button);
            }
            let serial = self.next_serial();
            let state = if down { wl_pointer::ButtonState::Pressed } else { wl_pointer::ButtonState::Released };
            if let Some(focus) = self.pointer.focus.clone() {
                for pointer in self.pointers_of(&focus) {
                    pointer.button(serial, time, u32::from(button), state);
                }
            }
            if !down && self.pointer.pressed.is_empty() {
                self.pointer_frame_focus();
                self.refocus_pointer();
            }
        }
        let (vertical, horizontal) = frame.wheel;
        if let Some(focus) = self.pointer.focus.clone() {
            let axes = [(wl_pointer::Axis::VerticalScroll, -vertical), (wl_pointer::Axis::HorizontalScroll, horizontal)];
            for pointer in self.pointers_of(&focus) {
                if pointer.version() >= 5 && axes.iter().any(|(_, steps)| *steps != 0) {
                    pointer.axis_source(wl_pointer::AxisSource::Wheel);
                }
                for (axis, steps) in axes.into_iter().filter(|(_, steps)| *steps != 0) {
                    if pointer.version() >= 5 {
                        pointer.axis_discrete(axis, steps);
                    }
                    pointer.axis(time, axis, f64::from(steps) * WHEEL_STEP);
                }
            }
        }
        self.pointer_frame_focus();
    }

    fn pointer_frame_focus(&self) {
        if let Some(focus) = &self.pointer.focus {
            self.pointer_frame_to(focus);
        }
    }
}
