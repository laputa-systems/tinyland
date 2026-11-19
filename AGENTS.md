# Tinyland workspace

This repository owns the compositor, the separately privileged seatd daemon,
and their shared seatd-protocol crate. Read each component's AGENTS.md before
editing it. Keep device authority in the daemon and compositor policy in
Tinyland. Broker loss fails closed; there is no direct-device fallback.

Run `cargo test --locked --workspace` with the pinned nightly,
`RUSTC_WRAPPER=$HOME/.cargo/bin/kache`, and `XKB_CONFIG_ROOT` pointing to
xkeyboard-config 2.43-r0 data. On Linux, `make test` uses the pinned Alpine
image and requires Docker and `~/.cargo/bin/kache`.
Tests do not qualify real devices, VT transitions, or downstream images.
Keep installation, supervision, and distribution policy outside the workspace.

Do not run formatters or change the pinned nightly. Use kache for Rust builds
and mold for Linux musl links. Keep scratch in ignored `.work/`.

## Verification

- `make test` builds the pinned image and mounts Linux host kache; `make test-ci`
  runs the same locked workspace suite without the mount. CI uses `make test-ci`.
- The kache mount expects a Linux binary. Cold and warm costs remain open.
  See `TODO-DESLOP.md` for the baseline and the missing lint target.
- Tinyland CLI and exit 2: `tinyland/tests/cli.rs`; Wayland and presentation:
  `tinyland/src/server/tests.rs` and its test modules; remote stream:
  `tinyland/tests/remote.rs` and `remote-protocol/src/lib.rs` tests.
- seatd CLI flags and exit codes: no dedicated CLI test. Socket admission and
  daemon behavior: `seatd/harness/adversarial.rs`; libseat wire and FD transfer:
  `seatd/harness/differential.rs`, `seatd/harness/protocol.golden`, and
  `seatd-protocol` tests. The transcript is finite, not universal compatibility.
- Embedded keymap `tinyland/data/us.xkb` is checked against pinned XKB data by
  `tinyland/src/input.rs`; regeneration is ignored in the ordinary suite.
- The headed bctl damage workload and pending device gates are described in
  `docs/CAMPAIGN-DAMAGE.md`; it is relevant to composition and presentation.
  bctl is a downstream consumer; test it against candidate changes to its contract.
- VM, real device, VT, oracle, and downstream appliance proofs are separate;
  see `tinyland/docs/verification.md` and `seatd/doc/validation.md`.

## Map

- Compositor runtime and limits: [session contract](tinyland/docs/session.md).
- Compositor builds, component tests, and guest proofs: [verification](tinyland/docs/verification.md).
- Daemon operation and recovery: [runtime](seatd/doc/runtime.md) and [recovery](seatd/doc/recovery.md).
- Daemon tests and device qualification: [validation](seatd/doc/validation.md).
- Deferred fixes and draft designs: [TODO.md](TODO.md).
- Active incremental compositor/DRM damage campaign: [TINY-14](docs/CAMPAIGN-DAMAGE.md); keep upstream ownership and coordinate the pinned bctl integration.
