#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShmFormat {
    Argb8888,
    Xrgb8888,
    Abgr8888,
    Xbgr8888,
}

impl ShmFormat {
    /// Byte offsets of the blue, green and red channels inside one pixel.
    pub fn channel_offsets(self) -> (usize, usize, usize) {
        match self {
            Self::Argb8888 | Self::Xrgb8888 => (0, 1, 2),
            Self::Abgr8888 | Self::Xbgr8888 => (2, 1, 0),
        }
    }

    pub fn has_alpha(self) -> bool {
        matches!(self, Self::Argb8888 | Self::Abgr8888)
    }
}

pub struct ShmPoolData {
    pub file: std::sync::Arc<std::fs::File>,
    pub size: i64,
}

impl ShmPoolData {
    pub fn new(file: std::fs::File, size: i32) -> Result<Self, String> {
        validate_pool_size(size)?;
        if file.metadata().map_err(|e| format!("pool metadata: {e}"))?.len() < size as u64 {
            return Err("pool size exceeds backing file".into());
        }
        Ok(Self { file: std::sync::Arc::new(file), size: size as i64 })
    }

    pub fn resize(&mut self, size: i32) -> Result<(), String> {
        if size as i64 <= self.size { return Err("pool resize must grow".into()); }
        if self.file.metadata().map_err(|e| format!("pool metadata: {e}"))?.len() < size as u64 {
            return Err("pool resize exceeds backing file".into());
        }
        self.size = size as i64;
        Ok(())
    }
}

pub struct ShmBufferData {
    pub file: std::sync::Arc<std::fs::File>,
    pub offset: i32,
    pub width: i32,
    pub height: i32,
    pub stride: i32,
    pub format: ShmFormat,
}

#[derive(Debug, Clone)]
pub struct SurfacePixels {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub format: ShmFormat,
}

pub fn validate_pool_size(size: i32) -> Result<usize, &'static str> {
    if size <= 0 {
        return Err("pool size must be positive");
    }
    Ok(size as usize)
}

pub fn validate_buffer(
    pool_size: i64,
    offset: i32,
    width: i32,
    height: i32,
    stride: i32,
) -> Result<(), &'static str> {
    if offset < 0 {
        return Err("negative buffer offset");
    }
    if width <= 0 || height <= 0 {
        return Err("non-positive buffer dimensions");
    }
    if width > 8192 || height > 8192 {
        return Err("buffer dimensions exceed limit");
    }
    if stride < width * 4 {
        return Err("stride smaller than row");
    }
    if stride % 4 != 0 {
        return Err("stride not 4-aligned");
    }
    if stride > 32768 {
        return Err("stride exceeds limit");
    }
    let end = offset as i64 + stride as i64 * height as i64;
    if end > pool_size {
        return Err("buffer extends past pool end");
    }
    Ok(())
}

pub fn read_rows(buf: &ShmBufferData) -> Result<SurfacePixels, String> {
    use std::os::unix::fs::FileExt;
    let wbytes = buf.width as usize * 4;
    let total = wbytes.checked_mul(buf.height as usize).ok_or("buffer size overflow")?;
    let mut out = Vec::new();
    out.try_reserve_exact(total).map_err(|_| "buffer allocation failed".to_string())?;
    out.resize(total, 0);
    for y in 0..buf.height as usize {
        let src = buf.offset as u64 + y as u64 * buf.stride as u64;
        let dst = y * wbytes;
        buf.file.read_exact_at(&mut out[dst..dst + wbytes], src)
            .map_err(|e| format!("read shm buffer: {e}"))?;
    }
    Ok(SurfacePixels { bytes: out, width: buf.width as u32, height: buf.height as u32, format: buf.format })
}

/// Replaces `pixels` inside `rects` (clipped to the buffer) with the buffer's current rows there.
/// `pixels` must already hold this buffer's size and format.
pub fn read_rects(buf: &ShmBufferData, pixels: &mut SurfacePixels, rects: &[crate::positioner::Rect]) -> Result<(), String> {
    use std::os::unix::fs::FileExt;
    if pixels.width != buf.width as u32 || pixels.height != buf.height as u32 || pixels.format != buf.format {
        return Err("partial read into a differently shaped surface".into());
    }
    let wbytes = buf.width as usize * 4;
    for rect in rects {
        let Some((x0, y0, x1, y1)) = crate::damage::clip(*rect, pixels.width, pixels.height) else { continue };
        for y in y0 as usize..y1 as usize {
            let src = buf.offset as u64 + y as u64 * buf.stride as u64 + u64::from(x0) * 4;
            let dst = y * wbytes + x0 as usize * 4;
            buf.file.read_exact_at(&mut pixels.bytes[dst..dst + (x1 - x0) as usize * 4], src)
                .map_err(|e| format!("read shm buffer: {e}"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_negative_offset() {
        assert!(validate_buffer(100, -4, 2, 2, 8).is_err());
    }

    #[test]
    fn rejects_past_end() {
        assert!(validate_buffer(16, 0, 2, 2, 8).is_ok());
        assert!(validate_buffer(15, 0, 2, 2, 8).is_err());
    }

    #[test]
    fn rejects_short_stride() {
        assert!(validate_buffer(100, 0, 4, 1, 15).is_err());
        assert!(validate_buffer(100, 0, 4, 1, 16).is_ok());
    }

    #[test]
    fn rejects_zero_size_pool() {
        assert!(validate_pool_size(0).is_err());
    }

    #[test]
    fn rejects_huge_dimensions() {
        assert!(validate_buffer(i64::MAX, 0, 9000, 2, 36000).is_err());
    }
}

#[cfg(test)]
mod lifetime_tests {
    use super::*;
    use std::os::unix::fs::FileExt;

    #[test]
    fn grown_pool_accepts_buffers_in_the_extended_range() {
        let file = tempfile::tempfile().unwrap();
        file.set_len(2304).unwrap();
        let mut pool = ShmPoolData::new(file, 2304).unwrap();
        pool.file.set_len(12096).unwrap();
        pool.resize(12096).unwrap();
        assert!(validate_buffer(pool.size, 9600, 9, 16, 36).is_ok());
        assert!(pool.resize(2304).is_err());
        assert!(pool.resize(16000).is_err());
    }

    #[test]
    fn pool_destruction_keeps_buffers_alive_and_truncation_returns_error() {
        let file = tempfile::tempfile().unwrap();
        file.set_len(24).unwrap();
        file.write_all_at(&[1, 2, 3, 255, 4, 5, 6, 255], 0).unwrap();
        file.write_all_at(&[7, 8, 9, 255, 10, 11, 12, 255], 12).unwrap();
        let pool = ShmPoolData::new(file, 24).unwrap();
        let buffer = ShmBufferData { file: pool.file.clone(), offset: 0, width: 2, height: 2, stride: 12, format: ShmFormat::Xrgb8888 };
        drop(pool);
        let pixels = read_rows(&buffer).unwrap();
        assert_eq!(pixels.bytes, [1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255]);
        buffer.file.set_len(0).unwrap();
        assert!(read_rows(&buffer).is_err());
        assert_eq!(pixels.bytes[0], 1);
    }

    #[test]
    fn partial_reads_replace_only_clipped_rectangles() {
        use crate::positioner::Rect;
        let file = tempfile::tempfile().unwrap();
        file.set_len(48).unwrap();
        let fresh: Vec<u8> = (100..148).collect();
        file.write_all_at(&fresh, 0).unwrap();
        let buffer = ShmBufferData { file: std::sync::Arc::new(file), offset: 0, width: 3, height: 2, stride: 24, format: ShmFormat::Xrgb8888 };
        let mut pixels = SurfacePixels { bytes: vec![0; 24], width: 3, height: 2, format: ShmFormat::Xrgb8888 };
        read_rects(&buffer, &mut pixels, &[Rect { x: 2, y: -5, width: 9, height: 6 }, Rect { x: -1, y: 1, width: 2, height: 1 }]).unwrap();
        let mut expected = vec![0; 24];
        expected[8..12].copy_from_slice(&fresh[8..12]);
        expected[12..16].copy_from_slice(&fresh[24..28]);
        assert_eq!(pixels.bytes, expected);
        let mut wrong = SurfacePixels { bytes: vec![0; 24], width: 3, height: 2, format: ShmFormat::Argb8888 };
        assert!(read_rects(&buffer, &mut wrong, &[]).is_err());
    }
}
