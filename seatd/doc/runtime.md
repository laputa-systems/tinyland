# Run seatd

Seatd is a foreground Linux daemon for one seat, `seat0`. Run it under an
external supervisor. Seatd does not fork, authenticate users, manage logins,
or manage power or supervision policy.

## Set up socket access

Seatd listens on `/run/seatd.sock` by default. Set `SEATD_SOCK` to use another
path. The socket directory must exist and be controlled by the administrator
or supervisor.

Use `-u user` and `-g group` to set socket ownership. Socket mode is `0770`
with or without those options. Neither option can be combined with `-z`.

Socket access grants access to the daemon's seat operations. Seatd records
peer credentials but does not use them for authorization. Every connected
peer can request any supported operation, including opening supported device
classes while its session is active. Protect the socket and its directory as
the trust boundary.

## Start under supervision

To run without VT binding:

```sh
SEATD_VTBOUND=0 seatd -g seat -l info
```

The daemon logs at `info` by default. For s6 readiness, `-n fd` writes one
newline and closes an inherited, already-open descriptor after the socket is
ready. Seatd validates that descriptor before creating any of its own, and
exits with status 1 if it is not open.

Startup holds an exclusive lock on `<socket path>.lock` from path inspection
through reclamation, bind, and listen, so concurrent starts admit one owner.
A start refuses a live listener or non-socket file and reclaims a stale
socket. Use `-z` to refuse an already occupied path, including a stale
socket; the check runs under the same lock.

## Select VT behavior

`SEATD_VTBOUND` enables VT binding only when its value is exactly `1`. An
unset variable enables VT binding. Values such as `0`, `true`, or an empty
string disable it.

Use the [disposable VM harness](../harness/VM.md) to qualify transitions and
restoration for the actual binary and kernel. Never run it on host console,
GPU, or input devices. Historical results are described in [validation](validation.md).

## Device and protocol scope

Seatd accepts Linux device paths with these canonical prefixes:

- `/dev/dri/` for DRM devices.
- `/dev/input/event` for evdev devices.
- `/dev/hidraw` for hidraw devices.

It resolves paths before classifying them and checks the opened descriptor's
identity and character-device type. This is a single-seat implementation; it
always creates `seat0`.

The daemon retains the libseat seatd wire protocol. The four-byte header and
integer fields use native byte order. Unix socket descriptor passing uses
`SCM_RIGHTS` as before; the Rust port does not change the FD wire format.

Per connection, the input and output byte buffers are each 256 bytes, and the
received and queued descriptor buffers each hold up to 64 file descriptors.
The daemon supports up to 128 devices for one client. The wire path limit is
256 bytes including NUL, but the whole-frame parser currently limits usable
path fields to 250 bytes. The remaining six bytes hold the header and length.
Longer otherwise-valid frames disconnect the client; see the workspace backlog.

## Revocation failure semantics

Failed revocation is logged without clearing the device's active flag, and
handoff continues. A disable notification therefore does not prove that the
old client's descriptors stopped working. After a revocation error, restart
affected clients and have them close every reference and reopen their devices,
as described in [recovery](recovery.md).
`session_revoke_failure_across_full_switch` pins this contract.

## Recover after failure

Follow [recovery instructions](recovery.md) for graceful shutdown, stale
sockets, SIGKILL, and client restarts. A descriptor already passed to a client
may remain open after the daemon exits.

The [command-line reference](../man/seatd.1.scd) lists all options.
