use crate::dmabuf::DmabufBuffer;
use crate::shm::SurfacePixels;
use std::sync::Arc;
use wayland_server::{backend::ObjectId, Resource};
use wayland_server::protocol::{wl_buffer, wl_callback, wl_surface};

#[derive(Debug, Clone)]
pub enum SurfaceContent {
    Shm(SurfacePixels),
    Dmabuf(DmabufContent),
}

#[derive(Debug, Clone)]
pub struct DmabufContent {
    pub buffer: wl_buffer::WlBuffer,
    pub data: Arc<DmabufBuffer>,
}

impl SurfaceContent {
    pub fn charged_bytes(&self) -> usize {
        match self {
            SurfaceContent::Shm(pixels) => pixels.bytes.len(),
            SurfaceContent::Dmabuf(_) => 0,
        }
    }

    pub fn dmabuf_id(&self) -> Option<ObjectId> {
        match self {
            SurfaceContent::Shm(_) => None,
            SurfaceContent::Dmabuf(content) => Some(content.buffer.id()),
        }
    }
}

#[derive(Debug, Default)]
pub struct PendingState {
    pub buffer: Option<Option<ObjectId>>,
    pub callbacks: Vec<wl_callback::WlCallback>,
}

#[derive(Debug, Default)]
pub struct SurfaceUpdate {
    pub content: Option<Option<SurfaceContent>>,
    pub callbacks: Vec<wl_callback::WlCallback>,
}

impl SurfaceUpdate {
    pub fn merge(&mut self, mut newer: Self) {
        if newer.content.is_some() { self.content = newer.content; }
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
    pub content: Option<SurfaceContent>,
    pub callbacks: Vec<wl_callback::WlCallback>,
    pub parent: Option<ObjectId>,
    pub stacking: Vec<ObjectId>,
    pub pending_stacking: Option<Vec<ObjectId>>,
    pub subsurface_pos: (i32, i32),
    pub pending_pos: Option<(i32, i32)>,
    pub subsurface_sync: bool,
    pub has_role: bool,
    pub is_popup: bool,
}

impl SurfaceData {
    pub fn new(resource: wl_surface::WlSurface) -> Self {
        let id = resource.id();
        Self {
            resource, pending: PendingState::default(), cached: None, cached_pos: None,
            cached_stacking: None, content: None,
            callbacks: Vec::new(), parent: None, stacking: vec![id], pending_stacking: None,
            subsurface_pos: (0, 0), pending_pos: None, subsurface_sync: true,
            has_role: false, is_popup: false,
        }
    }

    pub fn apply(&mut self, mut update: SurfaceUpdate) {
        if let Some(content) = update.content { self.content = content; }
        self.callbacks.append(&mut update.callbacks);
    }
}
