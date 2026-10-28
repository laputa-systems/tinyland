# Crash recovery

## Graceful shutdown (SIGINT/SIGTERM)

The daemon stops admission, destroys all sessions (closing every
daemon-held device descriptor), unlinks its socket, and exits 0.
A supervisor can then start a replacement on the same path.

## Daemon SIGKILL

SIGKILL cannot be caught. What is true afterwards:

- The socket path is left behind. The next start unlinks the stale
  socket and serves (same as the C daemon).
- Device descriptors the daemon held are closed by the kernel.
- Descriptors already passed to clients via `SCM_RIGHTS` stay open
  in those clients, but their authority is whatever the last
  deactivation left:
  - DRM: dropping the daemon's master releases master. A client
    holding a DRM descriptor keeps its render context but has no
    master until it reopens through a new daemon (which calls
    `drm_set_master` again).
  - evdev/hidraw: `EVIOCREVOKE`/`HIDIOCREVOKE` persist on the open
    file description. Revoked descriptors are dead; the client must
    close and reopen them after reconnecting.
- There is no transparent recovery. After any daemon death:

  1. Terminate seat clients (they cannot regain a working seat on
     the old connection; the daemon side is gone).
  2. Start a new daemon on the same socket path.
  3. Reconnect every client and reopen every device.

## Client crash

A client that exits without `close_seat` is reaped on disconnect:
its devices are closed and deactivated, its session is removed,
and a waiting session is activated. No operator action is needed.

## VT-bound seats

A replacement daemon re-reads the current VT on the next client
open or `SIGUSR2`. Kernel VT modes are per-descriptor; a dead
daemon's descriptors are closed, so the replacement must open the
VT again (normal open path does this). If the previous daemon died
mid-switch, send `SIGUSR2` after clients reconnect to resync.
Real-VT qualification is environment-gated (see plan N1).
