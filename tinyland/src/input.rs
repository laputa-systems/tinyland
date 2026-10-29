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
        if let Some(root) = std::env::var_os("TINYLAND_XKB_ROOT").or_else(|| std::env::var_os("XKB_CONFIG_ROOT")) {
            let path = std::path::PathBuf::from(root);
            if path.exists() {
                if context.include_path_append(path.as_path()) {
                    eprintln!("TINYLAND_XKB_ROOT {}", path.display());
                } else {
                    return Err("XKB include path append failed".into());
                }
            }
        }
        let _ = context.include_path_append_default();
        let keymap = xkb::Keymap::new_from_names(
            &context, "evdev", "pc105", "us", "", Some(String::new()), xkb::KEYMAP_COMPILE_NO_FLAGS,
        );
        let keymap = match keymap {
            Some(keymap) => keymap,
            None => {
                let system = std::path::Path::new("/usr/share/X11/xkb/rules/evdev");
                return Err(format!(
                    "compile US XKB keymap (override={:?}, system-rules-present={})",
                    std::env::var_os("TINYLAND_XKB_ROOT").or_else(|| std::env::var_os("XKB_CONFIG_ROOT")),
                    system.exists(),
                ));
            }
        };
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

    #[allow(dead_code)]
    pub fn reset_stuck(&mut self) -> [u32; 4] {
        for code in 0..256u16 {
            self.state.update_key((code as u32 + 8).into(), xkb::KeyDirection::Up);
        }
        self.modifiers()
    }
}

pub fn open_keyboard(path: &std::path::Path) -> Result<Device, String> {
    let mut device = Device::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    validate_keyboard(&device, path)?;
    device.set_nonblocking(true).map_err(|e| format!("input nonblocking: {e}"))?;
    device.grab().map_err(|e| format!("input grab: {e}"))?;
    eprintln!("TINYLAND_KEYBOARD device={} name={:?}", path.display(), device.name());
    Ok(device)
}

#[allow(dead_code)]
pub fn device_from_fd(fd: std::os::fd::OwnedFd, origin: &str) -> Result<Device, String> {
    let mut device = Device::from_fd(fd).map_err(|e| format!("input from {origin}: {e}"))?;
    let keyboard = device.supported_keys().is_some_and(|keys|
        [KeyCode::KEY_A, KeyCode::KEY_Z, KeyCode::KEY_ENTER].iter().all(|key| keys.contains(*key)));
    if !keyboard {
        return Err(format!("input {origin} is not a keyboard"));
    }
    device.set_nonblocking(true).map_err(|e| format!("input nonblocking: {e}"))?;
    device.grab().map_err(|e| format!("input grab: {e}"))?;
    eprintln!("TINYLAND_KEYBOARD device={origin} name={:?}", device.name());
    Ok(device)
}

fn validate_keyboard(device: &Device, path: &std::path::Path) -> Result<(), String> {
    let keyboard = device.supported_keys().is_some_and(|keys|
        [KeyCode::KEY_A, KeyCode::KEY_Z, KeyCode::KEY_ENTER].iter().all(|key| keys.contains(*key)));
    if !keyboard {
        return Err(format!("input {} is not a keyboard", path.display()));
    }
    Ok(())
}

pub fn discover_keyboard_paths() -> Result<Vec<std::path::PathBuf>, String> {
    let mut entries = std::fs::read_dir("/dev/input")
        .map_err(|e| format!("read /dev/input: {e}"))?
        .collect::<Result<Vec<_>, _>>().map_err(|e| format!("read input entry: {e}"))?;
    entries.sort_by_key(|e| e.path());
    let mut paths = Vec::new();
    for entry in entries {
        if !entry.file_name().as_encoded_bytes().starts_with(b"event") { continue; }
        paths.push(entry.path());
    }
    Ok(paths)
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
        let device = match open_keyboard(&path) {
            Ok(device) => device,
            Err(error) if error.contains("is not a keyboard") => continue,
            Err(error) => return Err(error),
        };
        devices.push(device);
    }
    if devices.is_empty() { return Err("no keyboard with alphabetic and enter keys".into()); }
    Ok(devices)
}

pub fn read_keys(device: &mut Device) -> Result<Vec<(u16, bool)>, std::io::Error> {
    match device.fetch_events() {
        Ok(events) => Ok(events.filter(|event| event.event_type() == EventType::KEY && event.value() != 2)
            .map(|event| (event.code(), event.value() != 0)).collect()),
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

/// True when the read error means the device is gone (unplugged).
pub fn is_gone(error: &std::io::Error) -> bool {
    error.raw_os_error() == Some(rustix::io::Errno::NODEV.raw_os_error())
}

/// VT number requested by a key press under the given depressed modifiers.
/// Evdev F1..F12 are codes 59..68; the modifier word is xkb depressed state
/// in core order (Shift, Lock, Control, Mod1..), so Ctrl+Alt is 0x0c.
/// Returns None unless the chord is complete: Ctrl and Alt held while a
/// function key goes down.
pub fn vt_switch_target(code: u16, depressed: u32) -> Option<i32> {
    if (59..=68).contains(&code) && depressed & 0x0c == 0x0c {
        Some((code - 58) as i32)
    } else {
        None
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

    #[test]
    fn reset_clears_depressed_modifiers_for_resume() {
        let mut keyboard = Keyboard::new().unwrap();
        assert_ne!(keyboard.update(KeyCode::KEY_LEFTSHIFT.code(), true)[0], 0);
        assert_eq!(keyboard.reset_stuck(), [0, 0, 0, 0]);
    }

    #[test]
    fn gone_matches_unplug_errno_only() {
        let gone = std::io::Error::from_raw_os_error(rustix::io::Errno::NODEV.raw_os_error());
        assert!(is_gone(&gone));
        assert!(!is_gone(&std::io::Error::from(std::io::ErrorKind::WouldBlock)));
        assert!(!is_gone(&std::io::Error::from_raw_os_error(
            rustix::io::Errno::INVAL.raw_os_error()
        )));
    }

    #[test]
    fn vt_switch_needs_ctrl_alt_and_function_key() {
        let mut keyboard = Keyboard::new().unwrap();
        assert_eq!(vt_switch_target(KeyCode::KEY_F2.code(), keyboard.modifiers()[0]), None);
        keyboard.update(KeyCode::KEY_LEFTCTRL.code(), true);
        assert_eq!(vt_switch_target(KeyCode::KEY_F2.code(), keyboard.modifiers()[0]), None);
        keyboard.update(KeyCode::KEY_LEFTALT.code(), true);
        assert_eq!(vt_switch_target(KeyCode::KEY_F2.code(), keyboard.modifiers()[0]), Some(2));
        assert_eq!(vt_switch_target(KeyCode::KEY_F12.code(), keyboard.modifiers()[0]), Some(12));
        assert_eq!(vt_switch_target(KeyCode::KEY_A.code(), keyboard.modifiers()[0]), None);
        keyboard.update(KeyCode::KEY_LEFTCTRL.code(), false);
        assert_eq!(vt_switch_target(KeyCode::KEY_F2.code(), keyboard.modifiers()[0]), None);
    }

    #[test]
    fn seat_fd_without_keyboard_is_rejected() {
        let fd = rustix::fs::memfd_create("tinyland-test", rustix::fs::MemfdFlags::CLOEXEC).unwrap();
        assert!(device_from_fd(fd, "test").is_err());
    }
}
