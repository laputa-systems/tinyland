use drm::buffer::{Buffer as _, DrmFourcc};
use drm::control::connector::{self};
use drm::control::dumbbuffer::DumbBuffer;
use drm::control::framebuffer;
use drm::control::{AtomicCommitFlags, Device as ControlDevice};
use drm::Device;
use std::fs::{File, OpenOptions};
use std::os::unix::io::{AsFd, BorrowedFd};
use std::path::Path;

struct Card(File);

impl AsFd for Card {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl Device for Card {}
impl ControlDevice for Card {}

impl Card {
    fn open(path: &Path) -> std::io::Result<Self> {
        let mut opts = OpenOptions::new();
        opts.read(true).write(true);
        Ok(Card(opts.open(path)?))
    }
}

pub struct DrmOutput {
    card: Card,
    db: DumbBuffer,
    fb: framebuffer::Handle,
    width: u32,
    height: u32,
    stride: u32,
    mode_path: String,
}

impl DrmOutput {
    pub fn open(device: &Path, want_w: u32, want_h: u32) -> Result<Self, String> {
        let card = Card::open(device).map_err(|e| format!("open {}: {e}", device.display()))?;
        let _ = card.set_client_capability(drm::ClientCapability::UniversalPlanes, true);
        let atomic_ok = card
            .set_client_capability(drm::ClientCapability::Atomic, true)
            .is_ok();
        log::info!("drm atomic client cap granted={atomic_ok}");

        let res = card
            .resource_handles()
            .map_err(|e| format!("resource handles: {e}"))?;
        let connectors: Vec<connector::Info> = res
            .connectors()
            .iter()
            .flat_map(|h| card.get_connector(*h, false))
            .collect();
        if connectors.is_empty() {
            return Err("no connectors".into());
        }
        let con = connectors
            .iter()
            .find(|c| c.state() == connector::State::Connected)
            .or_else(|| connectors.first())
            .ok_or("no connector")?;
        let mode = con
            .modes()
            .first()
            .copied()
            .ok_or_else(|| {
                format!(
                    "connector {:?} has no modes; requested {want_w}x{want_h}",
                    con.handle()
                )
            })?;
        let (mw, mh) = mode.size();
        log::info!("drm connector {:?} mode {mw}x{mh}", con.handle());
        let (w, h): (u32, u32) = (mw.into(), mh.into());
        if w < want_w || h < want_h {
            return Err(format!(
                "mode {w}x{h} smaller than requested {want_w}x{want_h}"
            ));
        }

        let fmt = DrmFourcc::Xrgb8888;
        let db = card
            .create_dumb_buffer((w, h), fmt, 32)
            .map_err(|e| format!("create dumb {w}x{h}: {e}"))?;
        let fb = card
            .add_framebuffer(&db, 24, 32)
            .map_err(|e| format!("add fb: {e}"))?;
        let stride = db.pitch();
        log::info!("drm dumb {w}x{h} stride={stride}");

        let mut out = DrmOutput {
            card,
            db,
            fb,
            width: w,
            height: h,
            stride,
            mode_path: String::from("unset"),
        };
        out.modeset(&mode, con.handle(), atomic_ok)?;
        Ok(out)
    }

    fn modeset(
        &mut self,
        mode: &drm::control::Mode,
        con: connector::Handle,
        atomic_ok: bool,
    ) -> Result<(), String> {
        if atomic_ok {
            match self.atomic_modeset(mode, con) {
                Ok(()) => {
                    self.mode_path = String::from("atomic");
                    log::info!("drm modeset path=atomic");
                    return Ok(());
                }
                Err(e) => log::warn!("drm atomic modeset failed ({e}); falling back to legacy"),
            }
        }
        let res = self
            .card
            .resource_handles()
            .map_err(|e| format!("resources: {e}"))?;
        let crtc_h = res.crtcs().first().copied().ok_or("no crtc")?;
        self.card
            .set_crtc(crtc_h, Some(self.fb), (0, 0), &[con], Some(*mode))
            .map_err(|e| format!("legacy set_crtc: {e}"))?;
        self.mode_path = String::from("legacy");
        log::info!("drm modeset path=legacy");
        Ok(())
    }

    fn atomic_modeset(
        &self,
        mode: &drm::control::Mode,
        con: connector::Handle,
    ) -> Result<(), String> {
        use drm::control::{atomic, property};
        let res = self
            .card
            .resource_handles()
            .map_err(|e| format!("resources: {e}"))?;
        let crtc_h = res.crtcs().first().copied().ok_or("no crtc")?;
        let planes = self.card.plane_handles().map_err(|e| format!("planes: {e}"))?;
        let mut primary: Option<drm::control::plane::Handle> = None;
        for p in &planes {
            let info = self
                .card
                .get_plane(*p)
                .map_err(|e| format!("plane info: {e}"))?;
            if !res.filter_crtcs(info.possible_crtcs()).contains(&crtc_h) {
                continue;
            }
            let props = self
                .card
                .get_properties(*p)
                .map_err(|e| format!("plane props: {e}"))?;
            let mut is_primary = false;
            for (id, val) in props.iter() {
                let pinfo = self
                    .card
                    .get_property(*id)
                    .map_err(|e| format!("prop info: {e}"))?;
                if pinfo.name().to_str().map(|x| x == "type").unwrap_or(false)
                    && *val == (drm::control::PlaneType::Primary as u64).into()
                {
                    is_primary = true;
                }
            }
            if is_primary {
                primary = Some(*p);
                break;
            }
        }
        let plane = primary.ok_or("no primary plane for crtc")?;
        let con_props = self
            .card
            .get_properties(con)
            .map_err(|e| format!("con props: {e}"))?
            .as_hashmap(&self.card)
            .map_err(|e| format!("con props map: {e}"))?;
        let crtc_props = self
            .card
            .get_properties(crtc_h)
            .map_err(|e| format!("crtc props: {e}"))?
            .as_hashmap(&self.card)
            .map_err(|e| format!("crtc props map: {e}"))?;
        let plane_props = self
            .card
            .get_properties(plane)
            .map_err(|e| format!("plane props: {e}"))?
            .as_hashmap(&self.card)
            .map_err(|e| format!("plane props map: {e}"))?;
        let blob = self
            .card
            .create_property_blob(mode)
            .map_err(|e| format!("blob: {e}"))?;
        let mut req = atomic::AtomicModeReq::new();
        req.add_property(
            con,
            con_props["CRTC_ID"].handle(),
            property::Value::CRTC(Some(crtc_h)),
        );
        req.add_property(crtc_h, crtc_props["MODE_ID"].handle(), blob);
        req.add_property(
            crtc_h,
            crtc_props["ACTIVE"].handle(),
            property::Value::Boolean(true),
        );
        req.add_property(
            plane,
            plane_props["FB_ID"].handle(),
            property::Value::Framebuffer(Some(self.fb)),
        );
        req.add_property(
            plane,
            plane_props["CRTC_ID"].handle(),
            property::Value::CRTC(Some(crtc_h)),
        );
        req.add_property(plane, plane_props["SRC_X"].handle(), property::Value::UnsignedRange(0));
        req.add_property(plane, plane_props["SRC_Y"].handle(), property::Value::UnsignedRange(0));
        let (mw, mh) = mode.size();
        req.add_property(
            plane,
            plane_props["SRC_W"].handle(),
            property::Value::UnsignedRange((mw as u64) << 16),
        );
        req.add_property(
            plane,
            plane_props["SRC_H"].handle(),
            property::Value::UnsignedRange((mh as u64) << 16),
        );
        req.add_property(
            plane,
            plane_props["CRTC_X"].handle(),
            property::Value::SignedRange(0),
        );
        req.add_property(
            plane,
            plane_props["CRTC_Y"].handle(),
            property::Value::SignedRange(0),
        );
        req.add_property(
            plane,
            plane_props["CRTC_W"].handle(),
            property::Value::UnsignedRange(mw as u64),
        );
        req.add_property(
            plane,
            plane_props["CRTC_H"].handle(),
            property::Value::UnsignedRange(mh as u64),
        );
        self.card
            .atomic_commit(AtomicCommitFlags::ALLOW_MODESET, req)
            .map_err(|e| format!("atomic commit: {e}"))?;
        Ok(())
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn mode_path(&self) -> &str {
        &self.mode_path
    }

    pub fn fill_solid(&mut self, r: u8, g: u8, b: u8) -> Result<(), String> {
        let stride = self.stride as usize;
        let w = self.width as usize;
        let h = self.height as usize;
        let mut map = self
            .card
            .map_dumb_buffer(&mut self.db)
            .map_err(|e| format!("map: {e}"))?;
        for y in 0..h {
            for x in 0..w {
                let o = y * stride + x * 4;
                map[o] = b;
                map[o + 1] = g;
                map[o + 2] = r;
                map[o + 3] = 0xFF;
            }
        }
        drop(map);
        self.dirty()
    }

    pub fn present_pixels(&mut self, pixels: &[u8], pw: u32, ph: u32) -> Result<(), String> {
        let stride = self.stride as usize;
        let w = (pw.min(self.width)) as usize;
        let h = (ph.min(self.height)) as usize;
        let src_stride = pw as usize * 4;
        let mut map = self
            .card
            .map_dumb_buffer(&mut self.db)
            .map_err(|e| format!("map: {e}"))?;
        for y in 0..h {
            let src_o = y * src_stride;
            let dst_o = y * stride;
            map[dst_o..dst_o + w * 4].copy_from_slice(&pixels[src_o..src_o + w * 4]);
        }
        drop(map);
        self.dirty()
    }

    fn dirty(&self) -> Result<(), String> {
        let rect = drm::control::ClipRect::new(0, 0, self.width as u16, self.height as u16);
        match self.card.dirty_framebuffer(self.fb, &[rect]) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => Ok(()),
            Err(e) => Err(format!("dirty fb: {e}")),
        }
    }
}

impl Drop for DrmOutput {
    fn drop(&mut self) {
        let _ = self.card.destroy_framebuffer(self.fb);
        let _ = self.card.destroy_dumb_buffer(self.db);
    }
}

pub fn drm_proof(device: &Path, width: u32, height: u32, shutdown: &crate::shutdown::Shutdown) -> i32 {
    match DrmOutput::open(device, width, height) {
        Ok(mut out) => {
            let (w, h) = out.size();
            if out.fill_solid(0x1A, 0x99, 0xCC).is_err() {
                eprintln!("drm-proof: fill failed");
                return 1;
            }
            println!("DRM_PROOF_OK size={w}x{h} path={}", out.mode_path());
            while shutdown.signal().is_none() {
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            0
        }
        Err(e) => {
            eprintln!("drm-proof failed: {e}");
            1
        }
    }
}
