use evdev::{Device, EventType, KeyCode};
use std::fs::File;
use std::io::Write;
use wayland_server::protocol::{wl_keyboard, wl_seat};
use xkbcommon::xkb;

#[derive(Debug)]
pub struct SeatData {
    pub resource: wl_seat::WlSeat,
    pub keyboards: Vec<wl_keyboard::WlKeyboard>,
    pub keyboard_focus: Option<wayland_server::backend::ObjectId>,
}

impl SeatData {
    pub fn new(resource: wl_seat::WlSeat) -> Self {
        Self { resource, keyboards: Vec::new(), keyboard_focus: None }
    }
}

pub struct Keyboard {
    state: xkb::State,
    pub keymap: File,
    pub keymap_size: u32,
}

impl Keyboard {
    pub fn new() -> Result<Self, String> {
        let mut context = xkb::Context::new(xkb::CONTEXT_NO_DEFAULT_INCLUDES);
        if !context.include_path_append_default() { return Err("XKB include paths unavailable".into()); }
        let keymap = xkb::Keymap::new_from_names(
            &context, "evdev", "pc105", "us", "", Some(String::new()), xkb::KEYMAP_COMPILE_NO_FLAGS,
        ).ok_or("compile US XKB keymap")?;
        let mut text = keymap.get_as_string(xkb::KEYMAP_FORMAT_TEXT_V1).into_bytes();
        text.push(0);
        let mut file = File::from(rustix::fs::memfd_create("tinyland-keymap", rustix::fs::MemfdFlags::CLOEXEC)
            .map_err(|e| format!("keymap memfd: {e}"))?);
        file.write_all(&text).map_err(|e| format!("keymap write: {e}"))?;
        Ok(Self { state: xkb::State::new(&keymap), keymap: file, keymap_size: text.len() as u32 })
    }

    pub fn update(&mut self, code: u16, down: bool) -> [u32; 4] {
        self.state.update_key((code as u32 + 8).into(), if down { xkb::KeyDirection::Down } else { xkb::KeyDirection::Up });
        self.modifiers()
    }

    pub fn modifiers(&self) -> [u32; 4] {
        [self.state.serialize_mods(xkb::STATE_MODS_DEPRESSED),
         self.state.serialize_mods(xkb::STATE_MODS_LATCHED),
         self.state.serialize_mods(xkb::STATE_MODS_LOCKED),
         self.state.serialize_layout(xkb::STATE_LAYOUT_EFFECTIVE)]
    }
}

pub fn discover_keyboards() -> Result<Vec<Device>, String> {
    let mut entries = std::fs::read_dir("/dev/input")
        .map_err(|e| format!("read /dev/input: {e}"))?
        .collect::<Result<Vec<_>, _>>().map_err(|e| format!("read input entry: {e}"))?;
    entries.sort_by_key(|e| e.path());
    let mut devices = Vec::new();
    for entry in entries {
        if !entry.file_name().as_encoded_bytes().starts_with(b"event") { continue; }
        let path = entry.path();
        let mut device = Device::open(&path).map_err(|e| format!("open {}: {e}", path.display()))?;
        let keyboard = device.supported_keys().is_some_and(|keys|
            [KeyCode::KEY_A, KeyCode::KEY_Z, KeyCode::KEY_ENTER].iter().all(|key| keys.contains(*key)));
        if !keyboard { continue; }
        device.set_nonblocking(true).map_err(|e| format!("input nonblocking: {e}"))?;
        device.grab().map_err(|e| format!("input grab: {e}"))?;
        eprintln!("TINYLAND_KEYBOARD device={} name={:?}", path.display(), device.name());
        devices.push(device);
    }
    if devices.is_empty() { return Err("no keyboard with alphabetic and enter keys".into()); }
    Ok(devices)
}

pub fn read_keys(device: &mut Device) -> Result<Vec<(u16, bool)>, String> {
    match device.fetch_events() {
        Ok(events) => Ok(events.filter(|event| event.event_type() == EventType::KEY && event.value() != 2)
            .map(|event| (event.code(), event.value() != 0)).collect()),
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(Vec::new()),
        Err(e) => Err(format!("read keyboard: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shift_changes_text_and_releases_its_modifier() {
        let mut keyboard = Keyboard::new().unwrap();
        let a = (KeyCode::KEY_A.code() as u32 + 8).into();
        assert_eq!(keyboard.state.key_get_utf8(a), "a");
        assert_ne!(keyboard.update(KeyCode::KEY_LEFTSHIFT.code(), true)[0], 0);
        assert_eq!(keyboard.state.key_get_utf8(a), "A");
        assert_eq!(keyboard.update(KeyCode::KEY_LEFTSHIFT.code(), false)[0], 0);
        assert_eq!(keyboard.state.key_get_utf8(a), "a");
    }
}
