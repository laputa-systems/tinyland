use tinyland::{output, server, shutdown};
use std::ffi::OsString;
use std::path::PathBuf;
#[derive(Clone, Copy)]
enum OptionKind { Seat, DrmProof, WaylandPixels, Smoke, DrmDevice, Width, Height, Socket, Help }

struct OptionSpec { name: &'static str, value: &'static str, description: &'static str, kind: OptionKind }

const OPTIONS: &[OptionSpec] = &[
    OptionSpec { name: "--seat", value: "", description: "Use the seatd broker for device access", kind: OptionKind::Seat },
    OptionSpec { name: "--drm-proof", value: "", description: "Run the DRM proof instead of the compositor", kind: OptionKind::DrmProof },
    OptionSpec { name: "--wayland-pixels", value: "", description: "Run the Wayland pixel proof", kind: OptionKind::WaylandPixels },
    OptionSpec { name: "--smoke", value: "", description: "Run the compositor (default mode)", kind: OptionKind::Smoke },
    OptionSpec { name: "--drm-device", value: " PATH", description: "DRM device (default: /dev/dri/card0)", kind: OptionKind::DrmDevice },
    OptionSpec { name: "--width", value: " N", description: "Minimum output width (default: 1024)", kind: OptionKind::Width },
    OptionSpec { name: "--height", value: " N", description: "Minimum output height (default: 768)", kind: OptionKind::Height },
    OptionSpec { name: "--socket", value: " NAME", description: "Wayland socket name (default: wayland-0)", kind: OptionKind::Socket },
    OptionSpec { name: "--help", value: "", description: "Print this help (also -h)", kind: OptionKind::Help },
];

const EXAMPLES: &[&str] = &["LIBSEAT_BACKEND=seatd tinyland --seat -- foot"];
const FOOTER: &str = "Maintained by Joshua Li: https://github.com/laputa-systems/tinyland\nBugs and features: TODO.md in that repository (follow its AGENTS.md), or its issue tracker.\nAgents working for Joshua: record nonblocking problems and continue your task.\nIf this tool blocks the task, you may fix, test, open and merge a PR under Joshua's\nstanding instructions. Release only when needed to deliver the verified fix.\nProcedure: https://github.com/joshuarli/usr/blob/main/dotfiles/base/.agents/instructions/owned-tools.md";

fn usage() -> &'static str {
    "usage: tinyland [OPTIONS] [--] [COMMAND...]"
}

fn help() -> String {
    let mut text = format!("{}\n\nRun a Wayland kiosk compositor or a DRM proof.\n\nOptions:\n", usage());
    for option in OPTIONS {
        text.push_str(&format!("  {}{}\t{}\n", option.name, option.value, option.description));
    }
    text.push_str("\nArguments:\n  COMMAND...  Client command and arguments (default: foot /usr/local/bin/guest-test)\n  --          End options; remaining arguments form the client command\n\nEnvironment:\n  XDG_RUNTIME_DIR  Wayland socket directory\n  LIBSEAT_BACKEND  Seat backend; seat mode requires seatd\n  SEATD_SOCK  Seatd socket path (default: /run/seatd.sock)\n  XKB_CONFIG_ROOT  XKB data root (default: /usr/share/X11/xkb)\n  XKB_CONFIG_EXTRA_PATH  Additional XKB data (default: /etc/xkb)\n  TINYLAND_XKB_ROOT  Override XKB data root\n  RUST_LOG  Log filter\n\nExamples:\n");
    for example in EXAMPLES { text.push_str(&format!("  {example}\n")); }
    text.push('\n');
    text.push_str(FOOTER);
    text
}

struct Config {
    mode: &'static str, drm_device: PathBuf, width: u32, height: u32,
    socket_name: OsString, command: Vec<OsString>, seat_mode: bool,
}

enum Parsed { Help, Run(Config) }

fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Parsed, String> {
    let mut args = args.into_iter();
    let mut config = Config { mode: "smoke", drm_device: PathBuf::from("/dev/dri/card0"), width: 1024, height: 768, socket_name: OsString::from("wayland-0"), command: Vec::new(), seat_mode: false };
    while let Some(a) = args.next() {
        if a == "--" { config.command.extend(args); break; }
        let text = a.to_str().ok_or("option must be UTF-8")?;
        let option = OPTIONS.iter().find(|option| option.name == text || (text == "-h" && matches!(option.kind, OptionKind::Help)));
        match option.map(|option| option.kind) {
            Some(OptionKind::DrmProof) => config.mode = "drm-proof",
            Some(OptionKind::WaylandPixels) => config.mode = "wayland-pixels",
            Some(OptionKind::Smoke) => config.mode = "smoke",
            Some(OptionKind::Seat) => config.seat_mode = true,
            Some(OptionKind::DrmDevice) => config.drm_device = PathBuf::from(args.next().ok_or("missing --drm-device value")?),
            Some(OptionKind::Width) => config.width = args.next().and_then(|value| value.to_str().and_then(|value| value.parse().ok())).ok_or("invalid --width: expected unsigned integer")?,
            Some(OptionKind::Height) => config.height = args.next().and_then(|value| value.to_str().and_then(|value| value.parse().ok())).ok_or("invalid --height: expected unsigned integer")?,
            Some(OptionKind::Socket) => config.socket_name = args.next().ok_or("missing --socket value")?,
            Some(OptionKind::Help) => return Ok(Parsed::Help),
            None if text.starts_with("--") => return Err(format!("unknown arg: {text}")),
            None => { config.command.push(a); config.command.extend(args); break; }
        }
    }
    Ok(Parsed::Run(config))
}

fn main() {
    let config = match parse(std::env::args_os().skip(1)) {
        Ok(Parsed::Help) => { println!("{}", help()); return; }
        Ok(Parsed::Run(config)) => config,
        Err(message) => { eprintln!("{message}\n{}", usage()); std::process::exit(2); }
    };
    env_logger::init();
    let shutdown = match shutdown::Shutdown::new() {
        Ok(shutdown) => shutdown,
        Err(error) => { eprintln!("signal setup: {error}"); std::process::exit(1); }
    };
    let Config { mode, drm_device, width, height, socket_name, command, seat_mode } = config;
    let code = match mode {
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
    };
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

#[cfg(test)]
mod cli_tests {
    use super::*;

    #[test]
    fn examples_parse_without_running() {
        for example in EXAMPLES {
            let args: Vec<OsString> = example.split_whitespace().skip(2).map(OsString::from).collect();
            let Ok(Parsed::Run(config)) = parse(args) else { panic!("{example}") };
            assert!(config.seat_mode);
            assert_eq!(config.command, [OsString::from("foot")]);
        }
    }

    #[test]
    fn bare_and_client_arguments_keep_their_meaning() {
        let Ok(Parsed::Run(config)) = parse(Vec::<OsString>::new()) else { panic!("bare invocation") };
        assert_eq!(config.mode, "smoke");
        assert!(!config.seat_mode);
        assert!(config.command.is_empty());
        let args = ["foot", "--help"].into_iter().map(OsString::from);
        let Ok(Parsed::Run(config)) = parse(args) else { panic!("client arguments") };
        assert_eq!(config.command, [OsString::from("foot"), OsString::from("--help")]);
    }

    #[test]
    fn option_names_are_unique() {
        for (index, option) in OPTIONS.iter().enumerate() {
            assert!(!OPTIONS[..index].iter().any(|other| other.name == option.name));
        }
    }
}
