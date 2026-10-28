use wayland_server::backend::ObjectId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShmFormat {
    Argb8888,
    Xrgb8888,
}

pub struct ShmPoolData {
    pub file: std::fs::File,
    pub mmap: memmap2::Mmap,
    pub size: i64,
}

impl ShmPoolData {
    pub fn resize(&mut self, size: i32) -> Result<(), String> {
        if size as i64 <= self.size {
            return Err("pool resize must grow".into());
        }
        // SAFETY: the mapping is read-only, owned by this pool, and only validated buffer ranges are read.
        let mmap = unsafe { memmap2::MmapOptions::new().len(size as usize).map(&self.file) }
            .map_err(|e| format!("resize mmap: {e}"))?;
        self.mmap = mmap;
        self.size = size as i64;
        Ok(())
    }
}

pub struct ShmBufferData {
    pub pool: ObjectId,
    pub offset: i32,
    pub width: i32,
    pub height: i32,
    pub stride: i32,
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

pub fn read_rows(pool: &memmap2::Mmap, buf: &ShmBufferData) -> Result<Vec<u8>, &'static str> {
    let need = buf.offset as usize + buf.stride as usize * buf.height as usize;
    if need > pool.len() {
        return Err("buffer out of mapping");
    }
    let wbytes = buf.width as usize * 4;
    let mut out = vec![0u8; wbytes * buf.height as usize];
    for y in 0..buf.height as usize {
        let src = buf.offset as usize + y * buf.stride as usize;
        let dst = y * wbytes;
        out[dst..dst + wbytes].copy_from_slice(&pool[src..src + wbytes]);
    }
    Ok(out)
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
mod resize_tests {
    use super::*;

    #[test]
    fn grown_pool_accepts_buffers_in_the_extended_range() {
        let file = tempfile::tempfile().unwrap();
        file.set_len(2304).unwrap();
        // SAFETY: the test owns the file and keeps its size at least the mapping length.
        let mmap = unsafe { memmap2::MmapOptions::new().len(2304).map(&file) }.unwrap();
        let mut pool = ShmPoolData { file, mmap, size: 2304 };
        pool.file.set_len(12096).unwrap();
        pool.resize(12096).unwrap();
        assert_eq!(pool.mmap.len(), 12096);
        assert!(validate_buffer(pool.size, 9600, 9, 16, 36).is_ok());
        assert!(pool.resize(2304).is_err());
    }
}
