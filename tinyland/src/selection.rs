use std::collections::HashMap;
use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::time::{Duration, Instant};
use wayland_server::backend::ObjectId;
use wayland_server::protocol::{wl_data_device, wl_data_source};

const MAX_TRANSFERS: usize = 16;
const TRANSFER_DEADLINE: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub(crate) struct DataSourceData {
    pub resource: wl_data_source::WlDataSource,
    pub mime_types: Vec<String>,
}

#[derive(Debug)]
pub(crate) enum Offer {
    Client(wl_data_source::WlDataSource),
    Host(Arc<[u8]>),
}

#[derive(Debug)]
pub(crate) struct Transfer {
    offer: ObjectId,
    fd: OwnedFd,
    bytes: Arc<[u8]>,
    written: usize,
    pub(crate) deadline: Instant,
}

#[derive(Debug, Default)]
pub(crate) struct SelectionState {
    pub sources: HashMap<ObjectId, DataSourceData>,
    pub devices: HashMap<ObjectId, wl_data_device::WlDataDevice>,
    pub offers: HashMap<ObjectId, Offer>,
    pub selection: Option<ObjectId>,
    pub host: Option<Arc<[u8]>>,
    pub transfers: Vec<Transfer>,
}

impl SelectionState {
    pub(crate) fn replace(&mut self, next: Option<ObjectId>) {
        if let Some(id) = self.selection.take() {
            if let Some(old) = self.sources.get(&id) { old.resource.cancelled(); }
        }
        self.offers.retain(|_, offer| matches!(offer, Offer::Client(_)));
        self.transfers.clear();
        self.host = None;
        self.selection = next;
    }

    pub(crate) fn receive_host(&mut self, offer: ObjectId, fd: OwnedFd, bytes: Arc<[u8]>) {
        if self.transfers.len() >= MAX_TRANSFERS { return; }
        let Ok(stat) = rustix::fs::fstat(&fd) else { return; };
        if rustix::fs::FileType::from_raw_mode(stat.st_mode) != rustix::fs::FileType::Fifo { return; }
        let Ok(flags) = rustix::fs::fcntl_getfl(&fd) else { return; };
        if !flags.intersects(rustix::fs::OFlags::WRONLY | rustix::fs::OFlags::RDWR) { return; }
        if rustix::fs::fcntl_setfl(&fd, flags | rustix::fs::OFlags::NONBLOCK).is_err() { return; }
        if rustix::io::fcntl_setfd(&fd, rustix::io::FdFlags::CLOEXEC).is_err() { return; }
        self.transfers.push(Transfer { offer, fd, bytes, written: 0, deadline: Instant::now() + TRANSFER_DEADLINE });
    }

    pub(crate) fn poll_fds(&self) -> Vec<rustix::event::PollFd<'_>> {
        self.transfers.iter().map(|transfer| rustix::event::PollFd::new(&transfer.fd, rustix::event::PollFlags::OUT)).collect()
    }

    pub(crate) fn service_transfers(&mut self) {
        let now = Instant::now();
        self.transfers.retain_mut(|transfer| {
            if now >= transfer.deadline { return false; }
            while transfer.written < transfer.bytes.len() {
                match rustix::io::write(&transfer.fd, &transfer.bytes[transfer.written..]) {
                    Ok(0) => return false,
                    Ok(n) => transfer.written += n,
                    Err(rustix::io::Errno::AGAIN) => return true,
                    Err(rustix::io::Errno::INTR) => return true,
                    Err(_) => return false,
                }
            }
            false
        });
    }

    pub(crate) fn remove_offer(&mut self, id: &ObjectId) {
        self.offers.remove(id);
        self.transfers.retain(|transfer| &transfer.offer != id);
    }
}
