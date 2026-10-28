use wayland_server::backend::ObjectId;
use wayland_server::protocol::{wl_callback, wl_surface};

#[derive(Debug, Clone, Default)]
pub struct PendingState {
    pub buffer: Option<Option<ObjectId>>,
    pub offset: (i32, i32),
    pub damage_full: bool,
    pub scale: i32,
}

impl PendingState {
    pub fn fresh() -> Self {
        PendingState { scale: 1, ..Default::default() }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CommittedState {
    pub buffer: Option<ObjectId>,
    pub scale: i32,
}

#[derive(Debug)]
pub struct SurfaceData {
    pub resource: wl_surface::WlSurface,
    pub pending: PendingState,
    pub current: CommittedState,
    pub callbacks: Vec<wl_callback::WlCallback>,
    pub parent: Option<ObjectId>,
    pub children: Vec<ObjectId>,
    pub subsurface_pos: (i32, i32),
    pub subsurface_sync: bool,
    pub has_role: bool,
    pub is_toplevel: bool,
    pub is_popup: bool,
}

impl SurfaceData {
    pub fn new(resource: wl_surface::WlSurface) -> Self {
        SurfaceData {
            resource,
            pending: PendingState::fresh(),
            current: CommittedState::default(),
            callbacks: Vec::new(),
            parent: None,
            children: Vec::new(),
            subsurface_pos: (0, 0),
            subsurface_sync: true,
            has_role: false,
            is_toplevel: false,
            is_popup: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_defaults_to_scale_one() {
        let p = PendingState::fresh();
        assert_eq!(p.scale, 1);
        assert!(p.buffer.is_none());
    }
}
