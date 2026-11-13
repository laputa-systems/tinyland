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

## Map

- Compositor runtime and limits: [session contract](tinyland/docs/session.md).
- Compositor builds, component tests, and guest proofs: [verification](tinyland/docs/verification.md).
- Daemon operation and recovery: [runtime](seatd/doc/runtime.md) and [recovery](seatd/doc/recovery.md).
- Daemon tests and device qualification: [validation](seatd/doc/validation.md).
- Deferred fixes and draft designs: [TODO.md](TODO.md).
