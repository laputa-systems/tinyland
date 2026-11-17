use tinyland::{output, server, shutdown};
use std::ffi::OsString;
use std::path::PathBuf;
fn usage() -> &'static str {
    "usage: tinyland [--remote] [--seat] [--drm-proof] [--wayland-pixels] [--smoke] [--drm-device PATH] [--width N] [--height N] [--socket NAME] [-- COMMAND...]"
}

fn argument_error(message: &str) -> ! {
    eprintln!("{message}\n{}", usage());
    std::process::exit(2);
}

fn main() {
    env_logger::init();
    let shutdown = match shutdown::Shutdown::new() {
        Ok(shutdown) => shutdown,
        Err(error) => { eprintln!("signal setup: {error}"); std::process::exit(1); }
    };
    let mut args = std::env::args_os().skip(1);
    let mut mode = "smoke";
    let mut drm_device: PathBuf = PathBuf::from("/dev/dri/card0");
    let mut width: u32 = 1024;
    let mut height: u32 = 768;
    let mut socket_name = OsString::from("wayland-0");
    let mut command: Vec<OsString> = Vec::new();
    let mut seat_mode = false;
    let mut remote = false;
    let mut explicit_physical = false;
    let mut separator = false;
    while let Some(a) = args.next() {
        if a == "--" {
            separator = true;
            for rest in args.by_ref() {
                command.push(rest);
            }
            break;
        }
        let text = a.to_str().unwrap_or_else(|| argument_error("option must be UTF-8")).to_string();
        match text.as_str() {
            "--drm-proof" => { mode = "drm-proof"; explicit_physical = true; },
            "--wayland-pixels" => { mode = "wayland-pixels"; explicit_physical = true; },
            "--smoke" => { mode = "smoke"; explicit_physical = true; },
            "--seat" => seat_mode = true,
            "--remote" => remote = true,
            "--drm-device" => {
                explicit_physical = true;
                drm_device = PathBuf::from(args.next().unwrap_or_else(|| argument_error("missing --drm-device value")));
            }
            "--width" => {
                width = args.next().and_then(|value| value.to_str().and_then(|value| value.parse().ok())).unwrap_or_else(|| argument_error("invalid --width: expected unsigned integer"));
            }
            "--height" => {
                height = args.next().and_then(|value| value.to_str().and_then(|value| value.parse().ok())).unwrap_or_else(|| argument_error("invalid --height: expected unsigned integer"));
            }
            "--socket" => {
                socket_name = args.next().unwrap_or_else(|| argument_error("missing --socket value"));
            }
            "--help" | "-h" => {
                println!("{}", usage());
                return;
            }
            other if other.starts_with("--") => {
                eprintln!("unknown arg: {other}\n{}", usage());
                std::process::exit(2);
            }
            _ => {
                command.push(OsString::from(text));
                for rest in args.by_ref() {
                    command.push(rest);
                }
                break;
            }
        }
    }

    if remote && (seat_mode || explicit_physical || !separator || command.is_empty() || tinyland_remote::Size::new(width, height).is_none()) {
        argument_error("invalid remote mode options");
    }
    let code = if remote {
        server::remote::run(&server::ServerConfig { mode, device: &drm_device, width, height, socket_name: &socket_name, command: &command, shutdown: &shutdown, seat_mode })
    } else { match mode {
        "drm-proof" => output::drm_proof(&drm_device, width, height, &shutdown),
        "wayland-pixels" | "smoke" => server::run(&server::ServerConfig {
            mode,
            device: &drm_device,
            width,
            height,
            socket_name: &socket_name,
            command: &command,
            shutdown: &shutdown,
            seat_mode,
        }),
        _ => {
            eprintln!("unknown mode");
            1
        }
    }};
    let signal = shutdown.signal();
    drop(shutdown);
    if let Some(signal) = signal {
        if let Err(error) = signal_hook::low_level::emulate_default_handler(signal) {
            eprintln!("restore signal {signal}: {error}");
        }
        std::process::exit(128 + signal);
    }
    std::process::exit(code);
}
