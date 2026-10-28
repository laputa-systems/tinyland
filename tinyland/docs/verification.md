# Verification reference

The supported proof host is Linux x86_64 with KVM. Other host and guest
architectures are unverified. The guest uses Alpine 3.20.3 and foot 1.17.2-r0.
Guest root access to DRM and evdev is test infrastructure. The compositor does
not implement production seat mediation.

`make drm-proof` verifies atomic DRM scanout. `make wayland-pixels` verifies
real foot shared-memory pixels. `make smoke` additionally injects `laputa` and
Enter exactly once through QMP. The guest program runs inside foot and must
report `GUEST_TEXT_OK` after reading that line.

Each proof writes console, compositor, QEMU, and PNG artifacts under `.work/`.
The host owns QEMU, requests QMP quit, waits, and kills and reaps the exact
child on failure. Each proof has a 90-second readiness bound, a 20-second
input bound where applicable, and a 15-second quit bound. QMP requests have a
20-second socket timeout. Input is never retried after an uncertain result.

`harness/check-proof.sh` requires a mapped surface and rejects protocol errors
and early exits. Smoke also requires input confirmation. The whole screenshot
must have at least 5% nonblack pixels and 0.01% magenta pixels. Nonblack means
R+G+B > 24. Magenta means R > 180, B > 180, G < 120. No crop is used.
`python3 harness/negative-proof.py` checks that absent input, a blank image,
a missing marker, and an early crash each fail for the expected reason. Its
synthetic images are failure fixtures, never positive proof evidence.

`cargo test --locked` checks shm sizes, strides, growth, pool destruction,
truncation, retained pixels after buffer release, synchronized child commits,
configure serial rejection, frame callback ordering, role cycles, protocol
errors, alpha composition, keyboard modifiers, child reaping, and shutdown
signals. Client buffers are read into owned pixel snapshots at commit.
Releasing a buffer never leaves scanout dependent on client-writable memory.
Frame callbacks fire only after a successful output write.

`make audit` checks the complete feature tree and the guest binary's dynamic
section. The compositor has no libwayland-server, libdrm, Mesa, EGL, GBM,
Pixman, Smithay, wlroots, systemd, or D-Bus dependency. Foot has its own client
libraries in the guest. libxkbcommon is the compositor's single native
keyboard library, linked statically from the guest's MIT-licensed package.
Application source contains zero unsafe blocks.

Signal handling uses `signal-hook` with default features disabled and its
`signal-hook-registry` dependency. Both use the existing libc dependency.
They provide safe signal registration without adding application FFI.
TERM, HUP, INT, and QUIT request event-loop shutdown. Cleanup reaps foot,
releases evdev grabs, closes Wayland, and releases DRM before signal death
is restored. Fatal signals use kernel descriptor cleanup; foot observes the
closed Wayland connection. No terminal mode is acquired by the compositor.

The S4 debug guest executable is about 44 MB with debug information. The
final audit records exact source line and executable byte counts. Build-time
impact is unmeasured: the host load check reported competing builds. No
performance improvement is claimed. Final release size is checked separately.
