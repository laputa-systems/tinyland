# Campaign: Minimal Rust Wayland Compositor

## Status

Goal: one Rust binary drives real foot pixels in QEMU and receives real QMP-injected input.
Base: empty repo, no commits; toolchain `nightly-2026-09-15` active; QEMU 11.1.2, `/dev/kvm` present.
Accepted: S0–S4 implementation and local proof complete on Linux x86_64 KVM.
`make smoke`, `make wayland-pixels`, and `make drm-parity` pass. Atomic and
legacy complete screenshots are byte-identical; legacy full input smoke passes.
Callbacks follow confirmed kernel page flips. The rootfs and all 44 add-on
APKs have committed hashes; fresh-root install and boot pass. Four live guest
negative cases reject missing input, black scanout, missing marker, and crash.
`make test`: 27 Rust unit tests, one CLI integration test, four Python tests pass.
`make audit`: no forbidden libraries or application unsafe; 2,798 Rust lines
including tests, release binary 3,661,528 bytes. No push.
S0 recorded loop: check 0.08s incremental, test build 8.4s at skeleton revision.
S5 remains open for completion of independent review. The reviewer identified
a subtree-depth bypass, reproduced and fixed, then failed twice because its
session lacks authorized hardware-key authentication. Local review and docs
refresh are complete; no independent P1/P2-clean verdict is claimed.
Needs owner: restore reviewer authentication or specify an approved review route.
Current build timings are unmeasured on the busy host; no timing gate is defined.
Verification commands and limits: `docs/verification.md`.

## Charter

- Decide and keep going. Record a decision only when a later slice depends on it.
- Within the grant below, proceed without waiting. Park items needing outside authority.
- Hard stops (park, do not push through): irreversible or outside-repo action; forbidden effect; loop with no checkable proof. Pushing is always forbidden; land on integration branch only.
- One writer, serial. No parallel pairs qualify: single crate shares `Cargo.toml`, `Cargo.lock`, `src/main.rs`.
- Worktrees for any future lanes: `.work/wt/<lane>` only, never `/tmp`; scratch in `.work/tmp`.
- Parent steering checkpoint after each accepted slice, on resume, after 2 failures.
- Failing item: 3 attempts max, then park on its branch with cause + retry condition.
- Parity before optimization. Perf/size numbers are logged, not gating, unless owner requires.
- Fix egregious problems met on the way in their own commit: panic on client input, deadlock/hang >60s, leak of mapped shm >1 buffer per commit, idle render loop >5 wakeups/s. Otherwise record and leave.
- Every commit is Conventional Commit with workstream scope; item ID in body. Code never cites this doc.

## Decisions

Owner-decided (2026-10-28):
- Host scope: P0 gates Linux x86_64 KVM only. macOS HVF, Linux aarch64 best-effort under Needs environment.
- Guest: pinned Alpine miniroot + pinned apk packages, hashes committed. No Laputa build-pipeline dependency.
- libxkbcommon allowed as the single native keyboard-map dep, isolated to input, audited. Pure-Rust XKB out of scope.
- Guest-only root access to `/dev/dri` + evdev permitted as test infrastructure. Not a production security claim; seat mediation deferred.

Prescribed (from code, evidence, or sound practice):
- Toolchain is `rust-toolchain.toml` `nightly-2026-09-15`, components rust-src + clippy. Do not downgrade to stable; do not bump the pin without owner approval. Reason: user direction; repo pin is the contract.
- Single crate, Linux-only, no async runtime, single-threaded poll event loop. No Smithay, wlroots, libwayland-server, libdrm, Mesa, EGL, GBM, Vulkan, Pixman, systemd, D-Bus, XWayland.
- Wayland via `wayland-server` default (pure-Rust `wayland-backend`); `system`/libwayland backend forbidden. Reason: docs.rs shows `system` feature opts into libwayland; default avoids it.
- DRM via `drm` crate (direct ioctls, bypasses libdrm) + `rustix`. Reason: docs.rs confirms direct approach.
- Modeset: try atomic, fall back to legacy on `ENOSYS`/atomic-unsupported with explicit log line stating which path. Both paths must show identical pixels. Reversal: if virtio-gpu in pinned guest lacks atomic, legacy becomes primary and atomic retry is parked.
- Advertise only implemented globals; never fake support to start foot. Protocol violation by client => disconnect with protocol error + log, no panic. Internal invariant violation => assert/panic, distinct from client errors.
- Validate before use: shm offsets/sizes/strides, pool sizes (foot uses large sparse pools), object lifetimes, subsurface parent links, configure serials. Fail closed.
- Buffer ownership: never release a buffer while readable for scanout; frame callbacks fire at presentation boundary only.
- QMP: preserve retryable-read vs uncertain-input distinction; never resend an input mutation after ambiguous failure.
- Unsafe only for mmap/ioctl via `drm`/`rustix`; each block carries a `// SAFETY:` comment stating the invariant. No other unsafe without owner approval.

Delegated (executor decides, smallest reversible assumption, log it):
- Exact atomic-vs-legacy probe order and dumb-buffer format choice (XRGB8888 vs ARGB8888 scanout), bound: must match virtio-gpu advertised formats, logged.
- Evdev discovery pattern, bound: must not hardcode `/dev/input/eventN`; match by name/capability, log chosen node.
- Screenshot magenta threshold and crop region, bound: fixed constants in harness, documented; any change re-runs full smoke.

## Baseline

- Tree: `plan.md` + `rust-toolchain.toml` only, zero commits. Known red: none yet; executor re-records before S1.
- Verified 2026-10-28: cage exists and builds on wlroots+xkbcommon (behavioral ref only); `wayland-server` 0.31.14 default backend is Rust; `drm` 0.15.0 bypasses libdrm; laputa repo exists with `boot/ docs/ guest/ system/` dirs; active toolchain `nightly-2026-09-15`; QEMU 11.1.2; `/dev/kvm` + `/dev/dri/card0` present; `foot` absent on host (guest-only).
- Verified for S3: foot 1.17.2 source requires compositor 4, subcompositor 1, shm 1, xdg shell 1, seat 5, and data device manager 3; positional command is supported. Laputa reference harness and QMP failure semantics were read before input implementation.
- Loop timings: `cargo --version` 0.023s; `git status` 0.001s. `cargo check/test`, QEMU boot unmeasured (no code). Time them in S0. Host load ~16.5 on 32 cores at preflight; if `perf-gate-ready -s` reports busy, perf numbers are deferred, never gating.

## Sequence

All items serial on the integrated tree. Each ends with its acceptance + narrowest affected suite; full suite at S3 and S5.

- S0 Skeleton + loop. Owns: `Cargo.toml`, `Cargo.lock`, `Makefile`, `src/main.rs`, `.gitignore`, `.work/` exclude. Prereq: none. Acceptance: `cargo check --locked` and `cargo test --locked --no-run` pass; `git worktree list` clean; loop times recorded in Status.
- S1 DRM scanout. Owns: `src/output.rs`, `src/main.rs` wiring, `harness/drm-proof.*`. Prereq: S0. Acceptance: `make drm-proof` boots pinned guest, scans out solid framebuffer, screenshot differs from black, teardown leaves no `qemu-system` process.
- S2 Wayland + software compose. Owns: `src/server.rs`, `src/surface.rs`, `src/shell.rs`, `src/shm.rs`, `src/selection.rs`, `src/output.rs` (compose path). Prereq: S1. Acceptance: `make wayland-pixels` launches real foot, completes configure/ack, commits ARGB8888/XRGB8888 shm buffer, display screenshot contains non-background terminal pixels, no protocol error.
- S3 Input + full smoke. Owns: `src/input.rs`, `src/main.rs` (child lifecycle), `harness/smoke.*`, `guest/*`. Prereq: S2. Acceptance: `make smoke` boots guest, maps foot, injects `laputa` via QMP through evdev+`wl_keyboard`, guest asserts text, screenshot contains magenta marker, artifacts (console, compositor, QEMU log, png) written, bounded teardown.
- S4 Hardening. Owns: `tests/*`, `harness/negative-*`, `docs/*`. Prereq: S3. Acceptance: `cargo test --locked` green (surface state, bad sizes/strides, serials, destruction, buffer ownership, protocol errors); negatives fail as expected (no input, blank fb, missing marker, early crash); `cargo tree -e features` + linker check show no libwayland-server/libdrm/Mesa; unsafe inventory + LOC/size/libs documented.
- S5 Close. Owns: deletions, final report. Prereq: S4. Acceptance: dead code removed, `ultrareview` P1/P2 clean, full `make smoke` + `cargo test --locked` green on integration branch.

## Acceptance

- `cargo check --locked`, `cargo test --locked`: success = zero failures; failure = any red, executor bisects before retry.
- `make drm-proof`: success = scanned-out pixels differ from black + clean teardown; failure = black screen, modeset error without diagnostic, or leftover QEMU.
- `make wayland-pixels`: success = foot-mapped log + terminal pixels; failure = protocol error, no commit, or placeholder/synthetic pixels.
- `make smoke`: success = guest text `laputa` confirmed + magenta marker in real screenshot + all artifacts + no stray process; failure = any missing link, simulated input, or fake client/image. Uncertain input outcome => park, never resend blindly.
- Audit: success = forbidden libs absent in `cargo tree`/linker output, every unsafe has `// SAFETY:`, measurements logged. Failure = forbidden dep or undocumented unsafe.

## Needs environment

Non-gating, unimplemented: macOS aarch64 HVF and Linux aarch64 KVM harness
ports and proofs. Both need architecture-specific guest images and QEMU
arguments plus appropriate hosts. The current harness supports Linux x86_64
KVM only; an `ARCH` assignment does not select another guest.

## Execution boundary and non-goals

Authorized: read Laputa/cage/dwl/wayland-rs/drm-rs sources; create pinned Alpine guest image; run QEMU with KVM locally; open `/dev/dri`+evdev as root inside guest only; add Rust deps within perf gates; write under repo + `.work/`.
Forbidden: push; modify Laputa or its deps; import Smithay/wlroots/libdrm/Mesa/EGL/GBM; claim unsupported hosts work; resend input after ambiguous QMP failure; fake pixels/clients/screenshots.
Non-goals: tiling/tags, multimonitor, XWayland, DMA-BUF, GPU rendering, animations, plugins, portals, display manager, production seat mediation.
Contract changes: toolchain pinned to `nightly-2026-09-15` (user direction, not stable); guest-only root in test harness (not production design). The seat advertises keyboard only; presentation completion uses kernel flip events. Invalid CLI options return usage status 2.

## Reference

- Cage (`cage-kiosk/cage`): single-fullscreen policy + lifecycle reference; wlroots-based, not a dependency.
- External reference: Laputa in the sibling laputa-systems repository. Its QEMU construction, guest proof, QMP helper, QEMU documentation, and foot profile informed input and timeout semantics. No image machinery was imported.
- `wayland-server` 0.31.14 + `wayland-protocols` generated defs; `drm` 0.15.0 + `rustix`; `libxkbcommon` isolated to input. Foot source pinned by executor for global-version truth.
