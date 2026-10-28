use wayland_server::protocol::{wl_keyboard, wl_pointer, wl_seat};

#[derive(Debug)]
pub struct SeatData {
    pub resource: wl_seat::WlSeat,
    pub pointers: Vec<wl_pointer::WlPointer>,
    pub keyboards: Vec<wl_keyboard::WlKeyboard>,
    pub pointer_focus: Option<wayland_server::backend::ObjectId>,
    pub keyboard_focus: Option<wayland_server::backend::ObjectId>,
}

impl SeatData {
    pub fn new(resource: wl_seat::WlSeat) -> Self {
        SeatData {
            resource,
            pointers: Vec::new(),
            keyboards: Vec::new(),
            pointer_focus: None,
            keyboard_focus: None,
        }
    }

    pub fn capabilities(&self) -> wl_seat::Capability {
        let mut caps = wl_seat::Capability::empty();
        if !self.pointers.is_empty() {
            caps |= wl_seat::Capability::Pointer;
        }
        if !self.keyboards.is_empty() {
            caps |= wl_seat::Capability::Keyboard;
        }
        caps
    }
}

pub fn discover_keyboards() -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let dir = match std::fs::read_dir("/dev/input") {
        Ok(d) => d,
        Err(_) => return out,
    };
    for entry in dir.flatten() {
        let name = entry.file_name();
        let s = name.to_string_lossy();
        if s.starts_with("event") {
            out.push(entry.path());
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_never_panics_without_input_dir() {
        let _ = discover_keyboards();
    }
}
