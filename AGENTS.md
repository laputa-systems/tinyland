# Tinyland workspace

This repository owns the compositor, the separately privileged seatd daemon,
and their shared seatd-protocol crate. Read each component's AGENTS.md before
editing it. Keep device authority in the daemon and compositor policy in
Tinyland. Broker loss fails closed; there is no direct-device fallback.

Run `cargo test --locked --workspace` with the pinned nightly and
`XKB_CONFIG_ROOT` pointing to xkeyboard-config 2.43-r0 data. Tests do not
qualify real devices, VT transitions, or downstream images.
Keep installation, supervision, and distribution policy outside the workspace.

Do not run formatters or change the pinned nightly. Use kache for Rust builds
and mold for Linux musl links. Keep scratch in ignored `.work/`.
