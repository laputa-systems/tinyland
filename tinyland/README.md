# Tinyland compositor

Tinyland scans out Wayland client pixels through DRM and takes keyboard and
pointer input. Each toplevel fills the output. The newest mapped toplevel gets
focus, and closing it returns to the previous one. Positioned popups compose
above their parent. The native Rust seatd client brokers device access for an
unprivileged compositor and fails closed when the broker disappears.

Run `cargo test --locked -p tinyland` from the workspace root. The optional
`headless` feature exposes the in-process server used by the WLCS adapter.
`make -C tinyland check`, `test`, and `audit` run component checks.

The retained x86_64 QEMU harness builds a disposable guest with
`/bin/bash tinyland/harness/build-guest.sh`. Run its proofs from `tinyland/`.
Qualify downstream images against their own pinned kernel, broker, and client
inputs.

```text
usage: tinyland [--seat] [--drm-proof] [--wayland-pixels] [--smoke] [--drm-device PATH] [--width N] [--height N] [--socket NAME] [-- COMMAND...]
```

Defaults are `/dev/dri/card0`, `1024x768`, and `wayland-0`. Invalid options
exit 2 before opening devices. The connector's preferred mode controls the
actual scanout size; requested dimensions are a lower bound.

Developer contracts are in [AGENTS.md](AGENTS.md), and qualification details
are in [verification.md](docs/verification.md). No compositor LICENSE file is
committed.
