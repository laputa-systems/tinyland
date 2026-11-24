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

## Remote session

`tinyland --remote --width W --height H --socket NAME -- COMMAND...` runs a
software output without opening a seat, DRM, or input device. Width and height
must each be between 1 and 4096. The caller connects stdin and stdout to the
trusted host transport; diagnostics use stderr. Remote mode cannot be combined
with seat or DRM options. Input source identifiers own held keys and buttons;
resetting a source releases its ownership. Pointer coordinates are output
pixels and wheel values are signed whole steps, bounded to 120 per message.
Transport closure, malformed input, an unexpected acknowledgment, or a stalled
frame terminates the session. The child is terminated and reaped on shutdown.

The `tinyland-remote` workspace crate defines the wire codec. Each message has
`TLR1`, a one-byte type, and a little-endian u32 body length. Bodies use
little-endian integers. Types 1 through 4 are Ready (u32 width, u32 height),
Frame (u64 serial, pixels), Damage (u64 serial, u32 count, then count
rectangles of u32 x, y, width, height, then each rectangle's pixel rows in
order), TiledDamage, and Cursor (type 5: u32 hotspot x, hotspot y, width, height, followed by premultiplied BGRA pixel rows). Cursor edges are at most 256 pixels; a zero width and height with zero hotspot and no pixels hides it. The host draws the cursor locally and it is absent from remote frame pixels. Cursor messages need no acknowledgment. Types 16 through 22 are Presented (u64 serial),
Key (u32 source, u16 evdev code, u8 pressed), Pointer (u32 source, u32 x,
u32 y), Button (u32 source, u8 button, u8 pressed), Wheel (u32 source, i32
horizontal, i32 vertical), ResetInput (u32 source), and Clipboard (u32 source,
UTF-8 bytes). Button values 1 through 5 mean left, right, middle, back, forward.
Boolean values are 0 or 1. Clipboard is UTF-8 bounded to 1 MiB and replaces the
current selection with a host-owned text offer. The latest clipboard survives
a viewer input reset until superseded or the session ends. Replacement cancels
old host offers and transfers. At most 16 nonblocking transfers run at once;
each expires after five seconds and receiver closure cancels it. Host transfers
accept writable pipe descriptors and set the transferred write description to
nonblocking. A client retaining a duplicate of that write description observes
the changed flag; the read endpoint keeps its own flags. Other descriptors cancel
the transfer. This path requires no proc filesystem.

Output writes are bounded to 64 KiB: transports such as Hydrogen vsock allocate
a packet buffer for each write and cannot accept a whole framebuffer at once.
Short writes and backpressure preserve the remaining encoded bytes.

Frames contain exactly width times height times four bytes in XRGB8888
little-endian order, B,G,R,X with a width times four stride. Damage replaces
only its rectangles of the previous output, in the same pixel order with each
rectangle's width times four stride. Rectangles are nonempty, lie inside the
output, and number at most 64. A Damage message with no rectangles changes no
pixels but still completes a presentation.

TiledDamage carries the same rectangles more compactly. After the rectangle
table, each rectangle is cut into 64 by 64 tiles from its own origin, the last
row and column clipped to the rectangle. Its tiles follow in row order, each a
kind byte: 0 for a tile sent raw, or 1 followed by the one B,G,R,X pixel that
every pixel of the tile equals. Then come the rectangle's raw pixels in row
order, leaving out the pixels inside solid tiles. The body length must account
for every byte. Readers return a TiledDamage message as Damage. The compositor
sends TiledDamage for every frame; its first frame covers the whole output. The host sends
Presented only after publishing the validated framebuffer, independent of
viewer presence.

The compositor keeps the presented output and the scene it showed. Commits
retain each surface's Damage and DamageBuffer requests; a commit replaces only
the damaged rows of the surface's retained pixels. A new buffer with no posted
damage, a changed size or format, or a buffer scale or transform the renderer
ignores counts as damage to the whole surface. At presentation, a surface that
kept its place contributes its accumulated damage; a surface that moved,
appeared, disappeared or changed size contributes its old and new extents; a
changed drawing order redraws the whole output. Each damaged output rectangle
is cleared and recomposed from every surface that intersects it, so alpha
blending stays exact. More than 16 rectangles are sent as their bounding box.

At most one frame is in flight. The remote output behaves like a 60 Hz
display that the host samples at up to 30 Hz. A dirty scene is presented at
most once every 1/60 s: its damage is composed into the retained output and
added to the unsent damage. When no frame was sent in the last 1/30 s, the
unsent damage is sent and the presented callbacks complete on its
acknowledgment. Otherwise they complete at once, and the unsent damage is sent
when the interval ends, without callbacks. The first change after a quiet
interval is therefore sent immediately, and continuous animation reaches the
host at 30 frames a second while clients keep 60. Presentations that change no
pixels send nothing. Commits made while a frame is in flight remain dirty until
the next presentation.
`read_compositor` and `write_compositor` take `Option<Size>`: pass the
negotiated size for frames and `None` for Ready. Clean EOF is returned as
`None` only at a message boundary.
For nonblocking receivers, `host_message_size` validates a complete nine-byte
host header and returns the total encoded length before buffering the body.
