# Seatd Rust port — campaign

## Status

- Goal: Rust seatd on Linux, libseat-compatible; supervised foreground daemon.
- Base: fork HEAD `bc0e96f` (2026-10-25). C baseline green (3/3).
- Accepted: none. Red: none. Parked: none.
- Next: R0. Resume: run the one-line prompt in the preflight report.
- First command: `cargo build --offline`.

## Charter

- Decide inside this doc and keep going; record a decision only when a later slice depends on it.
- The grant covers this repo only. A slice needing outside authority parks. Push is always forbidden.
- Hard stops (park the item): irreversible or outside-repo effects; deleting host sockets/devices; privileged ops on the host console/GPU/input; proof that runs nowhere available.
- Fix egregious breakage met on the way in its own commit: FD growth >0 over a 1000-reconnect storm, unbounded queue growth, idle wakeups >2x the C reference. Otherwise record and leave.
- Parity before optimization. No perf gating until R8 except the thresholds above.
- One writer by default. Parallel only: `doc/**` with one code slice. Worktrees at `.work/wt/<lane>`, never `/tmp`.
- Steering checkpoint after each accepted slice: what landed, what limits delivery, ruling (continue/narrow/serialize/park). No lanes after two sliceless checkpoints.
- Every commit is `type(scope): subject`, scope = slice ID, ID in body. Code comments never cite this doc.
- Failing item: reproduce minimal first; one hypothesis at a time; bisect after two refuted; 3 attempts, then park on its branch with cause + retry condition.

## Decisions

Owner (2026-10-28):

- O1 VT bar: qualify `SEATD_VTBOUND=0` + VT unit shims locally; real-VT proof is N1 and VT-bound ships labeled experimental until it runs.
- O2 Scope: no seatd-launch port, no logind/D-Bus/systemd/polkit, no auth/login/power/supervision, no libseat ABI rewrite.
- O3 Oracle: fork HEAD `bc0e96f`; C libseat with seatd backend only (`-Dlibseat-logind=disabled -Dlibseat-builtin=disabled`), driven with `LIBSEAT_BACKEND=seatd`.
- O4 Perf: log numbers per slice; gate only egregious (idle CPU/wakeups >2x C ref, any FD/memory growth under storms); full parity numbers at close.
- O5 Toolchain: `rust-toolchain.toml` pins nightly-2026-09-15 (+rust-src, clippy). No switching; every loop command runs under it.

Prescribed (reason; what reverses it):

- P1 Linux device classes: DRM (`/dev/dri/`), evdev (`/dev/input/event`), hidraw (`/dev/hidraw`) only — the three Linux paths the C code accepts (common/drm.c:26, common/evdev.c:20, common/hidraw.c:17). Reverser: audited new class with kernel-contract tests.
- P2 Symlink policy: keep C compat (`realpath` then open with `O_NOFOLLOW`, seatd/seat.c:340-384) and add post-open fstat type/identity validation; each path C accepted that Rust rejects goes in doc/diffs.md. Reverser: measured libseat/compositor breakage.
- P3 One synchronous nonblocking poll loop, no threads (C shape: seatd/poller.c:310-327). Reverser: profiled limiter requiring threads.
- P4 Deps: std + libc + rustix only, offline-pinned (`libc =0.2.182`, `rustix =0.38.44`, verified cached). No tokio/nix/clap/serde/bindgen/libdrm/D-Bus/RPC framework. Reverser: owner approval with build-time/binary-size numbers.
- P5 Errors observable, never silent advance; every fd has exactly one owner; unneeded received FDs closed; no ancillary resend on partial-write retry (common/connection.c:215-250).
- P6 Pure state transitions + narrow effect adapter for seat/VT/devices; no generalized framework.
- P7 Close deletes the C daemon; C libseat + needed common files move to `oracle/` as the retained oracle build. Liveness by build-graph + rg; survivors named with their consumer.

Delegated (reversible, no contract change, logged in commit body):

- D1 module/file names inside owned dirs; D2 test layout and assertion style; D3 log line format (levels map to libseat levels).

## Baseline

- Base `bc0e96f`; C suite green: linked_list, connection, poller 3/3 via `uvx meson setup` + `ninja test`.
- Loop, timed 2026-10-28 under the pinned toolchain: meson setup 0.7s; ninja build 0.2s; ninja test 0.2s; `cc -c` 0.05s; cargo cold 4–5s; cargo incremental 0.14s; per-module `cargo test` in seconds.
- Foundation proven: exact `u16/u16` NE header layout, socketpair SCM_RIGHTS round-trip with CLOEXEC/NONBLOCK, DRM/EVIOC/HIDIOC ioctl numbers from libc without libdrm (`FOUNDATION-OK` under pinned toolchain).
- Known red: none. `.work/` ignored via `.gitignore`.

## Sequence

Serial R0→R8 on `master`. Attempt limit 3, then park with cause + retry condition.

| ID | Owns | Requires | Proof |
|---|---|---|---|
| R0 skeleton: crate + CLI (`-h -n -u -g -l -v`, man/seatd.1.scd:11-33) + socket bind/replace (seatd/seatd.c:156-173) + `-n` readiness + foreground run + clean shutdown/unlink | Cargo.toml, Cargo.lock, src/main.rs, src/cli.rs, src/loop.rs | — | `cargo build --offline && ./target/debug/seatd -v` + bind/readiness/mode test + C suite green |
| R1 wire: framing, 256B buffers, 64-FD queues (include/connection.h:7-9), NE u16/int decode, opcodes/errors/FD exactly-once (seatd/client.c:349-443, common/connection.c:179-275) | src/wire.rs, src/conn.rs, harness/adversarial.rs | R0 | `cargo test --offline wire`: fragmented/coalesced, empty/oversized/malformed/unknown, EAGAIN/EINTR, peer-close, stray-FD, truncation |
| R2 admission: modes (`-u/-g` 0770, seatd/seatd.c:42-52), SO_PEERCRED (seatd/client.c:30-40), competing-instance refusal, slow-reader/fairness bounds | src/admit.rs | R1 | `cargo test --offline admit`: socket-mode + replace + storm matrix |
| R3 session: open/close, activate/deactivate, pending-disable+ack, switching, disconnect cleanup, exclusion + ordering (seatd/seat.c:192-298,542-716), VTBOUND=0 + VT-bound transitions via adapter | src/session.rs, src/effect.rs | R1 R2 | `cargo test --offline session` + libseat lifecycle differential vs oracle |
| R4 devices: DRM/evdev/hidraw policy (P1, P2), dup/device_id scoping, repeat/dup/stale/cross-client/exhaustion (seatd/seat.c:329-435), deactivate (seatd/seat.c:442-474) vs must-reopen (seatd/seat.c:509-535), FFI boundary + ioctl numbers (common/drm.c:10-11) | src/device.rs, src/policy.rs, src/sys.rs | R3 | `cargo test --offline device` adapter fault-injection; real-device claims stay N2 |
| R5 VT+signals: ioctls (common/terminal.c:154-278), SIGUSR1/2/INT/TERM (seatd/server.c:25-54), restore on exit; VT-bound via adapter, VTBOUND=0 live | src/vt.rs | R3 | `cargo test --offline vt` + signal/shutdown test; real VT stays N1 |
| R6 crash/shutdown: bounded cleanup, unlink without harming replacement, SIGKILL-with-retained-FDs behavior + reconnect procedure | src/shutdown.rs, doc/recovery.md | R3 R5 | kill/restart + FD-leak tests (`0` growth/1000 reconnects) |
| R7 differential+hardening: full corpus both daemons (unmodified C libseat, seatd-only, O3), goldens in harness/, doc/diffs.md register, reconnect/malformed/slow-reader/switch storms, FD/memory + idle CPU/wakeup logs (O4) | harness/differential.rs, harness/storm.rs, doc/diffs.md | R1–R6 | differential green + `cargo clippy` clean + storm/leak gates |
| R8 delivery+close: x86_64/aarch64-musl builds (both targets installed), locked deps, minimal CI with full-SHA pins, license attribution, docs (CLI/env/protocol, classes+trust, limits+recovery, per-arch log, gaps, foreground+readiness contract), C daemon deleted per P7, ultrareview P1/P2 fixed, final report | .github/workflows/ci.yml, doc/**, man/**, C deletions | R7 | `cargo build --offline --target x86_64-unknown-linux-musl` + `--target aarch64-unknown-linux-musl` + full `cargo test --offline` + oracle rebuild + clippy |

## Acceptance

- Binary: foreground `seatd` with documented CLI/socket/readiness; Laputa-compatible invocation (`-g seat`, `/run/seatd.sock`, `SEATD_VTBOUND=0`, `LIBSEAT_BACKEND=seatd`) — read-only reference, never modified.
- Protocol: byte-exact vs oracle on the corpus; adversarial cases fail observably, never advance unsafe state, never leak FDs.
- Admission/session/devices/VT/shutdown: per-slice proofs above; `0` FD growth over 1000-reconnect storms; revocation client-observable.
- Perf: numbers logged per slice from R7; egregious gate O4; no VTBOUND=0 result presented as VT proof (O1).
- Delivery: musl builds both arches locally; CI minimal with full-SHA pins; docs complete; C daemon deleted with liveness evidence; ultrareview P1/P2 clean.
- Never: production-ready claims without N1+N2 run; host-console/GPU/input testing; pushes.

## Needs environment

- N1 real-VT proof: VT-capable QEMU guest, VT-bound switch + release/acquire + restore; owner runs `harness/vt-proof.sh` there. Until then VT-bound is experimental (O1).
- N2 virtual-device qualification: disposable VM with virtio DRM/input, open/revoke/reopen observable via a `simpletest`-equivalent; fake-ioctl success never counts.
- N3 compositor smoke: existing dwl/foot binaries in the VM against Rust seatd; no Laputa/XSH modification.

## Execution boundary and non-goals

- Authorized: edit/create files in this repo; `cargo`/`rustc`/`uvx meson`/`ninja`/`qemu` runs; `.work/wt/<lane>` worktrees; offline cached deps only.
- Forbidden: push; touch Laputa/XSH/mdevd-rs/libudev-zero; host privileged device ops; unlinking anything but the daemon's own socket path; setuid; new network deps without owner approval (P4).
- Non-goals (O2): seatd-launch, logind/D-Bus/systemd/polkit/elogind, auth/login/power/supervision, libseat ABI rewrite, BSD/macOS, wscons, D-Bus-linked oracle (oracle stays seatd-only).
- Contract changes: (1) C→Rust, same CLI/socket/protocol surface; (2) VT-bound experimental until N1; (3) added post-open device validation may reject paths C accepted — each in doc/diffs.md; (4) C daemon deleted at close, libseat retained under oracle/.

## Reference

- Wire: opcodes/limits include/protocol.h:4-29; header + payload structs include/protocol.h:33-63; transport bounds include/connection.h:7-21; server validation seatd/client.c:349-443; flush-on-success seatd/client.c:439-442.
- Admission/lifecycle: CLI + replace + readiness seatd/seatd.c:59-208; signals + VTBOUND seat seatd/server.c:25-54; accept/add seatd/server.c:124-171; peer creds seatd/client.c:30-101; hangup/readable loop seatd/client.c:471-528.
- Seat/devices: create/add/remove seatd/seat.c:34-53,192-298; open/policy/flags seatd/seat.c:329-435; deactivate/reactivate seatd/seat.c:442-535; enable/disable/ack/switch seatd/seat.c:542-716; VT activate/release seatd/seat.c:723-757.
- Kernel: poller seatd/poller.c:71-93,310-327 (masks include/poller.h:12-16); terminal ioctls common/terminal.c:154-278; DRM common/drm.c:10-35; evdev common/evdev.c:20-26; hidraw common/hidraw.c:15-23.
- Client oracle: connect + SEATD_SOCK libseat/backend/seatd.c:45-72; open/validation libseat/backend/seatd.c:377-444; device/switch/disable libseat/backend/seatd.c:523-663; public API include/libseat.h:60-140; backend select libseat/libseat.c:37-71.
- Compat (read-only): Laputa runs foreground `seatd -g seat -l info`, socket `/run/seatd.sock`, proof env `SEATD_VTBOUND=0` + `LIBSEAT_BACKEND=seatd`; recipe builds logind-disabled/builtin-disabled with no seatd-launch. Demands: CLI `-g`, default `/run` socket, VTBOUND=0 seat, seatd-only libseat.
