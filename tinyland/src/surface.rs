use crate::damage::Damage;
use crate::positioner::Rect;
use crate::shm::SurfacePixels;
use wayland_server::{backend::ObjectId, Resource};
use wayland_server::protocol::{wl_callback, wl_surface};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RegionOp {
    Add,
    Subtract,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Region {
    ops: Vec<(RegionOp, Rect)>,
}

impl Region {
    pub(crate) fn len(&self) -> usize {
        self.ops.len()
    }

    pub(crate) fn add(&mut self, rect: Rect) {
        self.ops.push((RegionOp::Add, rect));
    }

    pub(crate) fn subtract(&mut self, rect: Rect) {
        self.ops.push((RegionOp::Subtract, rect));
    }

    pub(crate) fn contains(&self, x: f64, y: f64) -> bool {
        self.ops
            .iter()
            .rev()
            .find(|(_, rect)| rect.contains(x, y))
            .is_some_and(|(op, _)| *op == RegionOp::Add)
    }
}

#[derive(Debug, Default)]
pub(crate) struct PendingState {
    pub buffer: Option<Option<ObjectId>>,
    /// Damage and DamageBuffer since the last commit, in buffer pixels.
    pub damage: Damage,
    pub callbacks: Vec<wl_callback::WlCallback>,
    pub input_region: Option<Option<Region>>,
}

#[derive(Debug, Default)]
pub(crate) struct SurfaceUpdate {
    pub pixels: Option<Option<SurfacePixels>>,
    /// Where `pixels` differ from the pixels it replaces, in buffer pixels.
    pub damage: Damage,
    pub callbacks: Vec<wl_callback::WlCallback>,
    pub input_region: Option<Option<Region>>,
}

impl SurfaceUpdate {
    pub(crate) fn merge(&mut self, mut newer: Self) {
        if newer.pixels.is_some() { self.pixels = newer.pixels; self.damage.merge(&newer.damage); }
        if newer.input_region.is_some() { self.input_region = newer.input_region; }
        self.callbacks.append(&mut newer.callbacks);
    }
}

#[derive(Debug)]
pub(crate) struct SurfaceData {
    pub resource: wl_surface::WlSurface,
    pub pending: PendingState,
    pub cached: Option<SurfaceUpdate>,
    pub cached_pos: Option<(i32, i32)>,
    pub cached_stacking: Option<Vec<ObjectId>>,
    pub pixels: Option<SurfacePixels>,
    /// Where `pixels` changed since the output was last composed, in buffer pixels.
    pub damage: Damage,
    /// Set once the client asks for a buffer scale or transform the renderer ignores; its damage is then unknown.
    pub unscaled_damage_unknown: bool,
    pub input_region: Option<Region>,
    pub callbacks: Vec<wl_callback::WlCallback>,
    pub parent: Option<ObjectId>,
    pub stacking: Vec<ObjectId>,
    pub pending_stacking: Option<Vec<ObjectId>>,
    pub subsurface_pos: (i32, i32),
    pub pending_pos: Option<(i32, i32)>,
    pub subsurface_sync: bool,
    pub has_role: bool,
    pub is_subsurface: bool,
    pub is_popup: bool,
    pub is_cursor: bool,
}

impl SurfaceData {
    pub(crate) fn new(resource: wl_surface::WlSurface) -> Self {
        let id = resource.id();
        Self {
            resource, pending: PendingState::default(), cached: None, cached_pos: None,
            cached_stacking: None, pixels: None, damage: Damage::default(), unscaled_damage_unknown: false, input_region: None,
            callbacks: Vec::new(), parent: None, stacking: vec![id], pending_stacking: None,
            subsurface_pos: (0, 0), pending_pos: None, subsurface_sync: true,
            has_role: false, is_subsurface: false, is_popup: false, is_cursor: false,
        }
    }

    pub(crate) fn apply(&mut self, mut update: SurfaceUpdate) {
        if let Some(pixels) = update.pixels { self.pixels = pixels; self.damage.merge(&update.damage); }
        if let Some(region) = update.input_region { self.input_region = region; }
        self.callbacks.append(&mut update.callbacks);
    }
}
