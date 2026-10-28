# Verification reference

The supported proof host is Linux x86_64 with KVM. macOS and aarch64 harness
ports and proofs remain unimplemented. Guest root access to DRM and evdev is
test infrastructure. Production seat mediation is outside scope.

The guest starts from Alpine 3.20.3, verified against its committed rootfs
SHA256. That archive covers the 14 seed packages. `guest/packages.sha256`
pins the filenames and SHA256 values of all 44 additional APKs, including
foot 1.17.2-r0. `harness/prepare-guest.sh` verifies every cached or downloaded
APK and its signature before installing from local files with repositories
and network access disabled. A fresh root was installed and booted successfully.
`harness/test-guest-lock.py` checks that a corrupt cached APK is rejected.

`make drm-proof` verifies real atomic scanout. `make wayland-pixels` verifies
real foot shared-memory pixels. `make smoke` additionally injects `laputa`
and Enter through QMP. The guest program runs inside foot, prints a
non-magenta prompt, and must report `GUEST_TEXT_OK` after reading that line.
Only after successful input does it print a magenta `POST_INPUT_MARKER` to the
terminal. The screenshot magenta check therefore proves a post-input terminal
update followed by confirmed presentation, closing input to foot to guest to
changed pixels to presentation to screenshot. QMP queues all 14 press and
release events in one transaction, preventing repeats caused by delayed key-up.
An uncertain response exits 75 and is never retried. Explicit rejection is a
separate failure. `harness/test-qmp.py` exercises these transport outcomes.

`make drm-parity` compares the complete decoded atomic and legacy DRM
screenshots byte for byte to prove atomic-versus-legacy modesetting parity,
then runs the full input smoke through legacy fallback. Both modesetting
paths share the subsequent `page_flip` presentation path, so the comparison
is modeset parity rather than two independent presentation implementations.
The guest uses strace to return ENOSYS from the atomic capability
ioctl. The trace, explicit unsupported-capability diagnostic, and legacy
path log establish that fallback actually ran. The injection ordinal includes
the logger's initial terminal ioctl. A changed ordinal fails the proof guard.
Capability EINVAL, ENOSYS, and EOPNOTSUPP permit fallback. Atomic operation
failures permit fallback only for ENOSYS or EOPNOTSUPP. Other errors propagate.

The compositor owns two scanout buffers. It writes the back buffer, submits
an event-requesting page flip, and waits up to two seconds for the matching
kernel completion. Only then does it reuse the previous front buffer and
complete frame callbacks using the kernel timestamp. `TINYLAND_PRESENTED`
records actual flip completion. A failed or missing flip does not complete
callbacks. Client shm is copied at commit, so buffer release never leaves
scanout dependent on client-writable memory.

`make negative` boots five real guest sessions. Withheld input must time out.
A terminal rendered completely black must fail the nonblack check. A terminal
without the magenta marker must fail the marker check. A frozen terminal that
reports input success without a post-input update must also fail the marker
check, proving the screenshot requires changed pixels after input. Killing
the compositor must fail the process-liveness check. These live failures
replace the earlier artifact-only negative tests. Each case checks its
specific failure reason.

`harness/check-proof.sh` requires mapping, confirmed presentation, input
confirmation for smoke, and no protocol error or early exit. The screenshot
must have at least 5% nonblack pixels and 0.01% magenta pixels. Nonblack means
R+G+B > 24. Magenta means R > 180, B > 180, G < 120. No crop is used.

Proofs run serially against one guest image. They write console, compositor,
QEMU, and PNG artifacts under `.work/`, with fault names as suffixes. The host
owns QEMU and reaps the exact child on success or failure. Readiness is bounded
at 90 seconds, input confirmation at 20 seconds, and normal quit at 15 seconds.
Each QMP socket operation has a 20-second timeout. Failure cleanup attempts a
screenshot and QMP quit, then kills and reaps a child that remains alive.

`make test` runs the Rust suite plus QMP and package-lock tests. Rust coverage
includes shm validation and lifetimes, retained pixels after release, surface
commit and restacking transactions, three-level subsurface position ordering,
configure serials, destruction, bounded subtree reparenting, callbacks,
keyboard modifiers, logical-seat focus isolation across two clients and two
toplevels, clipboard focus, per-client and global memory budgets with
fallible allocation, collection bounds, child reaping, shutdown signals,
scale and transform subset handling, and malformed CLI options. Invalid CLI
arguments return usage status 2 before opening devices.

`make audit` checks the full feature tree and the release binary's dynamic
section, reports source lines and executable bytes, and rejects application
unsafe blocks. The compositor has no libwayland-server, libdrm, Mesa, EGL,
GBM, Pixman, Smithay, wlroots, systemd, or D-Bus dependency. Guest foot and
strace have their own dependencies. The compositor's libxkbcommon is statically
linked. Signal-hook and its registry use the existing libc dependency; both
have MIT or Apache-2.0 licenses. No new Rust dependency was added for closeout.

TERM, HUP, INT, and QUIT request event-loop shutdown. Cleanup reaps foot,
releases evdev grabs, closes Wayland, and releases DRM before restoring signal
death. Fatal signals use kernel descriptor cleanup; foot observes the closed
Wayland connection. The compositor acquires no terminal mode.

The seat advertises keyboard input only, with a US keymap. Clipboard offers
follow the single logical focus and forward transfer descriptors to the
source. Drag operations are cancelled and popups dismissed. The kiosk uses
one fullscreen toplevel, renders scale 1 with normal transform, and accepts
but ignores other positive scales and valid non-normal transforms as a
documented foot-focused limitation. Only non-positive scales and unknown
transform values are protocol errors. ARGB8888 or XRGB8888 shm is supported.
Global versions in `src/server.rs` are authoritative; outgoing events respect
bound versions.

On a fresh checkout, run `bash harness/build-guest.sh` before Rust tests to
prepare the native library and XKB data. Proof commands build the release
executable with the pinned nightly, kache, and mold. Build-time impact remains
unmeasured on the busy host. No performance improvement or timing-gate pass
is claimed.

The final local completion review covers source, callers, tests, guest setup,
transport, and recorded runtime proofs. An independent reviewer identified
the subtree-depth bypass, now fixed with a regression test, but its review
was interrupted twice by hardware-key authentication rejection. No independent
clean verdict is claimed.
