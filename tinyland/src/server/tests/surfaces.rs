use super::*;
use std::collections::HashMap;
use wayland_server::protocol::wl_subcompositor;

struct Peer {
    client: Client,
    socket: UnixStream,
}

type Ledgers = (usize, HashMap<ClientId, usize>);

fn ledgers(state: &State) -> Ledgers {
    (state.total_bytes, state.client_bytes.clone())
}

fn wire(display: &mut Display<State>, state: &mut State, socket: &mut UnixStream, id: u32, opcode: u16, args: &[u32]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend(id.to_ne_bytes());
    bytes.extend((((8 + args.len() * 4) as u32) << 16 | opcode as u32).to_ne_bytes());
    for arg in args {
        bytes.extend(arg.to_ne_bytes());
    }
    socket.write_all(&bytes).unwrap();
    display.dispatch_clients(state).unwrap();
    display.flush_clients().unwrap();
    drain_socket(socket)
}

impl Fixture {
    fn peer(&mut self) -> Peer {
        let (socket, server) = UnixStream::pair().unwrap();
        socket.set_nonblocking(true).unwrap();
        let client = self.state.dh.insert_client(server, Arc::new(UdsClientData)).unwrap();
        Peer { client, socket }
    }

    fn peer_surface(&mut self, peer: &Peer) -> wl_surface::WlSurface {
        let surface = peer.client.create_resource::<wl_surface::WlSurface, (), State>(&self.state.dh, 4, ()).unwrap();
        self.state.surfaces.insert(surface.id(), SurfaceData::new(surface.clone()));
        surface
    }

    fn peer_buffer(&mut self, peer: &Peer, width: i32, height: i32, fill: u8) -> wl_buffer::WlBuffer {
        let bytes = vec![fill; width as usize * height as usize * 4];
        let file = tempfile::tempfile().unwrap();
        file.set_len(bytes.len() as u64).unwrap();
        std::os::unix::fs::FileExt::write_all_at(&file, &bytes, 0).unwrap();
        let buffer = peer.client.create_resource::<wl_buffer::WlBuffer, (), State>(&self.state.dh, 1, ()).unwrap();
        self.state.buffers.insert(buffer.id(), (buffer.clone(), ShmBufferData {
            file: Arc::new(file), offset: 0, width, height, stride: width * 4, format: ShmFormat::Xrgb8888,
        }));
        buffer
    }

    fn peer_request(&mut self, peer: &mut Peer, object: &impl Resource, opcode: u16, args: &[u32]) -> Vec<u8> {
        wire(&mut self.display, &mut self.state, &mut peer.socket, object.id().protocol_id(), opcode, args)
    }

    fn peer_commit(&mut self, peer: &mut Peer, surface: &wl_surface::WlSurface, fill: u8) {
        let buffer = self.peer_buffer(peer, 2, 1, fill);
        self.peer_request(peer, surface, 1, &[buffer.id().protocol_id(), 0, 0]);
        self.peer_request(peer, surface, 6, &[]);
    }
}

struct Painted {
    parent: wl_surface::WlSurface,
    child: wl_surface::WlSurface,
}

const PAINTED_BYTES: usize = 3 * 8;

fn paint(f: &mut Fixture, peer: &mut Peer) -> Painted {
    let parent = f.peer_surface(peer);
    let child = f.peer_surface(peer);
    let subcompositor = peer.client.create_resource::<wl_subcompositor::WlSubcompositor, (), State>(&f.state.dh, 1, ()).unwrap();
    f.peer_request(peer, &subcompositor, 1, &[2, child.id().protocol_id(), parent.id().protocol_id()]);
    f.peer_commit(peer, &parent, 1);
    f.peer_commit(peer, &child, 2);
    f.peer_request(peer, &parent, 6, &[]);
    f.peer_commit(peer, &child, 3);
    let held = &f.state.surfaces[&child.id()];
    assert!(held.pixels.is_some() && held.cached.as_ref().is_some_and(|cached| cached.pixels.is_some()));
    Painted { parent, child }
}

fn healthy_commit_succeeds(f: &mut Fixture, surface: &wl_surface::WlSurface, fill: u8) {
    let events = f.show(surface, 2, 1, fill);
    assert!(display_errors(&events).is_empty(), "{events:?}");
    assert_eq!(f.state.surfaces[&surface.id()].pixels.as_ref().unwrap().bytes[0], fill);
}

#[test]
fn explicit_destruction_refunds_current_and_cached_pixels_and_leaves_other_clients_alone() {
    let mut f = Fixture::new();
    let healthy = f.surface();
    f.show(&healthy, 2, 1, 5);
    let before = ledgers(&f.state);
    assert_eq!(before.0, 8);
    let mut peer = f.peer();
    let painted = paint(&mut f, &mut peer);
    assert_eq!(f.state.total_bytes, before.0 + PAINTED_BYTES);
    assert_eq!(f.state.client_bytes[&peer.client.id()], PAINTED_BYTES);
    let subsurface_id = f.state.subsurfaces.keys().next().unwrap().clone();
    let subsurface = wayland_server::protocol::wl_subsurface::WlSubsurface::from_id(&f.state.dh, subsurface_id).unwrap();
    let mut errors = Vec::new();
    errors.extend(display_errors(&messages(&f.peer_request(&mut peer, &subsurface, 0, &[]))));
    errors.extend(display_errors(&messages(&f.peer_request(&mut peer, &painted.child, 0, &[]))));
    errors.extend(display_errors(&messages(&f.peer_request(&mut peer, &painted.parent, 0, &[]))));
    assert!(errors.is_empty(), "{errors:?}");
    assert!(peer.client.id() != f.client.id());
    assert_eq!(ledgers(&f.state), before);
    healthy_commit_succeeds(&mut f, &healthy, 6);
    assert_eq!(f.state.total_bytes, before.0);
}

#[test]
fn explicit_surface_destruction_without_subsurface_object_refunds_exactly_once() {
    let mut f = Fixture::new();
    let mut peer = f.peer();
    let before = ledgers(&f.state);
    let painted = paint(&mut f, &mut peer);
    f.peer_request(&mut peer, &painted.child, 0, &[]);
    f.peer_request(&mut peer, &painted.parent, 0, &[]);
    assert_eq!(ledgers(&f.state), before);
}

#[test]
fn disconnecting_a_painted_client_refunds_current_and_cached_pixels() {
    let mut f = Fixture::new();
    let healthy = f.surface();
    f.show(&healthy, 2, 1, 5);
    let before = ledgers(&f.state);
    let mut peer = f.peer();
    let painted = paint(&mut f, &mut peer);
    let peer_id = peer.client.id();
    assert_eq!(f.state.total_bytes, before.0 + PAINTED_BYTES);
    assert_eq!(f.state.client_bytes[&peer_id], PAINTED_BYTES);
    drop(painted);
    drop(peer.socket);
    f.display.dispatch_clients(&mut f.state).unwrap();
    f.display.flush_clients().unwrap();
    assert!(f.state.surfaces.keys().all(|id| f.state.dh.get_client(id.clone()).is_ok_and(|client| client.id() != peer_id)));
    assert_eq!(f.state.surfaces.len(), 1);
    assert_eq!(f.state.total_bytes, before.0, "global ledger must return to its prior value");
    assert_eq!(f.state.client_bytes, before.1, "disconnected client's ledger must be gone");
    healthy_commit_succeeds(&mut f, &healthy, 6);
    assert_eq!(f.state.total_bytes, before.0);
}

#[test]
fn repeated_painted_disconnects_do_not_exhaust_the_global_budget() {
    let mut f = Fixture::new();
    let healthy = f.surface();
    f.show(&healthy, 2, 1, 5);
    let before = ledgers(&f.state);
    for _ in 0..16 {
        let mut peer = f.peer();
        let painted = paint(&mut f, &mut peer);
        drop(painted);
        drop(peer.socket);
        f.display.dispatch_clients(&mut f.state).unwrap();
        f.display.flush_clients().unwrap();
    }
    assert_eq!(ledgers(&f.state), before);
    healthy_commit_succeeds(&mut f, &healthy, 7);
}

use wayland_server::protocol::wl_pointer::WlPointer;

fn decoded(args: &[u32], at: usize) -> f64 {
    f64::from(args[at] as i32) / 256.0
}

fn assert_close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 0.01, "{actual} != {expected}");
}

fn assert_entered(events: &Events, pointer: &WlPointer, surface: &wl_surface::WlSurface, x: f64, y: f64) {
    let enters = sent(events, pointer, POINTER_ENTER);
    assert_eq!(enters.len(), 1, "{events:?}");
    assert_eq!(enters[0][1], surface.id().protocol_id(), "entered the wrong surface: {events:?}");
    assert_close(decoded(enters[0], 2), x);
    assert_close(decoded(enters[0], 3), y);
}

fn assert_left(events: &Events, pointer: &WlPointer, surface: &wl_surface::WlSurface) {
    let leaves = sent(events, pointer, POINTER_LEAVE);
    assert_eq!(leaves.len(), 1, "{events:?}");
    assert_eq!(leaves[0][1], surface.id().protocol_id());
}

fn assert_no_focus_change(events: &Events, pointer: &WlPointer) {
    assert!(sent(events, pointer, POINTER_ENTER).is_empty(), "{events:?}");
    assert!(sent(events, pointer, POINTER_LEAVE).is_empty(), "{events:?}");
}

#[derive(Clone)]
struct Ids(std::rc::Rc<std::cell::Cell<u32>>);

impl Ids {
    fn new() -> Self {
        Ids(std::rc::Rc::new(std::cell::Cell::new(2)))
    }

    fn next(&self) -> u32 {
        let id = self.0.get();
        self.0.set(id + 1);
        id
    }
}

struct Tree {
    subcompositor: wl_subcompositor::WlSubcompositor,
    ids: Ids,
}

impl Tree {
    fn new(f: &mut Fixture, ids: &Ids) -> Self {
        let subcompositor = f.client.create_resource::<wl_subcompositor::WlSubcompositor, (), State>(&f.state.dh, 1, ()).unwrap();
        Tree { subcompositor, ids: ids.clone() }
    }

    fn attach(&mut self, f: &mut Fixture, parent: &wl_surface::WlSurface, x: i32, y: i32) -> (wl_surface::WlSurface, u32) {
        let child = f.surface();
        let id = self.ids.next();
        f.request(&self.subcompositor, 1, &[id, child.id().protocol_id(), parent.id().protocol_id()]);
        f.request_id(id, 1, &[x as u32, y as u32]);
        (child, id)
    }
}

fn small_toplevel(f: &mut Fixture, width: i32, height: i32, fill: u8) -> wl_surface::WlSurface {
    let surface = f.surface();
    let xdg = f.toplevel(&surface);
    f.request(&xdg, 4, &[7]);
    f.show(&surface, width, height, fill);
    surface
}

fn pixel(f: &Fixture, x: usize, y: usize) -> u8 {
    f.state.compose()[(y * f.state.out_size.0 as usize + x) * 4]
}

#[test]
fn child_over_its_parent_receives_the_pointer_with_child_local_coordinates() {
    let mut f = Fixture::with_output((100, 60));
    let pointer = f.pointer_seat();
    let parent = small_toplevel(&mut f, 40, 30, 10);
    let ids = Ids::new();
    let mut tree = Tree::new(&mut f, &ids);
    let (child, _) = tree.attach(&mut f, &parent, 10, 10);
    f.show(&child, 20, 20, 20);
    f.move_pointer(15.0, 15.0);
    let events = f.events();
    assert_entered(&events, &pointer, &parent, 15.0, 15.0);
    assert_eq!(pixel(&f, 15, 15), 10, "an uncommitted child is not presented");

    let events = messages(&f.request(&parent, 6, &[]));
    assert_left(&events, &pointer, &parent);
    assert_entered(&events, &pointer, &child, 5.0, 5.0);
    assert_eq!(pixel(&f, 15, 15), 20, "the entered surface is the one that is drawn there");

    f.move_pointer(35.0, 25.0);
    let events = f.events();
    assert_left(&events, &pointer, &child);
    assert_entered(&events, &pointer, &parent, 35.0, 25.0);
    assert_eq!(pixel(&f, 35, 25), 10);
}

#[test]
fn implicit_grab_keeps_a_child_focused_with_child_local_coordinates() {
    let mut f = Fixture::with_output((100, 60));
    let pointer = f.pointer_seat();
    let parent = small_toplevel(&mut f, 40, 30, 10);
    let ids = Ids::new();
    let mut tree = Tree::new(&mut f, &ids);
    let (child, _) = tree.attach(&mut f, &parent, 10, 10);
    f.show(&child, 20, 20, 20);
    f.request(&parent, 6, &[]);
    f.move_pointer(15.0, 15.0);
    f.events();
    f.press(true);
    f.move_pointer(35.0, 25.0);
    let events = f.events();
    assert_no_focus_change(&events, &pointer);
    let motion = sent(&events, &pointer, POINTER_MOTION);
    assert_eq!(motion.len(), 1);
    assert_close(decoded(motion[0], 1), 25.0);
    assert_close(decoded(motion[0], 2), 15.0);
    f.press(false);
    let events = f.events();
    assert_left(&events, &pointer, &child);
    assert_entered(&events, &pointer, &parent, 35.0, 25.0);
}

#[test]
fn child_outside_its_parent_is_a_pointer_target() {
    let mut f = Fixture::with_output((100, 60));
    let pointer = f.pointer_seat();
    let parent = small_toplevel(&mut f, 40, 30, 10);
    let ids = Ids::new();
    let mut tree = Tree::new(&mut f, &ids);
    let (child, _) = tree.attach(&mut f, &parent, 50, 10);
    f.show(&child, 20, 20, 20);
    f.request(&parent, 6, &[]);
    f.move_pointer(55.0, 15.0);
    let events = f.events();
    assert_entered(&events, &pointer, &child, 5.0, 5.0);
    assert_eq!(pixel(&f, 55, 15), 20);
    f.move_pointer(80.0, 15.0);
    let events = f.events();
    assert_left(&events, &pointer, &child);
    assert!(sent(&events, &pointer, POINTER_ENTER).is_empty());
}

#[test]
fn nested_child_accumulates_every_ancestor_origin() {
    let mut f = Fixture::with_output((100, 60));
    let pointer = f.pointer_seat();
    let parent = small_toplevel(&mut f, 40, 30, 10);
    let ids = Ids::new();
    let mut tree = Tree::new(&mut f, &ids);
    let (child, _) = tree.attach(&mut f, &parent, 50, 10);
    let (grandchild, _) = tree.attach(&mut f, &child, 3, 4);
    f.show(&grandchild, 10, 10, 30);
    f.show(&child, 20, 20, 20);
    f.request(&parent, 6, &[]);
    f.move_pointer(56.0, 16.0);
    let events = f.events();
    assert_entered(&events, &pointer, &grandchild, 3.0, 2.0);
    assert_eq!(pixel(&f, 56, 16), 30);
    f.move_pointer(52.0, 12.0);
    let events = f.events();
    assert_left(&events, &pointer, &grandchild);
    assert_entered(&events, &pointer, &child, 2.0, 2.0);
}

#[test]
fn overlapping_siblings_follow_presentation_order_and_restacking() {
    let mut f = Fixture::with_output((100, 60));
    let pointer = f.pointer_seat();
    let parent = small_toplevel(&mut f, 40, 30, 10);
    let ids = Ids::new();
    let mut tree = Tree::new(&mut f, &ids);
    let (lower, _) = tree.attach(&mut f, &parent, 10, 10);
    let (upper, upper_wire) = tree.attach(&mut f, &parent, 20, 10);
    f.show(&lower, 20, 20, 20);
    f.show(&upper, 20, 20, 30);
    f.request(&parent, 6, &[]);
    f.move_pointer(25.0, 15.0);
    let events = f.events();
    assert_entered(&events, &pointer, &upper, 5.0, 5.0);
    assert_eq!(pixel(&f, 25, 15), 30);

    f.request_id(upper_wire, 3, &[lower.id().protocol_id()]);
    let events = messages(&f.request(&parent, 6, &[]));
    assert_left(&events, &pointer, &upper);
    assert_entered(&events, &pointer, &lower, 15.0, 5.0);
    assert_eq!(pixel(&f, 25, 15), 20);
}

use wayland_protocols::xdg::shell::server::xdg_positioner::{Anchor, ConstraintAdjustment, Gravity};
use wayland_protocols::xdg::shell::server::{xdg_surface, xdg_wm_base};
use wayland_server::protocol::wl_compositor;

const SET_INPUT_REGION: u16 = 5;
const COMMIT: u16 = 6;
const REGION_ADD: u16 = 1;
const REGION_SUBTRACT: u16 = 2;
const REGION_DESTROY: u16 = 0;
const SUBSURFACE_SET_DESYNC: u16 = 5;

struct Regions {
    compositor: wl_compositor::WlCompositor,
    ids: Ids,
}

impl Regions {
    fn new(f: &mut Fixture, ids: &Ids) -> Self {
        Regions { compositor: f.client.create_resource::<wl_compositor::WlCompositor, (), State>(&f.state.dh, 4, ()).unwrap(), ids: ids.clone() }
    }

    fn create(&self, f: &mut Fixture) -> u32 {
        let id = self.ids.next();
        f.request(&self.compositor, 1, &[id]);
        id
    }

    fn add(&self, f: &mut Fixture, id: u32, rect: (i32, i32, i32, i32)) {
        f.request_id(id, REGION_ADD, &[rect.0 as u32, rect.1 as u32, rect.2 as u32, rect.3 as u32]);
    }

    fn subtract(&self, f: &mut Fixture, id: u32, rect: (i32, i32, i32, i32)) {
        f.request_id(id, REGION_SUBTRACT, &[rect.0 as u32, rect.1 as u32, rect.2 as u32, rect.3 as u32]);
    }
}

fn assign(f: &mut Fixture, surface: &wl_surface::WlSurface, region: u32) {
    f.request(surface, SET_INPUT_REGION, &[region]);
}

fn commit(f: &mut Fixture, surface: &wl_surface::WlSurface) -> Events {
    messages(&f.request(surface, COMMIT, &[]))
}

struct Covered {
    pointer: WlPointer,
    parent: wl_surface::WlSurface,
    child: wl_surface::WlSurface,
    child_wire: u32,
    regions: Regions,
}

fn covered() -> (Fixture, Covered) {
    let mut f = Fixture::with_output((100, 60));
    let pointer = f.pointer_seat();
    let parent = small_toplevel(&mut f, 40, 30, 10);
    let ids = Ids::new();
    let mut tree = Tree::new(&mut f, &ids);
    let (child, child_wire) = tree.attach(&mut f, &parent, 10, 10);
    f.show(&child, 20, 20, 20);
    f.request(&parent, COMMIT, &[]);
    let regions = Regions::new(&mut f, &ids);
    (f, Covered { pointer, parent, child, child_wire, regions })
}

fn hover(f: &mut Fixture, x: f64, y: f64) -> Events {
    f.move_pointer(x, y);
    f.events()
}

#[test]
fn empty_input_region_lets_clicks_fall_through_to_the_surface_below() {
    let (mut f, c) = covered();
    let events = hover(&mut f, 15.0, 15.0);
    assert_entered(&events, &c.pointer, &c.child, 5.0, 5.0);
    let region = c.regions.create(&mut f);
    assign(&mut f, &c.child, region);
    assert_eq!(f.state.pointer.focus, Some(c.child.id()), "assignment alone changes nothing before commit");
    commit(&mut f, &c.child);
    assert_eq!(f.state.pointer.focus, Some(c.child.id()), "synchronized child waits for its parent");
    let events = commit(&mut f, &c.parent);
    assert_left(&events, &c.pointer, &c.child);
    assert_entered(&events, &c.pointer, &c.parent, 15.0, 15.0);
    assert_eq!(pixel(&f, 15, 15), 20, "an excluded surface is still drawn");
    f.press(true);
    assert_eq!(f.state.pointer.focus, Some(c.parent.id()), "the click lands on the surface below");
    f.press(false);
}

#[test]
fn null_region_restores_infinite_input_while_an_empty_one_means_none() {
    let (mut f, c) = covered();
    let region = c.regions.create(&mut f);
    assign(&mut f, &c.child, region);
    f.request_id(c.child_wire, SUBSURFACE_SET_DESYNC, &[]);
    commit(&mut f, &c.child);
    let events = hover(&mut f, 15.0, 15.0);
    assert_entered(&events, &c.pointer, &c.parent, 15.0, 15.0);
    assign(&mut f, &c.child, 0);
    let events = commit(&mut f, &c.child);
    assert_left(&events, &c.pointer, &c.parent);
    assert_entered(&events, &c.pointer, &c.child, 5.0, 5.0);
}

#[test]
fn subtracted_hole_passes_through_while_the_rest_of_the_surface_keeps_input() {
    let (mut f, c) = covered();
    f.request_id(c.child_wire, SUBSURFACE_SET_DESYNC, &[]);
    let region = c.regions.create(&mut f);
    c.regions.add(&mut f, region, (0, 0, 20, 20));
    c.regions.subtract(&mut f, region, (5, 5, 10, 10));
    assign(&mut f, &c.child, region);
    commit(&mut f, &c.child);
    let events = hover(&mut f, 12.0, 12.0);
    assert_entered(&events, &c.pointer, &c.child, 2.0, 2.0);
    let events = hover(&mut f, 20.0, 20.0);
    assert_left(&events, &c.pointer, &c.child);
    assert_entered(&events, &c.pointer, &c.parent, 20.0, 20.0);
    let events = hover(&mut f, 28.0, 28.0);
    assert_left(&events, &c.pointer, &c.parent);
    assert_entered(&events, &c.pointer, &c.child, 18.0, 18.0);
}

#[test]
fn later_add_covers_an_earlier_subtraction() {
    let (mut f, c) = covered();
    f.request_id(c.child_wire, SUBSURFACE_SET_DESYNC, &[]);
    let region = c.regions.create(&mut f);
    c.regions.add(&mut f, region, (0, 0, 20, 20));
    c.regions.subtract(&mut f, region, (5, 5, 10, 10));
    c.regions.add(&mut f, region, (8, 8, 2, 2));
    assign(&mut f, &c.child, region);
    commit(&mut f, &c.child);
    let events = hover(&mut f, 19.0, 19.0);
    assert_entered(&events, &c.pointer, &c.child, 9.0, 9.0);
    let events = hover(&mut f, 24.0, 24.0);
    assert_left(&events, &c.pointer, &c.child);
    assert_entered(&events, &c.pointer, &c.parent, 24.0, 24.0);
}

#[test]
fn assigned_region_is_a_copy_that_survives_the_source_and_later_edits() {
    let (mut f, c) = covered();
    f.request_id(c.child_wire, SUBSURFACE_SET_DESYNC, &[]);
    let region = c.regions.create(&mut f);
    c.regions.add(&mut f, region, (0, 0, 5, 5));
    assign(&mut f, &c.child, region);
    c.regions.add(&mut f, region, (0, 0, 20, 20));
    commit(&mut f, &c.child);
    let events = hover(&mut f, 12.0, 12.0);
    assert_entered(&events, &c.pointer, &c.child, 2.0, 2.0);
    let events = hover(&mut f, 25.0, 25.0);
    assert_left(&events, &c.pointer, &c.child);
    assert_entered(&events, &c.pointer, &c.parent, 25.0, 25.0);

    let second = c.regions.create(&mut f);
    c.regions.add(&mut f, second, (10, 10, 10, 10));
    assign(&mut f, &c.child, second);
    f.request_id(second, REGION_DESTROY, &[]);
    assert_eq!(f.state.regions.len(), 1, "the destroyed source region is gone");
    let events = commit(&mut f, &c.child);
    assert_left(&events, &c.pointer, &c.parent);
    assert_entered(&events, &c.pointer, &c.child, 15.0, 15.0);
    let events = hover(&mut f, 12.0, 12.0);
    assert_left(&events, &c.pointer, &c.child);
    assert_entered(&events, &c.pointer, &c.parent, 12.0, 12.0);
}

#[test]
fn input_region_follows_the_commit_boundary_of_synchronized_and_desynchronized_surfaces() {
    let (mut f, c) = covered();
    let events = hover(&mut f, 15.0, 15.0);
    assert_entered(&events, &c.pointer, &c.child, 5.0, 5.0);
    let region = c.regions.create(&mut f);
    assign(&mut f, &c.child, region);
    commit(&mut f, &c.child);
    assert_eq!(f.state.pointer.focus, Some(c.child.id()));
    let events = commit(&mut f, &c.parent);
    assert_left(&events, &c.pointer, &c.child);
    assert_entered(&events, &c.pointer, &c.parent, 15.0, 15.0);

    assign(&mut f, &c.child, 0);
    commit(&mut f, &c.child);
    assert_eq!(f.state.pointer.focus, Some(c.parent.id()), "a synchronized null region waits too");
    let events = messages(&f.request_id(c.child_wire, SUBSURFACE_SET_DESYNC, &[]));
    assert_left(&events, &c.pointer, &c.parent);
    assert_entered(&events, &c.pointer, &c.child, 5.0, 5.0);

    assign(&mut f, &c.child, region);
    assert_eq!(f.state.pointer.focus, Some(c.child.id()), "uncommitted requests are not applied");
    let events = commit(&mut f, &c.child);
    assert_left(&events, &c.pointer, &c.child);
    assert_entered(&events, &c.pointer, &c.parent, 15.0, 15.0);
}

const XDG_SURFACE_CONFIGURE: u16 = 0;
const WM_BASE_INVALID_POSITIONER: u32 = 5;

#[derive(Clone, Copy)]
struct PopupSpec {
    size: (i32, i32),
    anchor_rect: (i32, i32, i32, i32),
    anchor: Anchor,
    gravity: Gravity,
    adjust: ConstraintAdjustment,
    offset: (i32, i32),
}

impl PopupSpec {
    fn at(x: i32, y: i32, width: i32, height: i32) -> Self {
        PopupSpec {
            size: (width, height),
            anchor_rect: (x, y, 1, 1),
            anchor: Anchor::TopLeft,
            gravity: Gravity::BottomRight,
            adjust: ConstraintAdjustment::empty(),
            offset: (0, 0),
        }
    }
}

struct Popup {
    surface: wl_surface::WlSurface,
    xdg: u32,
    popup: u32,
    events: Events,
}

impl Popup {
    fn configure(&self) -> Option<[i32; 4]> {
        self.events
            .iter()
            .find(|(object, opcode, _)| *object == self.popup && *opcode == POPUP_CONFIGURE)
            .map(|(_, _, args)| [args[0] as i32, args[1] as i32, args[2] as i32, args[3] as i32])
    }

    fn errors(&self) -> Vec<(u32, u32)> {
        display_errors(&self.events)
    }
}

fn open_popup(f: &mut Fixture, base: &xdg_wm_base::XdgWmBase, ids: &Ids, parent_xdg: u32, spec: PopupSpec) -> Popup {
    let positioner = ids.next();
    let xdg = ids.next();
    let popup = ids.next();
    f.request(base, 1, &[positioner]);
    f.request_id(positioner, 1, &[spec.size.0 as u32, spec.size.1 as u32]);
    let rect = spec.anchor_rect;
    f.request_id(positioner, 2, &[rect.0 as u32, rect.1 as u32, rect.2 as u32, rect.3 as u32]);
    f.request_id(positioner, 3, &[spec.anchor as u32]);
    f.request_id(positioner, 4, &[spec.gravity as u32]);
    f.request_id(positioner, 5, &[spec.adjust.bits()]);
    f.request_id(positioner, 6, &[spec.offset.0 as u32, spec.offset.1 as u32]);
    let surface = f.surface();
    f.request(base, 2, &[xdg, surface.id().protocol_id()]);
    let mut events = messages(&f.request_id(xdg, 2, &[popup, parent_xdg, positioner]));
    events.extend(messages(&f.request(&surface, COMMIT, &[])));
    Popup { surface, xdg, popup, events }
}

fn map_popup(f: &mut Fixture, popup: &Popup, width: i32, height: i32, fill: u8) {
    let serial = popup.events.iter().find(|(object, opcode, _)| *object == popup.xdg && *opcode == XDG_SURFACE_CONFIGURE).unwrap().2[0];
    f.request_id(popup.xdg, 4, &[serial]);
    f.show(&popup.surface, width, height, fill);
    assert!(f.state.popups.iter().any(|candidate| candidate.surface == popup.surface.id() && candidate.live()));
}

fn base(f: &mut Fixture) -> xdg_wm_base::XdgWmBase {
    f.client.create_resource::<xdg_wm_base::XdgWmBase, (), State>(&f.state.dh, 1, ()).unwrap()
}

#[test]
fn popup_with_a_restricted_input_region_passes_excluded_pixels_to_the_surface_below() {
    let mut f = Fixture::with_output((100, 60));
    let pointer = f.pointer_seat();
    let (parent, parent_xdg, _) = mapped_toplevel(&mut f, 10);
    let ids = Ids::new();
    let base = base(&mut f);
    let regions = Regions::new(&mut f, &ids);
    let popup = open_popup(&mut f, &base, &ids, parent_xdg.id().protocol_id(), PopupSpec::at(10, 10, 30, 20));
    assert_eq!(popup.configure(), Some([10, 10, 30, 20]));
    let region = regions.create(&mut f);
    regions.add(&mut f, region, (0, 0, 15, 20));
    assign(&mut f, &popup.surface, region);
    f.request_id(region, REGION_DESTROY, &[]);
    map_popup(&mut f, &popup, 30, 20, 99);
    f.events();
    let events = hover(&mut f, 15.0, 15.0);
    assert_entered(&events, &pointer, &popup.surface, 5.0, 5.0);
    let events = hover(&mut f, 30.0, 15.0);
    assert_left(&events, &pointer, &popup.surface);
    assert_entered(&events, &pointer, &parent, 30.0, 15.0);
    assert_eq!(pixel(&f, 30, 15), 99, "the excluded pixels are still drawn");
}

#[test]
fn transparent_popup_with_an_empty_input_region_never_captures_the_pointer() {
    let mut f = Fixture::with_output((100, 60));
    let pointer = f.pointer_seat();
    let (parent, parent_xdg, _) = mapped_toplevel(&mut f, 10);
    let ids = Ids::new();
    let base = base(&mut f);
    let regions = Regions::new(&mut f, &ids);
    let popup = open_popup(&mut f, &base, &ids, parent_xdg.id().protocol_id(), PopupSpec::at(10, 10, 30, 20));
    map_popup(&mut f, &popup, 30, 20, 99);
    f.events();
    let events = hover(&mut f, 15.0, 15.0);
    assert_entered(&events, &pointer, &popup.surface, 5.0, 5.0);
    let region = regions.create(&mut f);
    assign(&mut f, &popup.surface, region);
    let events = commit(&mut f, &popup.surface);
    assert_left(&events, &pointer, &popup.surface);
    assert_entered(&events, &pointer, &parent, 15.0, 15.0);
    assert_eq!(pixel(&f, 15, 15), 99);
    f.press(true);
    assert_eq!(f.state.pointer.focus, Some(parent.id()));
    f.press(false);
}

fn overflow_fixture() -> (Fixture, xdg_surface::XdgSurface, xdg_wm_base::XdgWmBase, Ids) {
    let mut f = Fixture::with_output((100, 100));
    let (_, parent_xdg, _) = mapped_toplevel(&mut f, 10);
    let base = base(&mut f);
    (f, parent_xdg, base, Ids::new())
}

fn ticket_offset_spec() -> PopupSpec {
    PopupSpec {
        size: (8, 8),
        anchor_rect: (40, 40, 4, 4),
        anchor: Anchor::None,
        gravity: Gravity::None,
        adjust: ConstraintAdjustment::empty(),
        offset: (i32::MAX, 0),
    }
}

#[test]
fn unconstrained_placement_beyond_i32_is_reported_as_an_invalid_positioner() {
    let (mut f, parent_xdg, base, ids) = overflow_fixture();
    let popup = open_popup(&mut f, &base, &ids, parent_xdg.id().protocol_id(), ticket_offset_spec());
    assert_eq!(popup.errors(), [(base.id().protocol_id(), WM_BASE_INVALID_POSITIONER)], "{:?}", popup.events);
    assert_eq!(popup.configure(), None, "no wrapped coordinates reach the client");
}

#[test]
fn constraint_adjustment_rescues_a_wide_placement_before_conversion() {
    let (mut f, parent_xdg, base, ids) = overflow_fixture();
    let spec = PopupSpec { adjust: ConstraintAdjustment::SlideX, ..ticket_offset_spec() };
    let popup = open_popup(&mut f, &base, &ids, parent_xdg.id().protocol_id(), spec);
    assert!(popup.errors().is_empty(), "{:?}", popup.events);
    assert_eq!(popup.configure(), Some([92, 38, 8, 8]));
}

#[test]
fn flip_decision_uses_the_exact_negated_offset() {
    let (mut f, parent_xdg, base, ids) = overflow_fixture();
    let spec = PopupSpec {
        size: (2, 2),
        anchor_rect: (0, 0, 2, 2),
        anchor: Anchor::None,
        gravity: Gravity::None,
        adjust: ConstraintAdjustment::FlipX,
        offset: (i32::MIN, 0),
    };
    let popup = open_popup(&mut f, &base, &ids, parent_xdg.id().protocol_id(), spec);
    assert!(popup.errors().is_empty(), "{:?}", popup.events);
    assert_eq!(popup.configure(), Some([i32::MIN, 0, 2, 2]), "neither side fits, so the original side is kept");
}

#[test]
fn wrapped_positions_do_not_masquerade_as_fitting_and_suppress_a_flip() {
    let (mut f, parent_xdg, base, ids) = overflow_fixture();
    let spec = PopupSpec {
        size: (8, 8),
        anchor_rect: (i32::MIN, 0, 20, 10),
        anchor: Anchor::Left,
        gravity: Gravity::Right,
        adjust: ConstraintAdjustment::FlipX,
        offset: (i32::MIN, 0),
    };
    let popup = open_popup(&mut f, &base, &ids, parent_xdg.id().protocol_id(), spec);
    assert!(popup.errors().is_empty(), "{:?}", popup.events);
    assert_eq!(popup.configure(), Some([12, 1, 8, 8]), "unflipped x is -2^32, which only wraps to 0 in 32 bits");
}

#[test]
fn nested_popup_origins_are_summed_without_saturating() {
    let mut f = Fixture::with_output((100, 100));
    let pointer = f.pointer_seat();
    let (_, parent_xdg, _) = mapped_toplevel(&mut f, 10);
    let base = base(&mut f);
    let ids = Ids::new();
    let mut parent = parent_xdg.id().protocol_id();
    let mut last = None;
    for (x, fill) in [(2_000_000_000, 41), (2_000_000_000, 42), (-2_000_000_000, 43), (-2_000_000_000, 77)] {
        let popup = open_popup(&mut f, &base, &ids, parent, PopupSpec::at(x, 0, 20, 20));
        assert!(popup.errors().is_empty(), "{:?}", popup.events);
        assert_eq!(popup.configure(), Some([x, 0, 20, 20]));
        map_popup(&mut f, &popup, 20, 20, fill);
        parent = popup.xdg;
        last = Some(popup);
    }
    let last = last.unwrap();
    assert_eq!(f.state.surface_origin(&last.surface.id()), Some((0, 0)));
    assert_eq!(pixel(&f, 5, 5), 77);
    f.events();
    let events = hover(&mut f, 5.0, 5.0);
    assert_entered(&events, &pointer, &last.surface, 5.0, 5.0);
}

#[test]
fn extreme_cursor_hotspots_neither_panic_nor_draw() {
    let mut f = Fixture::with_output((8, 4));
    let pointer = f.pointer_seat();
    mapped_toplevel(&mut f, 10);
    f.move_pointer(5.0, 2.0);
    let cursor = f.surface();
    f.show(&cursor, 2, 2, 77);
    for hotspot in [(i32::MIN, i32::MIN), (i32::MAX, i32::MAX), (i32::MIN, i32::MAX), (i32::MAX, i32::MIN), (i32::MIN, 0), (0, i32::MIN)] {
        let serial = f.state.pointer.enter_serial;
        f.request(&pointer, 0, &[serial, cursor.id().protocol_id(), hotspot.0 as u32, hotspot.1 as u32]);
        assert!(f.state.pointer.cursor.is_some());
        let frame = f.state.compose();
        for row in 0..4 {
            for column in 0..8 {
                assert_eq!(frame[(row * 8 + column) * 4], 10, "cursor leaked onto the output at {column},{row} with hotspot {hotspot:?}");
            }
        }
    }
}
