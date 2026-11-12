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
pub struct Region {
    ops: Vec<(RegionOp, Rect)>,
}

impl Region {
    pub fn len(&self) -> usize {
        self.ops.len()
    }

    pub fn add(&mut self, rect: Rect) {
        self.ops.push((RegionOp::Add, rect));
    }

    pub fn subtract(&mut self, rect: Rect) {
        self.ops.push((RegionOp::Subtract, rect));
    }

    pub fn contains(&self, x: f64, y: f64) -> bool {
        self.ops
            .iter()
            .rev()
            .find(|(_, rect)| rect.contains(x, y))
            .is_some_and(|(op, _)| *op == RegionOp::Add)
    }
}

#[derive(Debug, Default)]
pub struct PendingState {
    pub buffer: Option<Option<ObjectId>>,
    pub callbacks: Vec<wl_callback::WlCallback>,
    pub input_region: Option<Option<Region>>,
}

#[derive(Debug, Default)]
pub struct SurfaceUpdate {
    pub pixels: Option<Option<SurfacePixels>>,
    pub callbacks: Vec<wl_callback::WlCallback>,
    pub input_region: Option<Option<Region>>,
}

impl SurfaceUpdate {
    pub fn merge(&mut self, mut newer: Self) {
        if newer.pixels.is_some() { self.pixels = newer.pixels; }
        if newer.input_region.is_some() { self.input_region = newer.input_region; }
        self.callbacks.append(&mut newer.callbacks);
    }
}

#[derive(Debug)]
pub struct SurfaceData {
    pub resource: wl_surface::WlSurface,
    pub pending: PendingState,
    pub cached: Option<SurfaceUpdate>,
    pub cached_pos: Option<(i32, i32)>,
    pub cached_stacking: Option<Vec<ObjectId>>,
    pub pixels: Option<SurfacePixels>,
    pub input_region: Option<Region>,
    pub callbacks: Vec<wl_callback::WlCallback>,
    pub parent: Option<ObjectId>,
    pub stacking: Vec<ObjectId>,
    pub pending_stacking: Option<Vec<ObjectId>>,
    pub subsurface_pos: (i32, i32),
    pub pending_pos: Option<(i32, i32)>,
    pub subsurface_sync: bool,
    pub has_role: bool,
    pub is_popup: bool,
    pub is_cursor: bool,
}

impl SurfaceData {
    pub fn new(resource: wl_surface::WlSurface) -> Self {
        let id = resource.id();
        Self {
            resource, pending: PendingState::default(), cached: None, cached_pos: None,
            cached_stacking: None, pixels: None, input_region: None,
            callbacks: Vec::new(), parent: None, stacking: vec![id], pending_stacking: None,
            subsurface_pos: (0, 0), pending_pos: None, subsurface_sync: true,
            has_role: false, is_popup: false, is_cursor: false,
        }
    }

    pub fn apply(&mut self, mut update: SurfaceUpdate) {
        if let Some(pixels) = update.pixels { self.pixels = pixels; }
        if let Some(region) = update.input_region { self.input_region = region; }
        self.callbacks.append(&mut update.callbacks);
    }
}
