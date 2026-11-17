use std::io::{self, Read, Write};

const MAGIC: [u8; 4] = *b"TLR1";
const MAX_CLIPBOARD: usize = 1024 * 1024;

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompositorMessage { Ready { size: Size }, Frame { serial: u64, pixels: Vec<u8> } }

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
    }
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
    fn rejects_invalid_lengths_values_and_utf8() {
        let mut wire = Vec::new(); write_host(&mut wire, &HostMessage::Clipboard { source: 1, text: "x".into() }).unwrap();
        *wire.last_mut().unwrap() = 0xff; assert!(read_host(&mut &wire[..]).is_err());
        wire[5..9].copy_from_slice(&(MAX_CLIPBOARD as u32 + 5).to_le_bytes()); assert!(read_host(&mut &wire[..]).is_err());
        let mut wire = Vec::new(); write_host(&mut wire, &HostMessage::Key { source: 1, code: 30, pressed: true }).unwrap();
        *wire.last_mut().unwrap() = 2; assert!(read_host(&mut &wire[..]).is_err());
    }
}
