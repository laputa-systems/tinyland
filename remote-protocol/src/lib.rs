use std::io::{self, Read, Write};

const MAGIC: [u8; 4] = *b"TLR1";
const MAX_CLIPBOARD: usize = 1024 * 1024;
/// The most rectangles one damage message may carry; a compositor with more sends their bounding box.
pub const MAX_DAMAGE_RECTS: usize = 64;
/// Edge of the tiles a tiled damage rectangle is cut into, from the rectangle's origin.
pub const DAMAGE_TILE: u32 = 64;
/// The most moves one damage message may carry.
pub const MAX_DAMAGE_MOVES: usize = 64;
const TILE_RAW: u8 = 0;
const TILE_SOLID: u8 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size { width: u32, height: u32 }

impl Size {
    pub fn new(width: u32, height: u32) -> Option<Self> {
        (width > 0 && height > 0 && width <= 4096 && height <= 4096
            && width.checked_mul(height).and_then(|n| n.checked_mul(4)).is_some())
            .then_some(Self { width, height })
    }
    pub fn width(self) -> u32 { self.width }
    pub fn height(self) -> u32 { self.height }
    pub fn pixel_len(self) -> usize { self.width as usize * self.height as usize * 4 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button { Left, Right, Middle, Back, Forward }

/// A nonempty rectangle of output pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect { pub x: u32, pub y: u32, pub width: u32, pub height: u32 }

impl Rect {
    fn inside(self, size: Size) -> bool {
        self.width > 0 && self.height > 0
            && self.x.checked_add(self.width).is_some_and(|end| end <= size.width)
            && self.y.checked_add(self.height).is_some_and(|end| end <= size.height)
    }
    fn pixel_len(self) -> usize { self.width as usize * self.height as usize * 4 }
}

/// Copies `height` rows of the columns `x..x + width` of the previous output from row `src_y` to row `dst_y`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Move { pub x: u32, pub width: u32, pub src_y: u32, pub dst_y: u32, pub height: u32 }

impl Move {
    fn inside(self, size: Size) -> bool {
        Rect { x: self.x, y: self.src_y, width: self.width, height: self.height }.inside(size)
            && Rect { x: self.x, y: self.dst_y, width: self.width, height: self.height }.inside(size)
    }

    /// The rectangle this move writes.
    pub fn destination(self) -> Rect { Rect { x: self.x, y: self.dst_y, width: self.width, height: self.height } }

    /// Applies the move to `pixels`, a whole `size` output of BGRX rows, copying rows in the order that keeps
    /// overlapping source rows intact.
    pub fn apply(self, size: Size, pixels: &mut [u8]) {
        let (stride, span) = (size.width as usize * 4, self.width as usize * 4);
        let row = |y: u32| y as usize * stride + self.x as usize * 4;
        let copy = |pixels: &mut [u8], i: u32| { let from = row(self.src_y + i); pixels.copy_within(from..from + span, row(self.dst_y + i)); };
        if self.dst_y < self.src_y { for i in 0..self.height { copy(pixels, i); } } else { for i in (0..self.height).rev() { copy(pixels, i); } }
    }
}

/// `Frame` replaces the whole output. `Damage` first applies its moves, in order, to the previous output, then
/// replaces its rectangles, each with its rows of BGRX pixels; with neither it still asks the host to acknowledge
/// a presentation. Moves read the output as their earlier moves left it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompositorMessage { Ready { size: Size }, Frame { serial: u64, pixels: Vec<u8> }, Damage { serial: u64, moves: Vec<Move>, rects: Vec<(Rect, Vec<u8>)> } }

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostMessage {
    Presented { serial: u64 },
    Key { source: u32, code: u16, pressed: bool },
    Pointer { source: u32, x: u32, y: u32 },
    Button { source: u32, button: Button, pressed: bool },
    Wheel { source: u32, horizontal: i32, vertical: i32 },
    ResetInput { source: u32 },
    Clipboard { source: u32, text: String },
}

fn invalid() -> io::Error { io::Error::new(io::ErrorKind::InvalidData, "invalid remote message") }
fn header<R: Read>(reader: &mut R) -> io::Result<Option<(u8, usize)>> {
    let mut h = [0u8; 9];
    loop {
        match reader.read(&mut h[..1]) {
            Ok(0) => return Ok(None),
            Ok(_) => break,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    reader.read_exact(&mut h[1..])?;
    if h[..4] != MAGIC { return Err(invalid()); }
    Ok(Some((h[4], u32::from_le_bytes(h[5..9].try_into().unwrap()) as usize)))
}
fn emit<W: Write>(writer: &mut W, tag: u8, body: &[u8]) -> io::Result<()> {
    let len = u32::try_from(body.len()).map_err(|_| invalid())?;
    writer.write_all(&MAGIC)?;
    writer.write_all(&[tag])?;
    writer.write_all(&len.to_le_bytes())?;
    writer.write_all(body)
}
fn u32_at(b: &[u8], n: usize) -> u32 { u32::from_le_bytes(b[n..n+4].try_into().unwrap()) }
fn u64_at(b: &[u8], n: usize) -> u64 { u64::from_le_bytes(b[n..n+8].try_into().unwrap()) }
fn boolean(b: u8) -> io::Result<bool> { match b { 0 => Ok(false), 1 => Ok(true), _ => Err(invalid()) } }
fn button(b: u8) -> io::Result<Button> {
    match b { 1 => Ok(Button::Left), 2 => Ok(Button::Right), 3 => Ok(Button::Middle), 4 => Ok(Button::Back), 5 => Ok(Button::Forward), _ => Err(invalid()) }
}
fn button_byte(b: Button) -> u8 {
    match b { Button::Left => 1, Button::Right => 2, Button::Middle => 3, Button::Back => 4, Button::Forward => 5 }
}

pub fn host_message_size(header: &[u8; 9]) -> io::Result<usize> {
    if header[..4] != MAGIC { return Err(invalid()); }
    let tag = header[4];
    let len = u32::from_le_bytes(header[5..9].try_into().unwrap()) as usize;
    validate_host_length(tag, len)?;
    Ok(9 + len)
}

fn validate_host_length(tag: u8, len: usize) -> io::Result<()> {
    let valid = match tag { 16 => len == 8, 17 => len == 7, 18 | 20 => len == 12, 19 => len == 6, 21 => len == 4, 22 => (4..=MAX_CLIPBOARD + 4).contains(&len), _ => false };
    if valid { Ok(()) } else { Err(invalid()) }
}

pub fn read_host<R: Read>(reader: &mut R) -> io::Result<Option<HostMessage>> {
    let Some((tag, len)) = header(reader)? else { return Ok(None) };
    validate_host_length(tag, len)?;
    let mut b = vec![0; len];
    reader.read_exact(&mut b)?;
    let msg = match tag {
        16 => HostMessage::Presented { serial: u64_at(&b, 0) },
        17 => HostMessage::Key { source: u32_at(&b, 0), code: u16::from_le_bytes([b[4], b[5]]), pressed: boolean(b[6])? },
        18 => HostMessage::Pointer { source: u32_at(&b, 0), x: u32_at(&b, 4), y: u32_at(&b, 8) },
        19 => HostMessage::Button { source: u32_at(&b, 0), button: button(b[4])?, pressed: boolean(b[5])? },
        20 => HostMessage::Wheel { source: u32_at(&b, 0), horizontal: u32_at(&b, 4) as i32, vertical: u32_at(&b, 8) as i32 },
        21 => HostMessage::ResetInput { source: u32_at(&b, 0) },
        22 => HostMessage::Clipboard { source: u32_at(&b, 0), text: String::from_utf8(b[4..].to_vec()).map_err(|_| invalid())? },
        _ => unreachable!(),
    };
    Ok(Some(msg))
}

pub fn write_host<W: Write>(writer: &mut W, message: &HostMessage) -> io::Result<()> {
    let mut b = Vec::new();
    let tag = match message {
        HostMessage::Presented { serial } => { b.extend(serial.to_le_bytes()); 16 }
        HostMessage::Key { source, code, pressed } => { b.extend(source.to_le_bytes()); b.extend(code.to_le_bytes()); b.push(u8::from(*pressed)); 17 }
        HostMessage::Pointer { source, x, y } => { b.extend(source.to_le_bytes()); b.extend(x.to_le_bytes()); b.extend(y.to_le_bytes()); 18 }
        HostMessage::Button { source, button, pressed } => { b.extend(source.to_le_bytes()); b.push(button_byte(*button)); b.push(u8::from(*pressed)); 19 }
        HostMessage::Wheel { source, horizontal, vertical } => { b.extend(source.to_le_bytes()); b.extend(horizontal.to_le_bytes()); b.extend(vertical.to_le_bytes()); 20 }
        HostMessage::ResetInput { source } => { b.extend(source.to_le_bytes()); 21 }
        HostMessage::Clipboard { source, text } => { if text.len() > MAX_CLIPBOARD { return Err(invalid()); } b.extend(source.to_le_bytes()); b.extend(text.as_bytes()); 22 }
    };
    emit(writer, tag, &b)
}

pub fn read_compositor<R: Read>(reader: &mut R, size: Option<Size>) -> io::Result<Option<CompositorMessage>> {
    let Some((tag, len)) = header(reader)? else { return Ok(None) };
    match tag {
        1 if len == 8 => {
            let mut b = [0; 8]; reader.read_exact(&mut b)?;
            let size = Size::new(u32_at(&b, 0), u32_at(&b, 4)).ok_or_else(invalid)?;
            Ok(Some(CompositorMessage::Ready { size }))
        }
        2 if size.is_some_and(|s| len == s.pixel_len() + 8) => {
            let mut serial = [0; 8]; reader.read_exact(&mut serial)?;
            let mut pixels = vec![0; len - 8]; reader.read_exact(&mut pixels)?;
            Ok(Some(CompositorMessage::Frame { serial: u64::from_le_bytes(serial), pixels }))
        }
        3 if size.is_some() && len >= 12 => {
            let size = size.unwrap();
            let mut head = [0; 12]; reader.read_exact(&mut head)?;
            let count = u32_at(&head, 8) as usize;
            if count > MAX_DAMAGE_RECTS || len < 12 + count * 16 { return Err(invalid()); }
            let mut table = vec![0; count * 16]; reader.read_exact(&mut table)?;
            let rects: Vec<Rect> = table.chunks_exact(16).map(|b| Rect { x: u32_at(b, 0), y: u32_at(b, 4), width: u32_at(b, 8), height: u32_at(b, 12) }).collect();
            if !rects.iter().all(|r| r.inside(size)) || rects.iter().map(|r| r.pixel_len()).sum::<usize>() != len - 12 - count * 16 { return Err(invalid()); }
            let mut out = Vec::with_capacity(count);
            for rect in rects {
                let mut pixels = vec![0; rect.pixel_len()]; reader.read_exact(&mut pixels)?;
                out.push((rect, pixels));
            }
            Ok(Some(CompositorMessage::Damage { serial: u64_at(&head, 0), moves: Vec::new(), rects: out }))
        }
        5 if size.is_some() && len >= 16 => {
            let size = size.unwrap();
            let mut body = vec![0; len]; reader.read_exact(&mut body)?;
            let count = u32_at(&body, 8) as usize;
            if count > MAX_DAMAGE_RECTS || len < 12 + count * 16 { return Err(invalid()); }
            let rects: Vec<Rect> = body[12..12 + count * 16].chunks_exact(16).map(|b| Rect { x: u32_at(b, 0), y: u32_at(b, 4), width: u32_at(b, 8), height: u32_at(b, 12) }).collect();
            if !rects.iter().all(|r| r.inside(size)) { return Err(invalid()); }
            let mut at = 12 + count * 16;
            let moves_count = u32_at(body.get(at..at + 4).ok_or_else(invalid)?, 0) as usize;
            at += 4;
            if moves_count > MAX_DAMAGE_MOVES { return Err(invalid()); }
            let table = body.get(at..at + moves_count * 20).ok_or_else(invalid)?;
            let moves: Vec<Move> = table.chunks_exact(20).map(|b| Move { x: u32_at(b, 0), width: u32_at(b, 4), src_y: u32_at(b, 8), dst_y: u32_at(b, 12), height: u32_at(b, 16) }).collect();
            if !moves.iter().all(|m| m.inside(size)) { return Err(invalid()); }
            at += moves_count * 20;
            let mut out = Vec::with_capacity(count);
            for rect in rects {
                let (columns, rows) = tile_grid(rect);
                let mut solids = Vec::with_capacity(columns * rows);
                for _ in 0..columns * rows {
                    match *body.get(at).ok_or_else(invalid)? {
                        TILE_RAW => { solids.push(None); at += 1; }
                        TILE_SOLID => {
                            let pixel: [u8; 4] = body.get(at + 1..at + 5).ok_or_else(invalid)?.try_into().unwrap();
                            solids.push(Some(pixel)); at += 5;
                        }
                        _ => return Err(invalid()),
                    }
                }
                let stride = rect.width as usize * 4;
                let mut pixels = vec![0; rect.pixel_len()];
                for y in 0..rect.height as usize {
                    let tile_row = &solids[y / DAMAGE_TILE as usize * columns..][..columns];
                    for (x0, x1) in raw_spans(rect, tile_row) {
                        let span = body.get(at..at + (x1 - x0) * 4).ok_or_else(invalid)?;
                        pixels[y * stride + x0 * 4..y * stride + x1 * 4].copy_from_slice(span);
                        at += span.len();
                    }
                }
                for (index, pixel) in solids.iter().enumerate() {
                    let Some(pixel) = pixel else { continue };
                    let (tx, ty) = ((index % columns) * DAMAGE_TILE as usize, (index / columns) * DAMAGE_TILE as usize);
                    let tw = (DAMAGE_TILE as usize).min(rect.width as usize - tx);
                    let th = (DAMAGE_TILE as usize).min(rect.height as usize - ty);
                    let first = ty * stride + tx * 4;
                    for p in pixels[first..first + tw * 4].chunks_exact_mut(4) { p.copy_from_slice(pixel); }
                    for y in ty + 1..ty + th { pixels.copy_within(first..first + tw * 4, y * stride + tx * 4); }
                }
                out.push((rect, pixels));
            }
            if at != len { return Err(invalid()); }
            Ok(Some(CompositorMessage::Damage { serial: u64_at(&body, 0), moves, rects: out }))
        }
        _ => Err(invalid()),
    }
}

pub fn write_compositor<W: Write>(writer: &mut W, message: &CompositorMessage, size: Option<Size>) -> io::Result<()> {
    match message {
        CompositorMessage::Ready { size } => { let mut b = Vec::with_capacity(8); b.extend(size.width.to_le_bytes()); b.extend(size.height.to_le_bytes()); emit(writer, 1, &b) }
        CompositorMessage::Frame { serial, pixels } => {
            if !size.is_some_and(|s| pixels.len() == s.pixel_len()) { return Err(invalid()); }
            writer.write_all(&MAGIC)?; writer.write_all(&[2])?;
            writer.write_all(&((pixels.len() + 8) as u32).to_le_bytes())?;
            writer.write_all(&serial.to_le_bytes())?; writer.write_all(pixels)
        }
        CompositorMessage::Damage { serial, moves, rects } => {
            let size = size.ok_or_else(invalid)?;
            if !moves.is_empty() { return Err(invalid()); }
            if rects.iter().any(|(rect, pixels)| pixels.len() != rect.pixel_len()) { return Err(invalid()); }
            let table: Vec<Rect> = rects.iter().map(|(rect, _)| *rect).collect();
            damage_header(writer, *serial, size, &table)?;
            for (_, pixels) in rects { writer.write_all(pixels)?; }
            Ok(())
        }
    }
}

/// Writes a damage message whose rectangles are cut from `framebuffer`, a whole output of BGRX rows.
pub fn write_damage<W: Write>(writer: &mut W, serial: u64, size: Size, framebuffer: &[u8], rects: &[Rect]) -> io::Result<()> {
    if framebuffer.len() != size.pixel_len() { return Err(invalid()); }
    damage_header(writer, serial, size, rects)?;
    let stride = size.width as usize * 4;
    for rect in rects {
        for y in rect.y as usize..(rect.y + rect.height) as usize {
            let start = y * stride + rect.x as usize * 4;
            writer.write_all(&framebuffer[start..start + rect.width as usize * 4])?;
        }
    }
    Ok(())
}

/// Columns and rows of [`DAMAGE_TILE`]-sized tiles covering `rect`, from its origin.
fn tile_grid(rect: Rect) -> (usize, usize) {
    (rect.width.div_ceil(DAMAGE_TILE) as usize, rect.height.div_ceil(DAMAGE_TILE) as usize)
}

/// Pixel ranges, relative to the rectangle's left edge, that one row of `rect` sends raw: the runs of tiles in
/// `tile_row` that are not solid.
fn raw_spans(rect: Rect, tile_row: &[Option<[u8; 4]>]) -> impl Iterator<Item = (usize, usize)> + '_ {
    let (tile, width) = (DAMAGE_TILE as usize, rect.width as usize);
    let mut column = 0;
    std::iter::from_fn(move || {
        while column < tile_row.len() && tile_row[column].is_some() { column += 1; }
        if column == tile_row.len() { return None; }
        let start = column;
        while column < tile_row.len() && tile_row[column].is_none() { column += 1; }
        Some((start * tile, (column * tile).min(width)))
    })
}

/// The pixel every pixel of a tile equals, if there is one.
fn solid_pixel(framebuffer: &[u8], stride: usize, x: usize, y: usize, w: usize, h: usize) -> Option<[u8; 4]> {
    let first: [u8; 4] = framebuffer[y * stride + x * 4..][..4].try_into().unwrap();
    let uniform = (y..y + h).all(|row| framebuffer[row * stride + x * 4..][..w * 4].chunks_exact(4).all(|p| p == first));
    uniform.then_some(first)
}

/// Writes a damage message with `moves` and the rectangles cut from `framebuffer`, sending each
/// [`DAMAGE_TILE`]-sized tile of a single colour as one pixel. After the rectangle and move tables, each rectangle
/// carries a table of its tiles, one kind byte each and the pixel of solid tiles, then its other pixels row by
/// row. Readers return it as a [`CompositorMessage::Damage`].
pub fn write_tiled_damage<W: Write>(writer: &mut W, serial: u64, size: Size, framebuffer: &[u8], moves: &[Move], rects: &[Rect]) -> io::Result<()> {
    if framebuffer.len() != size.pixel_len() { return Err(invalid()); }
    if rects.len() > MAX_DAMAGE_RECTS || !rects.iter().all(|r| r.inside(size)) { return Err(invalid()); }
    if moves.len() > MAX_DAMAGE_MOVES || !moves.iter().all(|m| m.inside(size)) { return Err(invalid()); }
    let (stride, tile) = (size.width as usize * 4, DAMAGE_TILE as usize);
    let solids: Vec<Vec<Option<[u8; 4]>>> = rects.iter().map(|r| {
        let (columns, rows) = tile_grid(*r);
        (0..columns * rows).map(|i| {
            let (tx, ty) = ((i % columns) * tile, (i / columns) * tile);
            let (tw, th) = (tile.min(r.width as usize - tx), tile.min(r.height as usize - ty));
            solid_pixel(framebuffer, stride, r.x as usize + tx, r.y as usize + ty, tw, th)
        }).collect()
    }).collect();
    let body: usize = rects.iter().zip(&solids).map(|(r, kinds)| {
        let (columns, _) = tile_grid(*r);
        let table: usize = kinds.iter().map(|k| if k.is_some() { 5 } else { 1 }).sum();
        let raw: usize = (0..r.height as usize).map(|y| raw_spans(*r, &kinds[y / tile * columns..][..columns]).map(|(a, b)| (b - a) * 4).sum::<usize>()).sum();
        table + raw
    }).sum();
    let len = 16 + rects.len() * 16 + moves.len() * 20 + body;
    writer.write_all(&MAGIC)?; writer.write_all(&[5])?;
    writer.write_all(&u32::try_from(len).map_err(|_| invalid())?.to_le_bytes())?;
    writer.write_all(&serial.to_le_bytes())?;
    writer.write_all(&(rects.len() as u32).to_le_bytes())?;
    for r in rects { for v in [r.x, r.y, r.width, r.height] { writer.write_all(&v.to_le_bytes())?; } }
    writer.write_all(&(moves.len() as u32).to_le_bytes())?;
    for m in moves { for v in [m.x, m.width, m.src_y, m.dst_y, m.height] { writer.write_all(&v.to_le_bytes())?; } }
    for (rect, kinds) in rects.iter().zip(&solids) {
        for kind in kinds {
            match kind { Some(pixel) => { writer.write_all(&[TILE_SOLID])?; writer.write_all(pixel)?; } None => writer.write_all(&[TILE_RAW])? }
        }
        let (columns, _) = tile_grid(*rect);
        for y in rect.y as usize..(rect.y + rect.height) as usize {
            let tile_row = &kinds[(y - rect.y as usize) / tile * columns..][..columns];
            let row = y * stride + rect.x as usize * 4;
            for (x0, x1) in raw_spans(*rect, tile_row) { writer.write_all(&framebuffer[row + x0 * 4..row + x1 * 4])?; }
        }
    }
    Ok(())
}

fn damage_header<W: Write>(writer: &mut W, serial: u64, size: Size, rects: &[Rect]) -> io::Result<()> {
    if rects.len() > MAX_DAMAGE_RECTS || !rects.iter().all(|r| r.inside(size)) { return Err(invalid()); }
    let len = 12 + rects.len() * 16 + rects.iter().map(|r| r.pixel_len()).sum::<usize>();
    writer.write_all(&MAGIC)?; writer.write_all(&[3])?;
    writer.write_all(&u32::try_from(len).map_err(|_| invalid())?.to_le_bytes())?;
    writer.write_all(&serial.to_le_bytes())?;
    writer.write_all(&(rects.len() as u32).to_le_bytes())?;
    for r in rects { for v in [r.x, r.y, r.width, r.height] { writer.write_all(&v.to_le_bytes())?; } }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn host_roundtrip_and_truncation() {
        let messages = [HostMessage::Presented { serial: 42 }, HostMessage::Key { source: 1, code: 30, pressed: true }, HostMessage::Pointer { source: 2, x: 1, y: 9 }, HostMessage::Button { source: 3, button: Button::Middle, pressed: false }, HostMessage::Wheel { source: 4, horizontal: -2, vertical: 3 }, HostMessage::ResetInput { source: 5 }, HostMessage::Clipboard { source: 6, text: "hello λ".into() }];
        for msg in messages {
            let mut wire = Vec::new(); write_host(&mut wire, &msg).unwrap();
            assert_eq!(read_host(&mut &wire[..]).unwrap(), Some(msg));
            for n in 1..wire.len() { assert!(read_host(&mut &wire[..n]).is_err()); }
        }
        assert_eq!(read_host(&mut &[][..]).unwrap(), None);
    }
    #[test]
    fn frames_require_exact_size_and_direction() {
        assert!(Size::new(0, 1).is_none()); assert!(Size::new(4097, 1).is_none());
        let size = Size::new(2, 1).unwrap();
        let msg = CompositorMessage::Frame { serial: 8, pixels: vec![7; 8] };
        let mut wire = Vec::new(); write_compositor(&mut wire, &msg, Some(size)).unwrap();
        assert_eq!(read_compositor(&mut &wire[..], Some(size)).unwrap(), Some(msg));
        assert!(read_compositor(&mut &wire[..], None).is_err());
        assert!(read_host(&mut &wire[..]).is_err());
        for n in 1..wire.len() { assert!(read_compositor(&mut &wire[..n], Some(size)).is_err()); }
        wire[0] = 0; assert!(read_compositor(&mut &wire[..], Some(size)).is_err());
    }
    #[test]
    fn damage_carries_only_its_rectangles_and_rejects_bad_geometry() {
        let size = Size::new(4, 3).unwrap();
        let framebuffer: Vec<u8> = (0..size.pixel_len() as u32).map(|n| n as u8).collect();
        let rects = [Rect { x: 1, y: 1, width: 2, height: 2 }, Rect { x: 3, y: 0, width: 1, height: 1 }];
        let mut wire = Vec::new(); write_damage(&mut wire, 9, size, &framebuffer, &rects).unwrap();
        let Some(CompositorMessage::Damage { serial, rects: got, .. }) = read_compositor(&mut &wire[..], Some(size)).unwrap() else { panic!("not damage") };
        assert_eq!(serial, 9);
        assert_eq!(got[0], (rects[0], [&framebuffer[20..28], &framebuffer[36..44]].concat()));
        assert_eq!(got[1], (rects[1], framebuffer[12..16].to_vec()));
        let mut again = Vec::new(); write_compositor(&mut again, &CompositorMessage::Damage { serial, moves: vec![], rects: got }, Some(size)).unwrap();
        assert_eq!(again, wire);
        for n in 1..wire.len() { assert!(read_compositor(&mut &wire[..n], Some(size)).is_err()); }
        let mut empty = Vec::new(); write_damage(&mut empty, 10, size, &framebuffer, &[]).unwrap();
        assert_eq!(read_compositor(&mut &empty[..], Some(size)).unwrap(), Some(CompositorMessage::Damage { serial: 10, moves: vec![], rects: vec![] }));
        for bad in [Rect { x: 3, y: 0, width: 2, height: 1 }, Rect { x: 0, y: 0, width: 0, height: 1 }, Rect { x: u32::MAX, y: 0, width: 2, height: 1 }] {
            assert!(write_damage(&mut Vec::new(), 1, size, &framebuffer, &[bad]).is_err());
            let mut forged = Vec::new(); write_damage(&mut forged, 1, size, &framebuffer, &[Rect { x: 0, y: 0, width: 1, height: 1 }]).unwrap();
            forged[21..37].copy_from_slice(&[bad.x, bad.y, bad.width, bad.height].map(u32::to_le_bytes).concat());
            assert!(read_compositor(&mut &forged[..], Some(size)).is_err());
        }
        assert!(write_damage(&mut Vec::new(), 1, size, &framebuffer, &[Rect { x: 0, y: 0, width: 1, height: 1 }; MAX_DAMAGE_RECTS + 1]).is_err());
        assert!(read_compositor(&mut &wire[..], None).is_err());
    }

    #[test]
    fn tiled_damage_matches_plain_damage_and_shrinks_solid_tiles() {
        let size = Size::new(150, 70).unwrap();
        let mut framebuffer = vec![0; size.pixel_len()];
        for (i, p) in framebuffer.chunks_exact_mut(4).enumerate() { p.copy_from_slice(&[7, 8, 9, 0]); if i % 150 >= 128 && i / 150 < 3 { p[0] = i as u8; } }
        let rects = [Rect { x: 0, y: 0, width: 150, height: 70 }, Rect { x: 140, y: 1, width: 10, height: 2 }, Rect { x: 5, y: 66, width: 1, height: 1 }];
        let mut plain = Vec::new(); write_damage(&mut plain, 3, size, &framebuffer, &rects).unwrap();
        let mut tiled = Vec::new(); write_tiled_damage(&mut tiled, 3, size, &framebuffer, &[], &rects).unwrap();
        assert_eq!(read_compositor(&mut &tiled[..], Some(size)).unwrap(), read_compositor(&mut &plain[..], Some(size)).unwrap());
        assert!(tiled.len() * 6 < plain.len());
        for n in 1..tiled.len() { assert!(read_compositor(&mut &tiled[..n], Some(size)).is_err()); }
        let mut grown = tiled.clone(); grown.push(0);
        let len = u32::from_le_bytes(grown[5..9].try_into().unwrap()) + 1; grown[5..9].copy_from_slice(&len.to_le_bytes());
        assert!(read_compositor(&mut &grown[..], Some(size)).is_err());
        let mut forged = tiled.clone(); forged[9 + 12 + 3 * 16 + 4] = 2;
        assert!(read_compositor(&mut &forged[..], Some(size)).is_err());
        assert!(read_compositor(&mut &tiled[..], None).is_err());
    }

    #[test]
    fn tiled_damage_splits_rows_around_solid_tiles() {
        let size = Size::new(200, 130).unwrap();
        let mut framebuffer = vec![0; size.pixel_len()];
        for (i, p) in framebuffer.chunks_exact_mut(4).enumerate() {
            let (x, y) = (i % 200, i / 200);
            p.copy_from_slice(&if (64..128).contains(&x) || (y >= 64 && x >= 180) { [x as u8, y as u8, 3, 4] } else { [1, 2, 3, 4] });
        }
        let rects = [Rect { x: 0, y: 0, width: 200, height: 130 }, Rect { x: 30, y: 10, width: 150, height: 100 }];
        let mut plain = Vec::new(); write_damage(&mut plain, 5, size, &framebuffer, &rects).unwrap();
        let mut tiled = Vec::new(); write_tiled_damage(&mut tiled, 5, size, &framebuffer, &[], &rects).unwrap();
        assert_eq!(read_compositor(&mut &tiled[..], Some(size)).unwrap(), read_compositor(&mut &plain[..], Some(size)).unwrap());
        assert!(tiled.len() < plain.len());
    }

    #[test]
    fn moves_rebuild_a_scrolled_frame_from_the_previous_one() {
        let size = Size::new(70, 90).unwrap();
        let previous: Vec<u8> = (0..size.pixel_len()).map(|i| (i / 280 * 7 + i % 280) as u8).collect();
        let mut current = previous.clone();
        let stride = 280;
        current.copy_within(10 * stride..90 * stride, 0);
        for b in &mut current[80 * stride..] { *b = 0xaa; }
        let moves = [Move { x: 0, width: 70, src_y: 10, dst_y: 0, height: 80 }];
        let rects = [Rect { x: 0, y: 80, width: 70, height: 10 }];
        let mut wire = Vec::new(); write_tiled_damage(&mut wire, 4, size, &current, &moves, &rects).unwrap();
        let Some(CompositorMessage::Damage { serial: 4, moves: got_moves, rects: got }) = read_compositor(&mut &wire[..], Some(size)).unwrap() else { panic!("not damage") };
        assert_eq!(got_moves, moves);
        let mut rebuilt = previous.clone();
        for m in &got_moves { m.apply(size, &mut rebuilt); }
        for (rect, pixels) in &got { for (row, line) in pixels.chunks_exact(rect.width as usize * 4).enumerate() { let at = (rect.y as usize + row) * stride + rect.x as usize * 4; rebuilt[at..at + line.len()].copy_from_slice(line); } }
        assert_eq!(rebuilt, current);
        let mut down = previous.clone(); Move { x: 0, width: 70, src_y: 0, dst_y: 10, height: 80 }.apply(size, &mut down);
        assert_eq!(&down[10 * stride..], &previous[..80 * stride]);
        for bad in [Move { x: 0, width: 71, src_y: 0, dst_y: 0, height: 1 }, Move { x: 0, width: 1, src_y: 85, dst_y: 0, height: 6 }, Move { x: 0, width: 0, src_y: 0, dst_y: 0, height: 1 }] {
            assert!(write_tiled_damage(&mut Vec::new(), 1, size, &current, &[bad], &[]).is_err());
        }
        assert!(write_compositor(&mut Vec::new(), &CompositorMessage::Damage { serial: 1, moves: moves.to_vec(), rects: vec![] }, Some(size)).is_err());
    }

    #[test]
    fn rejects_invalid_lengths_values_and_utf8() {
        let mut wire = Vec::new(); write_host(&mut wire, &HostMessage::Clipboard { source: 1, text: "x".into() }).unwrap();
        *wire.last_mut().unwrap() = 0xff; assert!(read_host(&mut &wire[..]).is_err());
        wire[5..9].copy_from_slice(&(MAX_CLIPBOARD as u32 + 5).to_le_bytes()); assert!(read_host(&mut &wire[..]).is_err());
        let mut wire = Vec::new(); write_host(&mut wire, &HostMessage::Key { source: 1, code: 30, pressed: true }).unwrap();
        *wire.last_mut().unwrap() = 2; assert!(read_host(&mut &wire[..]).is_err());
    }
}
