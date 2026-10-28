mod input;
mod output;
mod selection;
mod server;
mod shell;
mod shm;
mod surface;

use std::path::PathBuf;

fn usage() -> &'static str {
    "usage: tinyland [--drm-proof] [--wayland-pixels] [--smoke] [--drm-device PATH] [--width N] [--height N]"
}

fn main() {
    env_logger::init();
    let mut args = std::env::args().skip(1);
    let mut mode = "smoke";
    let mut drm_device: PathBuf = PathBuf::from("/dev/dri/card0");
    let mut width: u32 = 1024;
    let mut height: u32 = 768;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--drm-proof" => mode = "drm-proof",
            "--wayland-pixels" => mode = "wayland-pixels",
            "--smoke" => mode = "smoke",
            "--drm-device" => {
                drm_device = PathBuf::from(args.next().expect("missing --drm-device value"));
            }
            "--width" => {
                width = args.next().expect("missing --width value").parse().expect("bad width");
            }
            "--height" => {
                height = args.next().expect("missing --height value").parse().expect("bad height");
            }
            "--help" | "-h" => {
                println!("{}", usage());
                return;
            }
            other => {
                eprintln!("unknown arg: {other}\n{}", usage());
                std::process::exit(2);
            }
        }
    }

    let code = match mode {
        "drm-proof" => output::drm_proof(&drm_device, width, height),
        "wayland-pixels" | "smoke" => server::run(mode, &drm_device, width, height),
        _ => {
            eprintln!("unknown mode");
            1
        }
    };
    std::process::exit(code);
}
