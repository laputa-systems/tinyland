# Tinyland

Tinyland is a Rust Wayland compositor with DRM scanout, software composition,
keyboard and pointer input, stacked maximized toplevels, and positioned popups.
The privileged seatd daemon runs as a separate process and brokers device file
descriptors to the unprivileged compositor through the shared seatd protocol.

See [running the compositor](tinyland/README.md) and
[running the daemon](seatd/README.md). Build and verification commands are in
[AGENTS.md](AGENTS.md), with separate component and device checks.

Downstream projects consume pinned commits from this repository. Submit shared
fixes, adjustments, and features here before updating downstream pins.

The daemon retains its [MIT license](seatd/LICENSE). The compositor has no
committed license declaration. Open defects are tracked in [TODO.md](TODO.md).
