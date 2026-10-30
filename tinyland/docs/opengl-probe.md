# OpenGL feasibility probe (Phase 3)

Measured 2026-10-30/31, Linux x86_64. Guest-executed where stated;
otherwise host-executed against the exact packaged payloads with the
container musl toolchain (same binaries the PM proof runs).

## Current Mesa (after `opengl=true`, `packages/mesa/PKGBUILD.xsh` rel 2)

- Options: `platforms=wayland`, `gallium-drivers=softpipe,virgl`,
  `egl/gbm/gles2` plus desktop `opengl` on, `llvm=disabled`,
  `glx=disabled`. No libGL is shipped (no GLX without X11); desktop GL
  is EGL-only, which is all Ghostty links.
- Generated sources regenerated for the new option set per
  `packages/mesa/files/generated/README.md` (81 outputs, archive
  `9938c1ac…`): the `opengl=false` set (70 outputs) lacked 11 desktop
  GL tables (`glapi_gentable.c`, `glapi_mapi_tmp.h`, `indirect*.c/h`,
  `g_egldispatchstubs.c/h`, `egl.def`, `gles2.def`).
- Proof (host-executed proof root, surfaceless EGL; same cases as the PM
  proof, replicated manually against payloads `a1a11de8…` + musl +
  libdrm + wayland-libs-client): `GL_RENDERER=softpipe`,
  `GL_VERSION=OpenGL ES 3.1 Mesa 26.2.4` (unchanged), and:
  `DESKTOP_GL_RENDERER=softpipe`,
  `DESKTOP_GL_VERSION=3.3 (Compatibility Profile) Mesa 26.2.4`,
  with a green 8x8 FBO clear + readback (`desktop-rendered`).
  Command: container-musl `cc` build of the proof program, run under
  the packaged `ld-musl` with `LD_LIBRARY_PATH` + `GBM_BACKENDS_PATH`.
- Payload 4.28M → 5.14M (+0.86MB). `DT_NEEDED` unchanged apart from
  the desktop driver: no `libLLVM`, no new runtime deps
  (libm/libdrm/libwayland-client/libgbm/libgallium only).

## Ghostty renderer requirement (packaged binary)

- `ghostty-custom-1.4.1-1`: `app runtime: .wayland`,
  `renderer: renderer.generic.Renderer(renderer.OpenGL)`; its source
  (`src/renderer/OpenGL.zig`) requests an EGL 4.3 core profile context
  (`MIN_VERSION_MAJOR=4`, `MIN_VERSION_MINOR=3`,
  `EGL_OPENGL_CORE_PROFILE_BIT`) and fails closed with `OpenGLOutdated`
  below 4.3.

## QEMU display (guest 3D path)

- `system/qemu.xsh` launches `-device virtio-gpu-pci` with no `virgl=on`
  and `-display none` in test mode; the host QEMU offers no virgl
  device option and only the `none` display backend, so the guest
  `virgl` gallium driver has no host backing. Active 3D driver:
  `softpipe` (software) only.

## Verdict

Softpipe desktop ceiling is **3.3 compat** (measured above), below
Ghostty's required **4.3 core**. virgl is unavailable on this host.
The only 4.3-capable software path is llvmpipe, which needs LLVM.

## Decision (recorded with numbers; no LLVM committed)

- Preferred remains virgl where a host supports it (no guest LLVM,
  no `libLLVM` in the kiosk closure, which current profiles forbid via
  `forbidden_sonames`). Not available here: host QEMU has no virgl
  device option and no EGL-capable display backend.
- llvmpipe fallback cost, measured: the `llvm-toolchain` payload
  (145.8M) ships `libLTO`/`libclang` only — no `libLLVM.so`. A shared
  `libLLVM` (~100MB class, against the 3.0M tinyland / 5.1M mesa
  scale) would need a recipe change plus a new runtime package, Mesa
  `-Dllvm=enabled` plus another generated-sources regeneration, the
  `libLLVM` profile-ban lift on ghostty profiles only, and a full
  guest re-proof. Estimated guest growth: tens of MB (order of the
  whole current kiosk image). Not committed in this slice.
- Landed instead: EGL-only desktop GL (3.3, functional, +0.86MB, no
  new deps) with the ceiling machine-checked by the mesa proof, so a
  future driver change that moves the ceiling re-opens this decision
  loudly rather than silently.
