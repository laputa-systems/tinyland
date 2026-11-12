# Tinyland

Tinyland is a Rust Wayland compositor with DRM scanout, software composition,
keyboard and pointer input, stacked maximized toplevels, and positioned popups.
The privileged seatd daemon runs as a separate process and brokers device file
descriptors to the unprivileged compositor through the shared seatd protocol.

This workspace is authoritative for `tinyland`, `seatd`, and `seatd-protocol`.
On Linux, `make test` runs the workspace tests in the pinned Alpine image
with its pinned XKB data. Docker and `~/.cargo/bin/kache` are required.

Run the component tests with the pinned nightly and xkeyboard-config 2.43-r0
XKB data. Set `XKB_CONFIG_ROOT` to that data directory:

```sh
XKB_CONFIG_ROOT=/path/to/xkeyboard-config-2.43-r0/xkb cargo test --locked --workspace
```

See [compositor verification](tinyland/docs/verification.md) and
[daemon validation](seatd/doc/validation.md) for device and VM qualification.
Previous guest proofs qualify their recorded inputs, not this merged workspace.

Downstream projects consume pinned commits from this repository. Submit shared
fixes, adjustments, and features here before updating downstream pins.

The daemon retains its [MIT license](seatd/LICENSE). The compositor has no
committed license declaration.
