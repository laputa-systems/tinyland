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
current real user ID. Non-0700 permissions produce the
`TINYLAND_RUNTIME_DIR_PERMS` diagnostic. Use mode 0700 for a normal session.

The compositor starts the explicit command after creating its display socket
and opening the keyboard and output. It reports `TINYLAND_COMPOSITOR_READY`
after startup. Primary-client exit ends the session with
`TINYLAND_SESSION_END`. Restart policy remains external.

## Presentation

`queue_present` writes an owned back buffer, requests a flip event, and arms
a two-second deadline. The main loop polls display, socket, DRM, and input
together. A matching CRTC event completes presentation. Queue errors and
wrong-CRTC events return failure without completing callbacks.

Submission takes the scene's dirty state and the frame callbacks of the
surfaces it shows; completion fires only that batch, so a commit that lands
while a flip is in flight stays dirty for the next frame. Suspend abandons an
in-flight frame and returns its callbacks to the scene. Direct device opens
still lack the nonblocking flag needed to keep event reads within the
deadline. See [open defects](../../TODO.md).

Pointer hit-testing walks the committed surface tree, subsurfaces included,
in the order it is drawn, and honours each surface's input region as of its
applied commit. Scale and transform limits are listed in the
[compositor usage guide](../README.md).

Each broker transaction has one deadline, five seconds by default, and is
cancelled when the compositor shuts down; a broker that stops answering ends
the session instead of blocking it.

## Keyboard data

The keyboard engine is KBVM. `src/input.rs` selects installed XKB rules or
the embedded US keymap in `data/us.xkb` and logs the choice. Its search uses
`/usr/share/X11/xkb` and `/etc/xkb`, with `XKB_CONFIG_ROOT` and
`XKB_CONFIG_EXTRA_PATH` overrides. `TINYLAND_XKB_ROOT` selects an explicit
rules directory; a nonexistent explicit directory is an error.

Regenerate the embedded map with `harness/regenerate-keymap.sh`, using
xkeyboard-config 2.43-r0. The installed binary must run without a source
checkout. It requires no libseat or libxkbcommon runtime library.

## Input and device ownership

Grabbed keyboards hide Ctrl+Alt+Fn from the console, so the compositor forwards
it to the broker. Delivered keys and buttons are owned per input device:
losing a device releases what it held, and suspend releases everything and
resets the lock state, so clients never keep a stuck key or pointer grab.
Revoked evdev descriptors must be reopened; removal must also close the
broker's cached device ID. A disable that arrives while recovering from
ENODEV suspends the seat instead of treating the device as unplugged.

Suspend and resume write best-effort markers named `laputa-seat-disabled`
and `laputa-seat-enabled` inside the runtime directory for guest observers.
Marker failure does not fail the session.
