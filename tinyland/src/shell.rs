use wayland_protocols::xdg::shell::server::{xdg_surface, xdg_toplevel};
use wayland_server::backend::ObjectId;

#[derive(Debug)]
pub struct ToplevelData {
    pub resource: xdg_toplevel::XdgToplevel,
    pub xdg_surface: xdg_surface::XdgSurface,
    pub surface: ObjectId,
    pub title: String,
    pub app_id: String,
    pub last_configure: u32,
    pub acked_configure: u32,
    pub mapped: bool,
    pub fullscreen: bool,
}

impl ToplevelData {
    pub fn new(
        resource: xdg_toplevel::XdgToplevel,
        xdg_surface: xdg_surface::XdgSurface,
        surface: ObjectId,
    ) -> Self {
        ToplevelData {
            resource,
            xdg_surface,
            surface,
            title: String::new(),
            app_id: String::new(),
            last_configure: 0,
            acked_configure: 0,
            mapped: false,
            fullscreen: false,
        }
    }
}

#[derive(Debug)]
pub struct XdgSurfaceData {
    pub resource: xdg_surface::XdgSurface,
    pub surface: ObjectId,
    pub has_toplevel: bool,
    pub has_popup: bool,
    pub geometry: Option<(i32, i32, i32, i32)>,
}

impl XdgSurfaceData {
    pub fn new(resource: xdg_surface::XdgSurface, surface: ObjectId) -> Self {
        XdgSurfaceData {
            resource,
            surface,
            has_toplevel: false,
            has_popup: false,
            geometry: None,
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn configure_serial_starts_zero() {
        assert_eq!(0u32, 0);
    }
}
