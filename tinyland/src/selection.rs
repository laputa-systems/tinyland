use std::collections::HashMap;
use wayland_server::backend::ObjectId;
use wayland_server::protocol::{wl_data_device, wl_data_offer, wl_data_source};

#[derive(Debug, Default)]
pub struct DataSourceData {
    pub resource: Option<wl_data_source::WlDataSource>,
    pub mime_types: Vec<String>,
    pub dnd_actions: u32,
}

#[derive(Debug)]
pub struct DataDeviceData {
    pub resource: wl_data_device::WlDataDevice,
    pub client_offers: Vec<wl_data_offer::WlDataOffer>,
}

#[derive(Debug, Default)]
pub struct SelectionState {
    pub sources: HashMap<ObjectId, DataSourceData>,
    pub devices: HashMap<ObjectId, DataDeviceData>,
    pub selection: Option<ObjectId>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_starts_empty() {
        let s = SelectionState::default();
        assert!(s.selection.is_none());
        assert!(s.sources.is_empty());
    }
}
