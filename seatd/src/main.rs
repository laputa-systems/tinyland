use seatd::admit;
use seatd::cli;
use seatd::log;
use seatd::r#loop::Server;
use seatd::readiness::Readiness;
use seatd::shutdown;
use seatd::{log_error, log_info};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    let prog = std::env::args().next().unwrap_or_else(|| "seatd".to_string());
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let opts = match cli::parse(&argv, &prog) {
        Ok(o) => o,
        Err(code) => std::process::exit(code),
    };
    if opts.show_help {
        print!("{}", cli::USAGE);
        return;
    }
    if opts.show_version {
        println!("seatd version {VERSION}");
        return;
    }
    log::set_level(opts.level);

    let path = admit::socket_path();

    let readiness = match opts.readiness {
        Some(fd) => match Readiness::acquire(fd) {
            Ok(readiness) => Some(readiness),
            Err(e) => {
                log_error!("Invalid readiness descriptor {fd}: {e}");
                std::process::exit(1);
            }
        },
        None => None,
    };

    let mut server = match Server::new() {
        Ok(s) => s,
        Err(e) => {
            log_error!("server_init failed: {e}");
            std::process::exit(1);
        }
    };

    let socket_fd = match admit::open_socket(&path, opts.existing_socket, opts.uid, opts.gid) {
        Ok(fd) => fd,
        Err(e) => {
            log_error!("Could not create server socket: {e}");
            server.finish();
            std::process::exit(1);
        }
    };
    server.set_listen(socket_fd);
    let socket_path = match shutdown::SocketPath::capture(&path) {
        Ok(socket) => socket,
        Err(e) => {
            log_error!("Could not identify bound socket: {e}");
            server.finish();
            std::process::exit(1);
        }
    };

    log_info!("seatd started");

    if let Some(readiness) = readiness {
        readiness.notify();
    }

    let result = server.run();

    shutdown::shutdown(server, socket_path);
    if let Err(e) = result {
        log_error!("Poller failed: {e}");
        std::process::exit(1);
    }
}
