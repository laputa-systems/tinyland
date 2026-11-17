use super::*;
use crate::input::{PointerFrame, BTN_LEFT};
use crate::seat::fake_broker::{seat_opened_body, start_server};
use seatd_protocol::wire::{
    CLIENT_CLOSE_DEVICE, CLIENT_DISABLE_SEAT, CLIENT_OPEN_SEAT, SERVER_DEVICE_CLOSED,
    SERVER_DISABLE_SEAT, SERVER_ENABLE_SEAT, SERVER_SEAT_DISABLED, SERVER_SEAT_OPENED,
};
use std::collections::VecDeque;
use std::time::Duration;

const CALLBACK_DONE: u16 = 0;
const KEYBOARD_KEY: u16 = 3;
const KEY_A: u16 = 30;
const KEY_B: u16 = 48;
const KEY_CAPSLOCK: u16 = 58;
const KEY_LEFTCTRL: u16 = 29;

fn done_times(events: &Events, callback: u32) -> Vec<u32> {
    events
        .iter()
        .filter(|(object, opcode, _)| *object == callback && *opcode == CALLBACK_DONE)
        .map(|(_, _, args)| args[0])
        .collect()
}

fn key_events(events: &Events, keyboard: &impl Resource) -> Vec<(u32, u32)> {
    let mut keys: Vec<(u32, u32)> = sent(events, keyboard, KEYBOARD_KEY).iter().map(|args| (args[2], args[3])).collect();
    keys.sort();
    keys
}

fn last_depressed(events: &Events, keyboard: &impl Resource) -> Option<u32> {
    sent(events, keyboard, KEYBOARD_MODIFIERS).last().map(|args| args[1])
}

fn mapped_frame(f: &mut Fixture, fill: u8) -> wl_surface::WlSurface {
    let (surface, _xdg, _) = mapped_toplevel(f, fill);
    f.state.submit_frame();
    f.state.complete_frame(1);
    f.events();
    surface
}

#[test]
fn a_commit_during_an_earlier_flight_stays_dirty_and_completes_only_after_its_own_flight() {
    let mut f = Fixture::new();
    let surface = mapped_frame(&mut f, 1);
    f.request(&surface, 3, &[2]);
    f.show(&surface, 2, 1, 2);
    f.state.submit_frame();
    f.request(&surface, 3, &[3]);
    f.show(&surface, 2, 1, 3);

    f.state.complete_frame(100);
    let events = f.events();
    assert_eq!(done_times(&events, 2), [100]);
    assert!(done_times(&events, 3).is_empty(), "the later commit was not in the completed frame");
    assert!(f.state.pending_present, "the later commit is still waiting to be displayed");

    let frame = f.state.submit_frame();
    assert_eq!(frame[0], 3);
    f.state.complete_frame(200);
    let events = f.events();
    assert_eq!(done_times(&events, 3), [200]);
    assert!(!f.state.pending_present);
}

#[test]
fn destroying_a_surface_during_its_flight_sends_it_nothing_and_keeps_the_newer_scene_dirty() {
    let mut f = Fixture::new();
    let surface = mapped_frame(&mut f, 1);
    f.request(&surface, 3, &[2]);
    f.show(&surface, 2, 1, 2);
    f.state.submit_frame();
    f.state.pending_present = false;
    f.request(&surface, 0, &[]);
    assert!(f.state.pending_present, "closing the window changes the scene");

    f.state.complete_frame(100);
    let events = f.events();
    assert!(done_times(&events, 2).is_empty());
    assert!(f.state.pending_present);
}

#[test]
fn an_abandoned_flight_keeps_its_callbacks_for_the_next_presentation() {
    let mut f = Fixture::new();
    let surface = mapped_frame(&mut f, 1);
    f.request(&surface, 3, &[2]);
    f.show(&surface, 2, 1, 2);
    f.state.submit_frame();

    f.state.abandon_frame();
    assert!(f.events().is_empty(), "an abandoned flight acknowledges nothing");
    assert!(f.state.pending_present);

    f.state.submit_frame();
    f.state.complete_frame(300);
    assert_eq!(done_times(&f.events(), 2), [300]);
}

fn keyboard_world() -> (Fixture, wayland_server::protocol::wl_keyboard::WlKeyboard) {
    let mut f = Fixture::with_output((100, 100));
    f.pointer_seat();
    mapped_toplevel(&mut f, 3);
    f.events();
    let keyboard = f.state.seats[0].keyboards[0].clone();
    (f, keyboard)
}

fn key(f: &mut Fixture, device: u32, code: u16, down: bool) {
    f.state.input_event(InputId(device), InputEvent::Key { code, down });
}

fn button(f: &mut Fixture, device: u32, down: bool) {
    f.state.input_event(
        InputId(device),
        InputEvent::Pointer(PointerFrame { buttons: vec![(BTN_LEFT, down)], ..Default::default() }),
    );
}

#[test]
fn losing_a_keyboard_releases_its_keys_and_modifiers_to_the_client() {
    let (mut f, keyboard) = keyboard_world();
    key(&mut f, 1, KEY_LEFTSHIFT, true);
    key(&mut f, 1, KEY_A, true);
    f.events();

    f.state.input_lost(InputId(1));
    let events = f.events();
    assert_eq!(key_events(&events, &keyboard), [(u32::from(KEY_A), 0), (u32::from(KEY_LEFTSHIFT), 0)]);
    assert_eq!(last_depressed(&events, &keyboard), Some(0));
    assert_eq!(f.state.keyboard.modifiers()[0], 0);
}

#[test]
fn a_key_held_on_two_devices_stays_pressed_until_its_last_owner_goes() {
    let (mut f, keyboard) = keyboard_world();
    key(&mut f, 1, KEY_LEFTSHIFT, true);
    key(&mut f, 2, KEY_LEFTSHIFT, true);
    let events = f.events();
    assert_eq!(key_events(&events, &keyboard), [(u32::from(KEY_LEFTSHIFT), 1)], "one logical press");

    f.state.input_lost(InputId(1));
    let events = f.events();
    assert!(key_events(&events, &keyboard).is_empty(), "the other device still holds the key");
    assert_ne!(f.state.keyboard.modifiers()[0], 0);

    f.state.input_lost(InputId(2));
    assert_eq!(key_events(&f.events(), &keyboard), [(u32::from(KEY_LEFTSHIFT), 0)]);
    assert_eq!(f.state.keyboard.modifiers()[0], 0);
}

#[test]
fn removing_an_unrelated_device_leaves_another_devices_key_held() {
    let (mut f, keyboard) = keyboard_world();
    key(&mut f, 1, KEY_A, true);
    f.events();

    f.state.input_lost(InputId(3));
    assert!(f.events().is_empty());

    key(&mut f, 1, KEY_A, false);
    assert_eq!(key_events(&f.events(), &keyboard), [(u32::from(KEY_A), 0)]);
}

#[test]
fn a_release_from_a_device_that_never_pressed_the_key_is_not_delivered() {
    let (mut f, keyboard) = keyboard_world();
    key(&mut f, 1, KEY_A, true);
    f.state.suspend_input();
    f.events();

    key(&mut f, 2, KEY_A, false);
    key(&mut f, 2, KEY_B, true);
    key(&mut f, 2, KEY_B, false);
    assert_eq!(
        key_events(&f.events(), &keyboard),
        [(u32::from(KEY_B), 0), (u32::from(KEY_B), 1)],
        "the key held across the suspend was released once, at the suspend"
    );
}

#[test]
fn suspending_releases_held_keys_and_resets_locks_for_the_focused_client() {
    let (mut f, keyboard) = keyboard_world();
    key(&mut f, 1, KEY_LEFTCTRL, true);
    key(&mut f, 1, KEY_A, true);
    key(&mut f, 1, KEY_CAPSLOCK, true);
    key(&mut f, 1, KEY_CAPSLOCK, false);
    assert_ne!(f.state.keyboard.modifiers()[2], 0);
    f.events();

    f.state.suspend_input();
    let events = f.events();
    assert_eq!(key_events(&events, &keyboard), [(u32::from(KEY_LEFTCTRL), 0), (u32::from(KEY_A), 0)]);
    assert_eq!(f.state.keyboard.modifiers(), [0, 0, 0, 0]);
    let last = sent(&events, &keyboard, KEYBOARD_MODIFIERS).last().map(|args| args[1..].to_vec());
    assert_eq!(last, Some(vec![0, 0, 0, 0]));
}

#[test]
fn a_newly_focused_client_is_told_which_keys_are_held() {
    let (mut f, keyboard) = keyboard_world();
    key(&mut f, 1, KEY_A, true);
    let second = f.surface();
    let second_xdg = f.toplevel(&second);
    f.request(&second_xdg, 4, &[7]);
    let events = f.show(&second, 100, 100, 4);
    let enter = sent(&events, &keyboard, KEYBOARD_ENTER);
    assert_eq!(enter.len(), 1);
    assert_eq!(enter[0][2..], [4, u32::from(KEY_A)]);
}

fn pointer_world() -> (Fixture, wayland_server::protocol::wl_pointer::WlPointer, wl_surface::WlSurface) {
    let mut f = Fixture::with_output((100, 100));
    let pointer = f.pointer_seat();
    let (first, _xdg, _) = mapped_toplevel(&mut f, 1);
    f.move_pointer(10.0, 10.0);
    f.events();
    (f, pointer, first)
}

#[test]
fn suspending_with_a_button_held_ends_the_grab_and_lets_the_pointer_refocus() {
    let (mut f, pointer, first) = pointer_world();
    button(&mut f, 1, true);
    let (second, _xdg, _) = mapped_toplevel(&mut f, 2);
    assert_eq!(f.state.pointer.focus, Some(first.id()), "the grab keeps the first surface");
    f.events();

    f.state.suspend_input();
    let events = f.events();
    let buttons = sent(&events, &pointer, POINTER_BUTTON);
    assert_eq!(buttons.len(), 1);
    assert_eq!(buttons[0][3], 0);
    assert!(f.state.pointer.pressed.is_empty());
    assert_eq!(f.state.pointer.focus, Some(second.id()));
    assert_eq!(sent(&events, &pointer, POINTER_ENTER)[0][1], second.id().protocol_id());
}

#[test]
fn losing_the_pointer_device_ends_its_grab_but_not_a_grab_another_device_shares() {
    let (mut f, pointer, first) = pointer_world();
    button(&mut f, 1, true);
    button(&mut f, 2, true);
    let (second, _xdg, _) = mapped_toplevel(&mut f, 2);
    f.events();

    f.state.input_lost(InputId(1));
    let events = f.events();
    assert!(sent(&events, &pointer, POINTER_BUTTON).is_empty(), "device 2 still holds the button");
    assert_eq!(f.state.pointer.focus, Some(first.id()));

    f.state.input_lost(InputId(2));
    let events = f.events();
    assert_eq!(sent(&events, &pointer, POINTER_BUTTON).len(), 1);
    assert_eq!(f.state.pointer.focus, Some(second.id()));
}

struct FakeDevices {
    count: usize,
    reads: VecDeque<Vec<(InputId, InputRead)>>,
    log: Vec<&'static str>,
    broker_ids: Vec<i32>,
}

impl FakeDevices {
    fn new(count: usize, broker_ids: &[i32]) -> Self {
        Self { count, reads: VecDeque::new(), log: Vec::new(), broker_ids: broker_ids.to_vec() }
    }
}

impl SeatDevices for FakeDevices {
    fn read_inputs(&mut self) -> Result<Vec<(InputId, InputRead)>, String> {
        Ok(self.reads.pop_front().unwrap_or_default())
    }

    fn remove_input(&mut self, _id: InputId, _broker: Option<&Session<'_>>) {
        self.count -= 1;
        self.log.push("remove");
    }

    fn input_count(&self) -> usize {
        self.count
    }

    fn release(&mut self, broker: &Session<'_>) -> Result<(), String> {
        self.log.push("release");
        self.count = 0;
        for id in &self.broker_ids {
            broker.close_device(*id, "fake")?;
        }
        Ok(())
    }

    fn reopen(&mut self, _broker: &Session<'_>) -> Result<(), String> {
        self.log.push("reopen");
        self.count = 1;
        Ok(())
    }
}

fn never() -> bool {
    false
}

fn seat_with<'a>(
    client: std::os::fd::OwnedFd,
    cancel: &'a dyn Fn() -> bool,
    devices: FakeDevices,
) -> Seat<'a, FakeDevices> {
    let session = Session::from_conn(client, cancel, Duration::from_secs(5)).unwrap();
    Seat { broker: Some(session), devices, suspended: false }
}

fn disable_script(
    go: std::sync::mpsc::Receiver<()>,
    release: std::sync::mpsc::Receiver<()>,
) -> (std::os::fd::OwnedFd, std::thread::JoinHandle<()>) {
    start_server(move |conn| {
        assert_eq!(conn.read_msg().0, CLIENT_OPEN_SEAT);
        conn.send(SERVER_SEAT_OPENED, &seat_opened_body("seat0"));
        go.recv().unwrap();
        conn.send(SERVER_DISABLE_SEAT, &[]);
        let (opcode, body) = conn.read_msg();
        assert_eq!((opcode, i32::from_ne_bytes(body.try_into().unwrap())), (CLIENT_CLOSE_DEVICE, 1));
        conn.send(SERVER_DEVICE_CLOSED, &[]);
        let (opcode, body) = conn.read_msg();
        assert_eq!((opcode, i32::from_ne_bytes(body.try_into().unwrap())), (CLIENT_CLOSE_DEVICE, 2));
        conn.send(SERVER_DEVICE_CLOSED, &[]);
        assert_eq!(conn.read_msg().0, CLIENT_DISABLE_SEAT);
        conn.send(SERVER_SEAT_DISABLED, &[]);
        conn.send(SERVER_ENABLE_SEAT, &[]);
        release.recv().unwrap();
    })
}

#[test]
fn a_disable_that_arrives_while_the_last_input_reads_enodev_suspends_instead_of_failing() {
    let (go, wait_go) = std::sync::mpsc::channel();
    let (release, wait_release) = std::sync::mpsc::channel();
    let (client, server) = disable_script(wait_go, wait_release);
    let mut f = Fixture::new();
    let cancel = never;
    let mut seat = seat_with(client, &cancel, FakeDevices::new(1, &[1, 2]));

    seat.service_broker(&mut f.state).unwrap();
    assert!(!seat.suspended);
    go.send(()).unwrap();
    seat.devices.reads.push_back(vec![(InputId(0), InputRead::Gone)]);
    seat.service_inputs(&mut f.state).expect("the compositor stays alive");

    assert!(seat.suspended);
    assert_eq!(seat.devices.log, ["release"]);
    for _ in 0..100 {
        if !seat.suspended {
            break;
        }
        seat.service_broker(&mut f.state).unwrap();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!seat.suspended, "the seat resumed on enable");
    assert_eq!(seat.devices.log, ["release", "reopen"]);
    release.send(()).unwrap();
    server.join().unwrap();
}

#[test]
fn input_collected_in_the_tick_a_disable_arrives_is_discarded() {
    let (mut f, keyboard) = keyboard_world();
    let (go, wait_go) = std::sync::mpsc::channel();
    let (release, wait_release) = std::sync::mpsc::channel();
    let (client, server) = disable_script(wait_go, wait_release);
    let cancel = never;
    let mut seat = seat_with(client, &cancel, FakeDevices::new(2, &[1, 2]));

    go.send(()).unwrap();
    let press = (std::time::SystemTime::now(), InputEvent::Key { code: KEY_A, down: true });
    seat.devices.reads.push_back(vec![
        (InputId(0), InputRead::Gone),
        (InputId(1), InputRead::Events(vec![press])),
    ]);
    seat.service_inputs(&mut f.state).unwrap();

    assert!(seat.suspended);
    assert!(key_events(&f.events(), &keyboard).is_empty(), "a press from before the suspend is not replayed");
    release.send(()).unwrap();
    server.join().unwrap();
}

#[test]
fn enodev_without_a_disable_is_a_real_unplug() {
    let (release, wait_release) = std::sync::mpsc::channel();
    let (client, server) = start_server(move |conn| {
        assert_eq!(conn.read_msg().0, CLIENT_OPEN_SEAT);
        conn.send(SERVER_SEAT_OPENED, &seat_opened_body("seat0"));
        wait_release.recv().unwrap();
    });
    let mut f = Fixture::new();
    let cancel = never;
    let mut seat = seat_with(client, &cancel, FakeDevices::new(1, &[1, 2]));

    seat.devices.reads.push_back(vec![(InputId(0), InputRead::Gone)]);
    let error = seat.service_inputs(&mut f.state).unwrap_err();
    assert!(error.contains("no input devices remain"), "unexpected: {error}");
    assert!(!seat.suspended);
    assert_eq!(seat.devices.log, ["remove"]);
    release.send(()).unwrap();
    server.join().unwrap();
}

#[test]
fn a_keyboard_bound_while_a_key_is_held_is_told_which_keys_are_held() {
    let (mut f, _keyboard) = keyboard_world();
    key(&mut f, 1, KEY_A, true);
    f.events();
    let seat = f.state.seats[0].resource.clone();
    const NEW_KEYBOARD: u32 = 2;
    let events = messages(&f.request(&seat, 1, &[NEW_KEYBOARD]));
    let enter: Vec<_> = events.iter().filter(|(object, opcode, _)| *object == NEW_KEYBOARD && *opcode == KEYBOARD_ENTER).collect();
    assert_eq!(enter.len(), 1);
    assert_eq!(enter[0].2[2..], [4, u32::from(KEY_A)]);
}

#[test]
fn remote_ack_completes_only_its_submitted_callbacks() {
    let mut f = Fixture::new();
    let surface = mapped_frame(&mut f, 1);
    f.request(&surface, 3, &[2]);
    f.show(&surface, 2, 1, 2);
    let size = tinyland_remote::Size::new(2, 1).unwrap();
    let mut transport = super::super::remote::Transport::test_transport();
    transport.queue_frame(&mut f.state, size).unwrap();
    transport.write().unwrap();
    assert!(done_times(&f.events(), 2).is_empty());
    f.request(&surface, 3, &[3]);
    f.show(&surface, 2, 1, 3);
    assert!(transport.handle(&mut f.state, size, tinyland_remote::HostMessage::Presented { serial: 2 }).is_err());
    assert!(done_times(&f.events(), 2).is_empty());
    transport.handle(&mut f.state, size, tinyland_remote::HostMessage::Presented { serial: 1 }).unwrap();
    assert_eq!(done_times(&f.events(), 2).len(), 1);
    assert!(transport.handle(&mut f.state, size, tinyland_remote::HostMessage::Presented { serial: 1 }).is_err());
    assert!(f.state.pending_present);
    assert!(done_times(&f.events(), 3).is_empty());
    transport.queue_frame(&mut f.state, size).unwrap();
    transport.write().unwrap();
    transport.handle(&mut f.state, size, tinyland_remote::HostMessage::Presented { serial: 2 }).unwrap();
    assert_eq!(done_times(&f.events(), 3).len(), 1);
}

#[test]
fn remote_disconnect_releases_only_that_sources_modifiers_and_keys() {
    use tinyland_remote::{HostMessage, Size};
    let (mut f, keyboard) = keyboard_world();
    let size = Size::new(100, 100).unwrap();
    let mut transport = super::super::remote::Transport::test_transport();
    for source in [1, 2] {
        transport.handle(&mut f.state, size, HostMessage::Key { source, code: KEY_LEFTSHIFT, pressed: true }).unwrap();
    }
    f.events();
    transport.handle(&mut f.state, size, HostMessage::ResetInput { source: 1 }).unwrap();
    assert!(key_events(&f.events(), &keyboard).is_empty());
    transport.handle(&mut f.state, size, HostMessage::ResetInput { source: 2 }).unwrap();
    let events = f.events();
    assert_eq!(key_events(&events, &keyboard), [(u32::from(KEY_LEFTSHIFT), 0)]);
    assert_eq!(last_depressed(&events, &keyboard), Some(0));
}

#[test]
fn remote_pointer_buttons_remain_owned_until_last_source_resets() {
    use tinyland_remote::{Button, HostMessage, Size};
    let (mut f, pointer, _) = pointer_world();
    let size = Size::new(100, 100).unwrap();
    let mut transport = super::super::remote::Transport::test_transport();
    transport.handle(&mut f.state, size, HostMessage::Pointer { source: 1, x: 20, y: 30 }).unwrap();
    assert!((f.state.pointer.position.0 - 20.0).abs() < 0.001);
    assert!((f.state.pointer.position.1 - 30.0).abs() < 0.001);
    for source in [1, 2] {
        transport.handle(&mut f.state, size, HostMessage::Button { source, button: Button::Left, pressed: true }).unwrap();
    }
    let events = f.events();
    assert_eq!(sent(&events, &pointer, POINTER_BUTTON).len(), 1);
    transport.handle(&mut f.state, size, HostMessage::ResetInput { source: 1 }).unwrap();
    assert!(sent(&f.events(), &pointer, POINTER_BUTTON).is_empty());
    transport.handle(&mut f.state, size, HostMessage::ResetInput { source: 2 }).unwrap();
    let events = f.events();
    let buttons = sent(&events, &pointer, POINTER_BUTTON);
    assert_eq!(buttons.len(), 1);
    assert_eq!(buttons[0][3], 0);
}

#[test]
fn remote_clipboard_replacement_cancels_blocked_transfers_and_preserves_latest_on_reset() {
    use std::io::Read;
    use std::os::fd::OwnedFd;
    use tinyland_remote::{HostMessage, Size};
    use wayland_server::protocol::wl_data_offer;

    let mut f = Fixture::new();
    let size = Size::new(2, 1).unwrap();
    let mut transport = super::super::remote::Transport::test_transport();
    transport.handle(&mut f.state, size, HostMessage::Clipboard { source: 1, text: "a".repeat(1024 * 1024) }).unwrap();
    let offer = f.client.create_resource::<wl_data_offer::WlDataOffer, (), State>(&f.state.dh, 3, ()).unwrap();
    let (mut reader, writer) = std::io::pipe().unwrap();
    f.state.selection.receive_host(offer.id(), OwnedFd::from(writer), f.state.selection.host.as_ref().unwrap().clone());
    f.state.selection.service_transfers();
    assert_eq!(f.state.selection.transfers.len(), 1);
    transport.handle(&mut f.state, size, HostMessage::Clipboard { source: 2, text: "new".into() }).unwrap();
    assert!(f.state.selection.transfers.is_empty());
    let mut received = Vec::new();
    reader.read_to_end(&mut received).unwrap();
    assert!(!received.is_empty() && received.len() < 1024 * 1024);
    transport.handle(&mut f.state, size, HostMessage::ResetInput { source: 2 }).unwrap();
    assert_eq!(&**f.state.selection.host.as_ref().unwrap(), b"new");
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    f.state.selection.receive_host(offer.id(), OwnedFd::from(writer), f.state.selection.host.as_ref().unwrap().clone());
    f.state.selection.service_transfers();
    assert!(f.state.selection.transfers.is_empty());
    let mut readers = Vec::new();
    for _ in 0..17 {
        let (reader, writer) = std::io::pipe().unwrap();
        readers.push(reader);
        f.state.selection.receive_host(offer.id(), OwnedFd::from(writer), Arc::from(vec![b'x'; 1024 * 1024]));
    }
    assert_eq!(f.state.selection.transfers.len(), 16);
    f.state.selection.service_transfers();
    f.state.selection.transfers[0].deadline = std::time::Instant::now();
    f.state.selection.service_transfers();
    assert_eq!(f.state.selection.transfers.len(), 15);
    f.state.selection.remove_offer(&offer.id());
    assert!(f.state.selection.transfers.is_empty());

    let (mut reader, writer) = std::io::pipe().unwrap();
    let original = vec![b'z'; 1024 * 1024];
    f.state.selection.receive_host(offer.id(), OwnedFd::from(writer), Arc::from(original.clone()));
    let mut received = Vec::new();
    while !f.state.selection.transfers.is_empty() {
        f.state.selection.service_transfers();
        let mut chunk = [0; 8192];
        let count = reader.read(&mut chunk).unwrap();
        received.extend_from_slice(&chunk[..count]);
    }
    reader.read_to_end(&mut received).unwrap();
    assert_eq!(received, original);

    let (mut reader, writer) = std::io::pipe().unwrap();
    f.state.selection.receive_host(offer.id(), OwnedFd::from(writer), Arc::from(&b"pending"[..]));
    drop(f);
    let mut received = Vec::new();
    reader.read_to_end(&mut received).unwrap();
    assert!(received.is_empty());
}

#[test]
fn remote_and_client_clipboards_replace_each_other_without_reusing_host_offers() {
    use tinyland_remote::{HostMessage, Size};
    use wayland_server::protocol::{wl_data_device, wl_data_source};
    let mut f = Fixture::new();
    let surface = f.surface();
    f.toplevel(&surface);
    let top = f.state.toplevels.iter().find(|(_, top)| top.surface == surface.id()).unwrap().0.clone();
    f.state.toplevels.get_mut(&top).unwrap().mapped = true;
    f.state.set_active(top);
    let device = f.client.create_resource::<wl_data_device::WlDataDevice, (), State>(&f.state.dh, 3, ()).unwrap();
    f.state.selection.devices.insert(device.id(), device.clone());
    let source = f.client.create_resource::<wl_data_source::WlDataSource, (), State>(&f.state.dh, 3, ()).unwrap();
    f.state.selection.sources.insert(source.id(), crate::selection::DataSourceData { resource: source.clone(), mime_types: vec!["text/plain".into()] });
    let mut transport = super::super::remote::Transport::test_transport();
    let size = Size::new(2, 1).unwrap();
    transport.handle(&mut f.state, size, HostMessage::Clipboard { source: 1, text: "first".into() }).unwrap();
    let old_offer = f.state.selection.offers.keys().next().unwrap().clone();
    f.request(&device, 1, &[source.id().protocol_id(), 0]);
    assert!(f.state.selection.host.is_none());
    assert_eq!(f.state.selection.selection, Some(source.id()));
    assert!(!f.state.selection.offers.contains_key(&old_offer));
    transport.handle(&mut f.state, size, HostMessage::Clipboard { source: 2, text: "second".into() }).unwrap();
    let events = f.events();
    assert!(events.iter().any(|(id, opcode, _)| *id == source.id().protocol_id() && *opcode == 2));
    assert_eq!(&**f.state.selection.host.as_ref().unwrap(), b"second");
    assert!(!f.state.selection.offers.contains_key(&old_offer));
}
