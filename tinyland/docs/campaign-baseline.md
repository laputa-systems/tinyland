# Campaign baseline (Phase 0)

Measured 2026-10-30, Linux x86_64. Re-measures `docs/CAMPAIGN-TINYLAND-GHOSTTY.md`.
Every number cites its proof command. No implementation in this file.

## Re-measured table

| Item | Value | Proof command |
|---|---|---|
| `tinyland` binary | 2117480 bytes | `tar xzf .out/artifacts/x86_64/v2/sha256/fd64294d5956a57ef1d2972600f0b1dc0fe1eef9ad0aca30c2a4db1c39d8f766/payload.tar.gz -C /tmp/opencode/tl; stat -c %s /tmp/opencode/tl/usr/bin/tinyland` |
| `tinyland` DT_NEEDED | `libseat.so.1`, `libxkbcommon.so.0`, `libgcc_s.so`, `libc.so` | `readelf -d /tmp/opencode/tl/usr/bin/tinyland` (RUNPATH `/usr/lib` also present) |
| Tinyland Cargo graph | 80 locked crates, `xkbcommon 0.9.0`, no `kbvm` | `grep -c 'name = ' tinyland/Cargo.lock`; `grep -A2 'name = "xkbcommon"' tinyland/Cargo.lock` |
| Tinyland toolchain | `nightly-2026-09-15` | `cat tinyland/rust-toolchain.toml` (`rustc 1.100.0-nightly (574ff7d98 2026-09-14)`) |
| `qemu-tinyland-foot` closure | 41 plan nodes | `python3 -c "import json; print(len(json.load(open('target/laputa/qemu-tinyland-foot/build-plan.json'))['nodes']))"` |
| `qemu-tinyland-vt-foot` closure | 41 plan nodes | same over `qemu-tinyland-vt-foot/build-plan.json` |
| `qemu-dwl-foot` closure | 48 plan nodes | same over `qemu-dwl-foot/build-plan.json` |
| Mesa 26.2.4 config | EGL/GBM/GLES on softpipe+virgl, `-Dopengl=false -Dllvm=disabled`, `-Dglx=disabled` | `sed -n '199,232p' packages/mesa/PKGBUILD.xsh` |
| Ghostty Custom release | unverified from here: no URL/payload in repo; campaign cites `release-354a4e32dc4959ec10709adbdeac114594a05097` at `354a4e3`, 7 assets, musl tarballs dynamic on libEGL/libwayland-client/libxkbcommon | `grep -rli ghostty packages/ profiles/ docs/ tinyland/src tinyland/Cargo.*` returns only the campaign doc |
| Repo references to ghostty/kbvm | none outside campaign doc | same grep as above |
| Tinyland proof surface | `harness/{proof,run-proof,check-proof,wayland-pixels,drm-proof,drm-parity,seat-proof,negative-runtime,smoke}.sh`, `harness/{qmp,test-qmp,test-guest-lock,check-screenshot}.py`, guest QMP input + screenshot checks | `ls tinyland/harness/`; `tinyland/docs/verification.md`; `tinyland/AGENTS.md` |

Payload identity: `tinyland-0.1.0-1` artifact `fd64294d5956a57ef1d2972600f0b1dc0fe1eef9ad0aca30c2a4db1c39d8f766`, payload SHA-256 `8f7532ec24b08f5c74f578135d2ca7a9605acf436d22e8bfff49d6bb49fcb393` per `.out/artifacts/x86_64/v2/sha256/fd64294d*/artifact.json`.

## Cargo tree delta (baseline)

`cargo tree --locked` (104 lines) root deps: `drm 0.15`, `env_logger 0.11`, `evdev 0.13`, `log 0.4`, `rustix 1.1`, `signal-hook 0.4`, `wayland-protocols 0.32`, `wayland-server 0.31`, `xkbcommon 0.9`. No `kbvm`, no Smithay/wlroots/libdrm/Mesa/EGL/GBM. Phase 1 must show no `xkbcommon` runtime crate and no `libxkbcommon` NEEDED.

## Guest proof inventory (existing, unchanged)

- `make smoke`: QMP `laputa`+Enter, `GUEST_TEXT_OK` then magenta marker, screenshot 5% nonblack + 0.01% magenta.
- `make wayland-pixels`: foot shm pixels.
- `make drm-proof`: real scanout, 30% nonblack.
- `make drm-parity`: atomic vs legacy byte-equal + legacy smoke rerun.
- `make negative`: five live faults in `negative-runtime.sh`.
- `make seat`: seatd-mediated, `TINYLAND_DIRECT_DENIED` + `TINYLAND_SEAT_OPEN`, same smoke gates.
- `cargo test --locked` + `test-qmp.py` + `test-guest-lock.py`; `harness/audit.sh` (no forbidden libs, no `unsafe` outside `src/seat.rs`).
- Laputa: `make profile-tinyland-test`, `make profile-tinyland-vt-test` on KVM x86_64.

Source: `tinyland/docs/verification.md`, `tinyland/docs/laputa-integration.md`, `docs/QEMU.md`, `docs/TINYLAND.md`.

## KBVM toolchain verdict

PASS. `kbvm 0.2.0` (MSRV 1.95.0) compiles under pinned `nightly-2026-09-15` (`rustc 1.100.0-nightly`).

Reproducer:

```sh
cargo +nightly-2026-09-15 check --manifest-path /tmp/opencode/kbvm-test/Cargo.toml
# Cargo.toml: kbvm = "0.2.0", src/main.rs uses Context::default()
# result: Finished dev profile, kbvm v0.2.0 checked green
```

Full log in shell history 2026-10-30. No version-string reasoning; compile is the verdict.

## Mesa baseline

`packages/mesa/PKGBUILD.xsh:mesa_options`: `platforms=wayland`, `gallium-drivers=softpipe,virgl`, `egl/gbm/gles2` enabled, `opengl=false`, `llvm=disabled`, `glx=disabled`. Generated inputs vendored per `packages/mesa/files/generated/README.md`. Phase 3 must guest-prove desktop core >=4.3 before any flag change; virgl preferred unless probe proves otherwise with numbers.

## Blockers / unknowns carried forward

- Ghostty tarball hashes and download URLs not in repo; Phase 2 must pin them with SHA-256, no SKIP.
- Softpipe 4.3 capability undecided; Phase 3 probe decides.
- aarch64 runtime and macOS host remain Needs environment per campaign.
