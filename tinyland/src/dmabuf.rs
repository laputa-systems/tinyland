use std::os::fd::{AsFd, OwnedFd};

pub const DRM_FORMAT_XRGB8888: u32 = 0x3432_5258;
pub const DRM_FORMAT_ARGB8888: u32 = 0x3432_5241;
pub const DRM_FORMAT_MOD_INVALID: u64 = 0x00ff_ffff_ffff_ffff;
pub const SUPPORTED_FORMATS: [(u32, u64); 2] =
    [(DRM_FORMAT_XRGB8888, DRM_FORMAT_MOD_INVALID), (DRM_FORMAT_ARGB8888, DRM_FORMAT_MOD_INVALID)];
pub const MAX_DIMENSION: i32 = 8192;
pub const MAX_STRIDE: u32 = 32768;

#[derive(Debug)]
pub struct DmabufBuffer {
    pub fd: OwnedFd,
    pub width: u32,
    pub height: u32,
    pub offset: u32,
    pub stride: u32,
}

#[derive(Debug)]
pub struct DmabufPlane {
    pub fd: OwnedFd,
    pub offset: u32,
    pub stride: u32,
    pub modifier: u64,
}

#[derive(Debug, Default)]
pub struct DmabufParams {
    pub used: bool,
    pub plane: Option<DmabufPlane>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamsError {
    AlreadyUsed = 0,
    PlaneIdx = 1,
    PlaneSet = 2,
    Incomplete = 3,
    InvalidFormat = 4,
    InvalidDimensions = 5,
    OutOfBounds = 6,
}

impl ParamsError {
    pub fn message(self) -> &'static str {
        match self {
            ParamsError::AlreadyUsed => "dmabuf params already used",
            ParamsError::PlaneIdx => "only single-plane formats are supported",
            ParamsError::PlaneSet => "dmabuf plane already set",
            ParamsError::Incomplete => "dmabuf plane 0 missing",
            ParamsError::InvalidFormat => "dmabuf format or modifier not advertised",
            ParamsError::InvalidDimensions => "invalid dmabuf dimensions",
            ParamsError::OutOfBounds => "dmabuf plane exceeds its file",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Unsupported {
    Flags(u32),
}

impl DmabufParams {
    pub fn add(&mut self, fd: OwnedFd, plane_idx: u32, offset: u32, stride: u32, modifier: u64) -> Result<(), ParamsError> {
        if self.used {
            return Err(ParamsError::AlreadyUsed);
        }
        if plane_idx != 0 {
            return Err(ParamsError::PlaneIdx);
        }
        if self.plane.is_some() {
            return Err(ParamsError::PlaneSet);
        }
        self.plane = Some(DmabufPlane { fd, offset, stride, modifier });
        Ok(())
    }

    pub fn create(
        &mut self,
        width: i32,
        height: i32,
        format: u32,
        flags: u32,
    ) -> Result<Result<DmabufBuffer, Unsupported>, ParamsError> {
        if self.used {
            return Err(ParamsError::AlreadyUsed);
        }
        self.used = true;
        let plane = self.plane.take().ok_or(ParamsError::Incomplete)?;
        if !SUPPORTED_FORMATS.contains(&(format, plane.modifier)) {
            return Err(ParamsError::InvalidFormat);
        }
        if width <= 0 || height <= 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
            return Err(ParamsError::InvalidDimensions);
        }
        let row = width as u64 * 4;
        if (plane.stride as u64) < row || plane.stride % 4 != 0 || plane.stride > MAX_STRIDE {
            return Err(ParamsError::OutOfBounds);
        }
        let end = plane.offset as u64 + plane.stride as u64 * (height as u64 - 1) + row;
        let size = rustix::fs::seek(plane.fd.as_fd(), rustix::fs::SeekFrom::End(0)).map_err(|_| ParamsError::OutOfBounds)?;
        if end > size {
            return Err(ParamsError::OutOfBounds);
        }
        if flags != 0 {
            return Ok(Err(Unsupported::Flags(flags)));
        }
        Ok(Ok(DmabufBuffer {
            fd: plane.fd,
            width: width as u32,
            height: height as u32,
            offset: plane.offset,
            stride: plane.stride,
        }))
    }
}

#[derive(Debug)]
pub struct DmabufFeedback {
    pub device: u64,
    pub table: OwnedFd,
    pub table_size: u32,
}

impl DmabufFeedback {
    pub fn new(device: u64) -> Result<Self, String> {
        use rustix::fs::{fcntl_add_seals, memfd_create, MemfdFlags, SealFlags};
        let mut bytes = Vec::new();
        for (format, modifier) in SUPPORTED_FORMATS {
            bytes.extend(format.to_ne_bytes());
            bytes.extend(0u32.to_ne_bytes());
            bytes.extend(modifier.to_ne_bytes());
        }
        let table = memfd_create("tinyland-dmabuf-formats", MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING)
            .map_err(|error| format!("format table memfd: {error}"))?;
        let mut written = 0;
        while written < bytes.len() {
            written += rustix::io::write(&table, &bytes[written..]).map_err(|error| format!("format table write: {error}"))?;
        }
        fcntl_add_seals(&table, SealFlags::SHRINK | SealFlags::GROW | SealFlags::WRITE | SealFlags::SEAL)
            .map_err(|error| format!("format table seal: {error}"))?;
        Ok(Self { device, table, table_size: bytes.len() as u32 })
    }

    pub fn device_bytes(&self) -> Vec<u8> {
        self.device.to_ne_bytes().to_vec()
    }

    pub fn tranche_indices() -> Vec<u8> {
        (0..SUPPORTED_FORMATS.len() as u16).flat_map(u16::to_ne_bytes).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backing(size: u64) -> OwnedFd {
        let file = tempfile::tempfile().unwrap();
        file.set_len(size).unwrap();
        OwnedFd::from(file)
    }

    fn params(size: u64, offset: u32, stride: u32, modifier: u64) -> DmabufParams {
        let mut params = DmabufParams::default();
        params.add(backing(size), 0, offset, stride, modifier).unwrap();
        params
    }

    #[test]
    fn exact_fit_is_accepted_and_one_byte_short_is_out_of_bounds() {
        let buffer = params(4 * 16 + 8, 0, 16, DRM_FORMAT_MOD_INVALID).create(2, 5, DRM_FORMAT_XRGB8888, 0).unwrap().unwrap();
        assert_eq!((buffer.width, buffer.height, buffer.stride), (2, 5, 16));
        let short = params(4 * 16 + 7, 0, 16, DRM_FORMAT_MOD_INVALID).create(2, 5, DRM_FORMAT_XRGB8888, 0);
        assert_eq!(short.unwrap_err(), ParamsError::OutOfBounds);
        let offset = params(4 * 16 + 8, 4, 16, DRM_FORMAT_MOD_INVALID).create(2, 5, DRM_FORMAT_ARGB8888, 0);
        assert_eq!(offset.unwrap_err(), ParamsError::OutOfBounds);
    }

    #[test]
    fn only_advertised_format_and_modifier_pairs_are_accepted() {
        let linear = params(64, 0, 8, 0).create(2, 2, DRM_FORMAT_XRGB8888, 0);
        assert_eq!(linear.unwrap_err(), ParamsError::InvalidFormat);
        let nv12 = params(64, 0, 8, DRM_FORMAT_MOD_INVALID).create(2, 2, 0x3231_564e, 0);
        assert_eq!(nv12.unwrap_err(), ParamsError::InvalidFormat);
    }

    #[test]
    fn plane_rules_follow_the_protocol() {
        let mut params = DmabufParams::default();
        assert_eq!(params.add(backing(64), 1, 0, 8, DRM_FORMAT_MOD_INVALID), Err(ParamsError::PlaneIdx));
        params.add(backing(64), 0, 0, 8, DRM_FORMAT_MOD_INVALID).unwrap();
        assert_eq!(params.add(backing(64), 0, 0, 8, DRM_FORMAT_MOD_INVALID), Err(ParamsError::PlaneSet));
        params.create(2, 2, DRM_FORMAT_XRGB8888, 0).unwrap().unwrap();
        assert_eq!(params.create(2, 2, DRM_FORMAT_XRGB8888, 0).unwrap_err(), ParamsError::AlreadyUsed);
        assert_eq!(params.add(backing(64), 0, 0, 8, DRM_FORMAT_MOD_INVALID), Err(ParamsError::AlreadyUsed));
        assert_eq!(DmabufParams::default().create(2, 2, DRM_FORMAT_XRGB8888, 0).unwrap_err(), ParamsError::Incomplete);
    }

    #[test]
    fn invalid_dimensions_and_strides_are_rejected() {
        for (width, height) in [(0, 2), (2, 0), (-1, 2), (MAX_DIMENSION + 1, 1)] {
            let result = params(1 << 20, 0, 32768, DRM_FORMAT_MOD_INVALID).create(width, height, DRM_FORMAT_XRGB8888, 0);
            assert_eq!(result.unwrap_err(), ParamsError::InvalidDimensions);
        }
        for stride in [4, 10, MAX_STRIDE + 4] {
            let result = params(1 << 20, 0, stride, DRM_FORMAT_MOD_INVALID).create(2, 2, DRM_FORMAT_XRGB8888, 0);
            assert_eq!(result.unwrap_err(), ParamsError::OutOfBounds);
        }
    }

    #[test]
    fn y_invert_and_interlaced_buffers_fail_instead_of_being_shown_wrongly() {
        let result = params(64, 0, 8, DRM_FORMAT_MOD_INVALID).create(2, 2, DRM_FORMAT_XRGB8888, 1).unwrap();
        assert_eq!(result.unwrap_err(), Unsupported::Flags(1));
    }

    #[test]
    fn format_table_is_sealed_and_lists_each_advertised_pair() {
        use std::os::unix::fs::FileExt;
        let feedback = DmabufFeedback::new(0xe200).unwrap();
        assert_eq!(feedback.table_size, 32);
        let file = std::fs::File::from(feedback.table.try_clone().unwrap());
        let mut entry = [0u8; 16];
        file.read_exact_at(&mut entry, 16).unwrap();
        assert_eq!(&entry[..4], &DRM_FORMAT_ARGB8888.to_ne_bytes());
        assert_eq!(&entry[8..], &DRM_FORMAT_MOD_INVALID.to_ne_bytes());
        assert!(file.write_at(&[0], 0).is_err());
        assert_eq!(DmabufFeedback::tranche_indices(), [0u16.to_ne_bytes(), 1u16.to_ne_bytes()].concat());
        assert_eq!(feedback.device_bytes(), 0xe200u64.to_ne_bytes());
    }
}
