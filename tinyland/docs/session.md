# Compositor session contract

Run Tinyland as an unprivileged user with `--seat` and an explicit
`-- COMMAND...`. Set `LIBSEAT_BACKEND=seatd` and start the seatd daemon
before the session. Device discovery, rendering, and input belong to the
compositor. Device authority belongs to the separately privileged daemon.
Installation, supervision, restart, and login policy belong to the caller.

## Broker connection

The native client in `src/seat.rs` uses the shared `seatd-protocol` crate.
`LIBSEAT_BACKEND=seatd` is required before connecting. `SEATD_SOCK` selects
the socket, defaulting to `/run/seatd.sock`. Connection or dispatch failure
terminates the brokered session. Broker loss never falls back to direct open.

Device descriptors arrive through SCM_RIGHTS and are owned by their callers.
The session tracks device IDs and retains the broker connection. On disable,
release scanout and input descriptors. On reactivation, reopen devices and
reset keyboard modifiers before delivering input.

## Client and runtime directory

`XDG_RUNTIME_DIR` must name an existing absolute directory owned by the
current effective user. Non-0700 permissions produce the
`TINYLAND_RUNTIME_DIR_PERMS` diagnostic. Use mode 0700 for a normal session.

The compositor starts the explicit command after creating its display socket
and opening the keyboard and output. It reports `TINYLAND_COMPOSITOR_READY`
after startup. Primary-client exit ends the session with
`TINYLAND_SESSION_END`. Restart policy remains external.

## Presentation

`queue_present` writes the back buffer, requests a flip event, and arms a
two-second deadline. The main loop polls display, socket, DRM, and input
together. A matching CRTC event completes presentation and then frame
callbacks. Timeout, failed presentation, and a wrong CRTC fail the operation.
Disable and shutdown remain responsive while a flip is pending.

## Keyboard data

The keyboard engine is KBVM. Installed XKB data under `/usr/share/X11/xkb`
is used when `rules/evdev` is available. Otherwise Tinyland uses the embedded
US keymap in `data/us.xkb`. The selected source is logged.
`TINYLAND_XKB_ROOT` and `XKB_CONFIG_ROOT` override the data location.
A nonexistent explicit `TINYLAND_XKB_ROOT` is an error.

Regenerate the embedded map with `harness/regenerate-keymap.sh`, using
xkeyboard-config 2.43-r0. The installed binary must run without a source
checkout. It requires no libseat or libxkbcommon runtime library.
