# Tinyland compositor

Tinyland scans out Wayland client pixels through DRM and takes keyboard and
pointer input. Each toplevel fills the output. The newest mapped toplevel gets
focus, and closing it returns to the previous one. Positioned popups compose
above their parent. The native Rust seatd client brokers device access for an
unprivileged compositor and fails closed when the broker disappears.

Start the daemon first and give your session user access to its socket.
Set `XDG_RUNTIME_DIR` to an existing directory owned by that user with mode
0700, then start an installed Wayland application:

```sh
LIBSEAT_BACKEND=seatd tinyland --seat -- foot
```

Pass an explicit command. Omitting it launches the guest-only
`foot /usr/local/bin/guest-test` command. The primary command's exit ends the
compositor session. Use `tinyland --help` for the available options.

Defaults are `/dev/dri/card0`, `1024x768`, and `wayland-0`. Invalid options
exit 2 before opening devices. The first mode returned by the selected
connector controls scanout size; requested dimensions are a lower bound.
Rendering uses copied shared-memory buffers at scale 1 with normal transform.
Non-unit scale and non-normal transform requests are currently accepted but
ignored.

See the [session contract](docs/session.md) for runtime setup and limitations.
Developer commands are in [AGENTS.md](AGENTS.md), and build and qualification
details are in [verification.md](docs/verification.md).
