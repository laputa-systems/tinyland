# Verification reference

The supported proof host is Linux x86_64 with KVM. macOS and aarch64
harness ports and proofs remain unimplemented. The guest uses Alpine 3.20.3 and foot 1.17.2-r0.
`guest/packages` pins the installed dependency closure. APK verifies repository
signatures with the keys from the pinned rootfs.
Guest root access to DRM and evdev is test infrastructure. The compositor does
not implement production seat mediation.

`make drm-proof` verifies atomic DRM scanout. `make wayland-pixels` verifies
real foot shared-memory pixels. `make smoke` additionally injects `laputa` and
Enter exactly once through QMP. The guest program runs inside foot and must
report `GUEST_TEXT_OK` after reading that line.

Proofs write console, QEMU, and PNG artifacts under `.work/`. Wayland proofs
also record a separate compositor log.
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
`signal-hook-registry` dependency. Both use libc already present in the dependency tree. Their licenses are
MIT or Apache-2.0.
They provide safe signal registration without adding application FFI.
TERM, HUP, INT, and QUIT request event-loop shutdown. Cleanup reaps foot,
releases evdev grabs, closes Wayland, and releases DRM before signal death
is restored. Fatal signals use kernel descriptor cleanup; foot observes the
closed Wayland connection. No terminal mode is acquired by the compositor.

The final release executable is 3,656,192 bytes, with no dynamic library
requirements. The audit counts 2,694 Rust lines including tests. Removing the
unused mmap and direct libc and bitflags declarations keeps the dependency
surface small. Build-time impact is unmeasured: the host load check reported
competing builds. There is no time-based performance gate in this repository.
No performance improvement is claimed.

The seat advertises keyboard input only, with a US keymap. Clipboard selection
offers follow keyboard focus and forward transfer descriptors to the source.
Drag operations are cancelled and popups are dismissed. The kiosk uses one
fullscreen toplevel, scale 1, normal transform, and ARGB8888 or XRGB8888 shm.
The global versions in `src/server.rs` are authoritative. Version-1 output
bindings receive only version-1 events.

Before running Rust tests on a fresh checkout, run `bash harness/build-guest.sh`
to prepare the native static library and XKB data in `.work/guest-root`.
The local build configuration uses those guest files and links musl with mold.
Proof commands build the release executable. The pinned nightly stays unchanged.
