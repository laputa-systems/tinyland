# OpenGL feasibility probe (Phase 3)

Guest-executed where stated; build-only where noted. No Mesa flag changed.

## Current Mesa (pinned, `packages/mesa/PKGBUILD.xsh`)

- Options: `platforms=wayland`, `gallium-drivers=softpipe,virgl`,
  `egl/gbm/gles2` on, `opengl=false`, `llvm=disabled`, `glx=disabled`.
- Proof (`packages/mesa/proof.xsh`, host-executed against the proof root):
  surfaceless EGL init, GLES2 context, offscreen 8x8 render with red/blue
  readback match. Reports `GL_RENDERER=softpipe`, `OpenGL ES 3.1 Mesa`,
  `rendered`. This passed as a dependency of the ghostty-custom build
  (mesa artifact `a11f0c78…`, payload 4.1M).
- Desktop GL: none. `opengl=false` means no `libGL`; no desktop core
  context exists to probe. Any `eglCreateContext` for desktop OpenGL
  fails at link time (no library), not at version negotiation.

## Ghostty renderer requirement (packaged binary)

- `ghostty-custom-1.4.1-1` (artifact `8a48057a…`, payload 17.8M, binary
  29.5M): `readelf -d` shows `libEGL.so.1`, `libwayland-client.so.0`,
  `libxkbcommon.so.0`, `libc.so` only. No GTK/Adwaita/X11/glibc.
- `--version` (proof root, `LD_LIBRARY_PATH` set): `Ghostty 1.4.1-dev`,
  `app runtime: .wayland`, `renderer: renderer.generic.Renderer(renderer.OpenGL)`.
  The renderer is desktop OpenGL, not GLES. Campaign threshold is
  desktop core >=4.3 with offscreen readback match.

## QEMU display (guest 3D path)

- `system/qemu.xsh` launches `-device virtio-gpu-pci` with no `virgl=on`
  and `-display none` in test mode. The guest DRM is 2D virtio-gpu;
  there is no virgl host backing, so the guest `virgl` gallium driver
  has no transport. The only active 3D driver is `softpipe` (software).

## Verdict

Softpipe without LLVM cannot do desktop 4.3 (softpipe exposes desktop
3.3 at best; 4.3+ needs llvmpipe+LLVM or virgl with host 3D). Current
closure has neither: `opengl=false` (no desktop GL at all) and
`llvm=disabled` (no llvmpipe). Guest e2e with Ghostty is blocked on a
desktop-GL renderer init reproducer (Phase 4/5 must show Ghostty
failing to create its OpenGL context under the current Mesa).

## Decision (recorded before any Mesa change, per campaign)

- Preferred: virgl. Enable host 3D (`virgl=on` + EGL display in
  `system/qemu.xsh` test mode only if KVM/host supports it) and Mesa
  `-Dopengl=true` (keep `-Dllvm=disabled`, keep `gallium-drivers=
  softpipe,virgl`). No guest LLVM, no `libLLVM` in the kiosk closure
  (current profiles forbid `libLLVM` via `forbidden_sonames`).
- Fallback: llvmpipe. Requires `-Dllvm=enabled`, which pulls the LLVM
  runtime into the guest image and the `llvm-toolchain` build closure.
  Current mesa payload is 4.1M with no LLVM; llvmpipe+LLVM would add
  tens of MB (libLLVM alone dwarfs the 2.1M tinyland and 4.1M mesa)
  and violate the minimal-closure rule that forbids `libLLVM`.
  Measure before adopting: rebuild Mesa with LLVM, record `DT_NEEDED`,
  payload bytes, plan nodes, and cold-start delta.

No Mesa flag changed in this phase. Phase 4 profiles are built against
the current Mesa to capture the blocker reproducer; the Mesa change
lands only with the virgl QEMU + proof update.
