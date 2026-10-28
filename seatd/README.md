# seatd

This repository contains a Linux Rust implementation of the seatd foreground
daemon. It serves the existing libseat seatd protocol over a Unix socket. The
daemon supports one seat, `seat0`.

## Build

The repository pins `nightly-2026-09-15` with `rust-src` and `clippy` in
`rust-toolchain.toml`. Install that toolchain and build with the locked,
offline dependency set:

```sh
cargo build --offline --locked
```

Rust builds use the `kache` rustc wrapper. Linux musl targets link with `mold`.
See [validation](doc/validation.md) for target builds and local checks.

## Run under a supervisor

Run seatd as a supervised foreground process. For a seat group named `seat`,
the following invocation disables VT binding:

```sh
SEATD_VTBOUND=0 seatd -g seat -l info
```

The daemon listens on `/run/seatd.sock` unless `SEATD_SOCK` names another path.
Socket access is the admission boundary. Every process that can connect can
request seat and supported-device operations. Seatd does not authenticate
users or manage login, power, or supervision policy. Read [runtime operation](doc/runtime.md)
before deployment.

## Reference client

The C libseat client and protocol inputs retained under `oracle/` are a
test-only compatibility oracle. The helper rebuilds the original C daemon
from Git history and builds the retained client scenario for differential
checks. They are not runtime dependencies of the Rust daemon. See [validation](doc/validation.md) for the oracle
commands and the scope of the comparisons.

VT-bound operation, virtio DRM/evdev handoff, and dwl/foot input have also
passed disposable-guest qualification. See [VM qualification](harness/VM.md)
for the tested environment and reproduction commands.

## License

The project retains the MIT license and attribution to Kenny Levinsen (2020).
The Rust daemon derives from the retained C reference implementation.
