use super::*;
use wayland_server::protocol::{wl_subcompositor, wl_subsurface};

const ATTACH: u16 = 1;
const COMMIT: u16 = 6;
const SET_BUFFER_SCALE: u16 = 8;
const DAMAGE_BUFFER: u16 = 9;
const SET_POSITION: u16 = 1;
const SET_DESYNC: u16 = 5;

fn pixel_buffer(f: &mut Fixture, width: i32, height: i32, bytes: &[u8], format: ShmFormat) -> wl_buffer::WlBuffer {
    let file = tempfile::tempfile().unwrap();
    file.set_len(bytes.len() as u64).unwrap();
    std::os::unix::fs::FileExt::write_all_at(&file, bytes, 0).unwrap();
    let buffer = f.client.create_resource::<wl_buffer::WlBuffer, (), State>(&f.state.dh, 1, ()).unwrap();
    f.state.buffers.insert(buffer.id(), (buffer.clone(), ShmBufferData { file: Arc::new(file), offset: 0, width, height, stride: width * 4, format }));
    buffer
}

fn commit(f: &mut Fixture, surface: &wl_surface::WlSurface, buffer: Option<&wl_buffer::WlBuffer>, damage: &[(i32, i32, i32, i32)]) {
    if let Some(buffer) = buffer { f.request(surface, ATTACH, &[buffer.id().protocol_id(), 0, 0]); }
    for &(x, y, w, h) in damage { f.request(surface, DAMAGE_BUFFER, &[x as u32, y as u32, w as u32, h as u32]); }
    f.request(surface, COMMIT, &[]);
}

/// Presents one frame and checks the persistent framebuffer against an independent full composition.
fn present(f: &mut Fixture) -> Vec<(u32, u32, u32, u32)> {
    let rects = f.state.submit_damage();
    f.state.complete_frame(1);
    assert!(f.state.framebuffer == f.state.compose(), "damage-tracked output differs from a full composition");
    rects
}

fn toplevel_with_pixels(f: &mut Fixture, width: i32, height: i32, pixels: &[u8]) -> wl_surface::WlSurface {
    let surface = f.surface();
    let xdg = f.toplevel(&surface);
    f.request(&xdg, 4, &[7]);
    let buffer = pixel_buffer(f, width, height, pixels, ShmFormat::Xrgb8888);
    commit(f, &surface, Some(&buffer), &[(0, 0, width, height)]);
    surface
}

struct Lcg(u32);
impl Lcg {
    fn next(&mut self) -> u32 { self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223); self.0 >> 8 }
    fn below(&mut self, n: u32) -> u32 { self.next() % n }
}

#[test]
fn a_small_damaged_commit_presents_only_its_rectangle() {
    let mut f = Fixture::with_output((64, 40));
    let mut model: Vec<u8> = (0..64 * 40 * 4).map(|n| (n % 251) as u8).collect();
    let surface = toplevel_with_pixels(&mut f, 64, 40, &model);
    assert_eq!(present(&mut f), vec![(0, 0, 64, 40)], "the first frame is whole");
    for y in 7..10 { for x in 10..15 { model[(y * 64 + x) * 4] ^= 0xff; } }
    let buffer = pixel_buffer(&mut f, 64, 40, &model, ShmFormat::Xrgb8888);
    commit(&mut f, &surface, Some(&buffer), &[(10, 7, 5, 3)]);
    assert_eq!(present(&mut f), vec![(10, 7, 15, 10)]);
    f.request(&surface, 3, &[2]);
    f.request(&surface, COMMIT, &[]);
    assert_eq!(present(&mut f), vec![], "a callback-only commit changes no pixels");
}

#[test]
fn pixels_outside_posted_damage_keep_their_previous_contents() {
    let mut f = Fixture::with_output((8, 4));
    let surface = toplevel_with_pixels(&mut f, 8, 4, &[10; 8 * 4 * 4]);
    present(&mut f);
    let buffer = pixel_buffer(&mut f, 8, 4, &[200; 8 * 4 * 4], ShmFormat::Xrgb8888);
    commit(&mut f, &surface, Some(&buffer), &[(1, 1, 2, 1)]);
    assert_eq!(present(&mut f), vec![(1, 1, 3, 2)]);
    let at = |x: usize, y: usize| f.state.framebuffer[(y * 8 + x) * 4];
    assert_eq!((at(1, 1), at(2, 1), at(0, 0), at(3, 1)), (200, 200, 10, 10));
}

#[test]
fn ignored_buffer_scale_makes_damage_whole() {
    let mut f = Fixture::with_output((16, 16));
    let surface = toplevel_with_pixels(&mut f, 16, 16, &[1; 16 * 16 * 4]);
    present(&mut f);
    f.request(&surface, SET_BUFFER_SCALE, &[2]);
    let buffer = pixel_buffer(&mut f, 16, 16, &[3; 16 * 16 * 4], ShmFormat::Xrgb8888);
    commit(&mut f, &surface, Some(&buffer), &[(0, 0, 1, 1)]);
    assert_eq!(present(&mut f), vec![(0, 0, 16, 16)]);
}

#[test]
fn random_damage_moves_and_resizes_always_match_a_full_composition() {
    let (out_w, out_h) = (96u32, 72u32);
    let mut f = Fixture::with_output((out_w, out_h));
    let mut rng = Lcg(5);
    let (pw, ph) = (80, 60);
    let mut parent_model: Vec<u8> = (0..pw * ph * 4).map(|_| rng.next() as u8).collect();
    let parent = toplevel_with_pixels(&mut f, pw, ph, &parent_model);
    let child = f.surface();
    let subcompositor = f.client.create_resource::<wl_subcompositor::WlSubcompositor, (), State>(&f.state.dh, 1, ()).unwrap();
    f.request(&subcompositor, 1, &[2, child.id().protocol_id(), parent.id().protocol_id()]);
    let id = f.state.subsurfaces.keys().next().unwrap().clone();
    let sub = wl_subsurface::WlSubsurface::from_id(&f.state.dh, id).unwrap();
    f.request(&sub, SET_DESYNC, &[]);
    let (mut cw, mut ch) = (20i32, 14i32);
    let mut child_model: Vec<u8> = (0..cw * ch * 4).map(|_| rng.next() as u8).collect();
    let buffer = pixel_buffer(&mut f, cw, ch, &child_model, ShmFormat::Argb8888);
    commit(&mut f, &child, Some(&buffer), &[(0, 0, cw, ch)]);
    present(&mut f);
    let mut sparse = 0;
    let mut next_id = 3;
    for step in 0..300 {
        match rng.below(6) {
            0 | 1 => {
                let (x, y, w, h) = (rng.below(pw as u32) as i32, rng.below(ph as u32) as i32, 1 + rng.below(30) as i32, 1 + rng.below(20) as i32);
                for yy in y..(y + h).min(ph) { for xx in x..(x + w).min(pw) { let at = ((yy * pw + xx) * 4) as usize; parent_model[at..at + 3].copy_from_slice(&[rng.next() as u8, step as u8, 7]); } }
                let buffer = pixel_buffer(&mut f, pw, ph, &parent_model, ShmFormat::Xrgb8888);
                commit(&mut f, &parent, Some(&buffer), &[(x, y, w, h), (x + w + 100, 0, 5, 5)]);
            }
            2 => {
                let (x, y) = (rng.below(cw as u32) as i32, rng.below(ch as u32) as i32);
                let at = ((y * cw + x) * 4) as usize;
                let alpha = [0, 128, 255][rng.below(3) as usize];
                child_model[at..at + 4].copy_from_slice(&[(rng.next() as u8).min(alpha), 0, (step as u8).min(alpha), alpha]);
                let buffer = pixel_buffer(&mut f, cw, ch, &child_model, ShmFormat::Argb8888);
                commit(&mut f, &child, Some(&buffer), &[(x, y, 1, 1)]);
            }
            3 => {
                f.request(&sub, SET_POSITION, &[rng.below(90), (rng.below(80) as i32 - 10) as u32]);
                f.request(&parent, COMMIT, &[]);
            }
            4 => {
                (cw, ch) = (1 + rng.below(40) as i32, 1 + rng.below(30) as i32);
                child_model = (0..cw * ch * 4).map(|_| rng.next() as u8).collect();
                let buffer = pixel_buffer(&mut f, cw, ch, &child_model, ShmFormat::Argb8888);
                commit(&mut f, &child, Some(&buffer), &[(0, 0, 1, 1)]);
            }
            _ => {
                f.request(&parent, 3, &[next_id]);
                next_id += 1;
                f.request(&parent, COMMIT, &[]);
            }
        }
        let rects = present(&mut f);
        sparse += usize::from(rects.iter().map(|r| (r.2 - r.0) * (r.3 - r.1)).sum::<u32>() < out_w * out_h / 2);
    }
    assert!(sparse > 150, "most random steps should present sparse damage, got {sparse}");
}
