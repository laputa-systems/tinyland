# tinyland agents

Minimal Rust Wayland kiosk: maximized toplevels stacked newest on top,
positioned popups, keyboard and pointer, software compose, DRM scanout,
with a separate privileged seat broker.

Commands: `cargo check --locked`, `cargo test --locked`, `make audit`,
and `make proof` for the disposable x86_64 QEMU seat proof.

Invariants: Linux-only single-threaded poll loop with no async runtime.
Pure-Rust `wayland-server` backend only, never Smithay, wlroots, libwayland, libdrm, Mesa, EGL, GBM, Pixman, systemd, D-Bus, or XWayland.
Try atomic modeset then legacy fallback only on ENOSYS/EOPNOTSUPP (EINVAL also allows fallback at capability stage); both paths share page-flip presentation and must show identical pixels.
Client protocol violations disconnect with an error, internal invariant failures assert, never panic on client input.
Validate shm offsets, strides, sizes, and object lifetimes before use, copy shm at commit, and never let scanout reference client-writable memory.
Fire frame callbacks only on confirmed kernel flip and complete nothing on a failed flip.
The QEMU harness sends input as one 14-event QMP transaction, exits 75 on
an uncertain outcome, and never resends.
Exit 2 on bad CLI before opening devices, and shut down on HUP/INT/TERM/QUIT while reaping foot.
Keep `src/` free of `unsafe`; the seat client in `src/seat.rs` speaks the
seatd protocol through the shared `../seatd-protocol` crate. Runtime keyboard
handling uses KBVM; libxkbcommon is confined to the optional test oracle.
Treat guest-only root on `/dev/dri` and evdev as test harness, not a production seat claim.

Map: [runtime and presentation](docs/session.md),
[builds and verification](docs/verification.md), and [open defects](../TODO.md).
Entry points are `src/main.rs`, `src/server.rs`, `src/output.rs`, `src/input.rs`,
`harness/seat-proof.sh`, `guest/seat-proof`, and
`guest/test-app`.
