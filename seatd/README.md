# seatd

This workspace contains a Linux Rust implementation of the seatd foreground
daemon. It serves the existing libseat seatd protocol over a Unix socket. The
daemon supports one seat, `seat0`.

## Build

The repository pins `nightly-2026-09-15` with `rust-src` and `clippy` in
`rust-toolchain.toml`. Install that toolchain and build with the locked,
offline dependency set:

```sh
cargo build --offline --locked -p seatd
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

## Protocol compatibility

The daemon speaks the libseat seatd protocol over the socket: open and
close seats and devices, enable and disable sessions, switch sessions,
ping. The opcodes, framing, and SCM_RIGHTS descriptor passing live in the
shared `../seatd-protocol` crate; tinyland's native client uses the same
crate, so client and server cannot drift. The exact byte sequences are
pinned in [harness/protocol.golden](harness/protocol.golden) and exercised
by a raw-protocol guest client with no C dependencies. Read
[validation](doc/validation.md) for what the checks cover.

VT-bound operation, virtio DRM/evdev handoff, and compositor input were
qualified on Laputa's disposable x86_64 guest. The retained daemon harness is
in `harness/`. Consumer image proofs remain in Laputa and bctl. See
[../tinyland/docs/verification.md](../tinyland/docs/verification.md).

## License

The project retains the MIT license and attribution to Kenny Levinsen (2020).
