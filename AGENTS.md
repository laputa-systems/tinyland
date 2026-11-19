# Tinyland workspace

This repository owns the compositor, the separately privileged seatd daemon,
and their shared seatd-protocol crate. Read each component's AGENTS.md before
editing it. Keep device authority in the daemon and compositor policy in
Tinyland. Broker loss fails closed; there is no direct-device fallback.

Run `cargo test --locked --workspace` with the pinned nightly,
`RUSTC_WRAPPER=$HOME/.cargo/bin/kache`, and `XKB_CONFIG_ROOT` pointing to
xkeyboard-config 2.43-r0 data. `make test` uses the pinned Alpine image and requires Docker.
Tests do not qualify real devices, VT transitions, or downstream images.
Keep installation, supervision, and distribution policy outside the workspace.

Do not run formatters or change the pinned nightly. Use kache for Rust builds
and mold for Linux musl links. Keep scratch in ignored `.work/`.

## Verification

- Edit: a named test in the pinned image (`region_`: 0.13s execution,
  build separate). Change: `make test` (ground 17.41s warm; candidate 22.54s,
  both observed on a busy host); `make lint` candidate 18.07s. Scenario:
  adversarial and differential harnesses (3.60s and 0.20s execution at ground).
  Full: real device, VT, VM, oracle and downstream qualification are separate.
- Critical contract and test map, mutation and coverage observations, and
  unverified boundaries: [Tests pass](docs/deslop-tests.md).

- `make test` runs the locked workspace suite in pinned Alpine with Linux kache and mold.
  `make test-ci` aliases it. `make lint` runs strict workspace Clippy in the image.
  On Linux ARM64 under Docker on macOS ARM64, observed cold and warm kache runs
  took 49.69s and 17.41s; lint took 54.64s including image rebuild. The host was busy.
- Tinyland CLI and exit 2: `tinyland/tests/cli.rs`; seatd CLI flags and exit codes: none.
- Wayland socket and presentation: `tinyland/src/server/tests.rs` and its modules.
  Remote stream and API: `tinyland/tests/remote.rs`, `remote-protocol/src/lib.rs` tests.
- Seatd socket admission and daemon behavior: `seatd/harness/adversarial.rs`.
  Libseat wire, FD transfer, and protocol API: `seatd/harness/differential.rs`,
  `seatd/harness/protocol.golden`, and `seatd-protocol` tests. The transcript is finite.
- Embedded `tinyland/data/us.xkb`: `tinyland/src/input.rs` compares pinned XKB data.
  It is generated from [xkeyboard-config](https://gitlab.freedesktop.org/xkeyboard-config/xkeyboard-config) 2.43-r0;
  `tinyland/harness/regenerate-keymap.sh` regenerates it. No vendored source trees are tracked.
- Library consumers: tinyland and seatd consume seatd-protocol; tinyland and downstream bctl
  consume tinyland-remote. bctl must test candidate changes to its contract.
- Headed bctl damage workload: `docs/CAMPAIGN-DAMAGE.md`. Real device, VT, VM, oracle,
  and downstream qualification are separate: `tinyland/docs/verification.md` and
  `seatd/doc/validation.md`. The ordinary suite does not qualify them.

## Map

- Compositor runtime and limits: [session contract](tinyland/docs/session.md).
- Compositor builds, component tests, and guest proofs: [verification](tinyland/docs/verification.md).
- Daemon operation and recovery: [runtime](seatd/doc/runtime.md) and [recovery](seatd/doc/recovery.md).
- Daemon tests and device qualification: [validation](seatd/doc/validation.md).
- Deferred fixes and draft designs: [TODO.md](TODO.md).
- Active incremental compositor/DRM damage campaign: [TINY-14](docs/CAMPAIGN-DAMAGE.md); keep upstream ownership and coordinate the pinned bctl integration.
