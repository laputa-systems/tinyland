use crate::admit;
use crate::r#loop::Server;
use crate::{log_error, log_info};

pub fn shutdown(server: Server, path: &str) {
    if let Err(e) = admit::unlink_path(path) {
        log_error!("Could not remove socket: {e}");
    }
    server.finish();
    log_info!("seatd stopped");
}
