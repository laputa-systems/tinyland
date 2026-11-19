Prefix: DSLP
Next: 9

Deslop campaign backlog ([plan](https://github.com/laputa-systems/stuff/blob/main/docs/campaigns/deslop.md), tier B). One agent works this file with the `deslop` skill: take the lowest-numbered open ticket whose area does not start with `Needs owner ·`. After DSLP-5, work the open P1 and P2 findings, then fill in `## Report` below.

## Report

DSLP-5 closed, Dead code, 2026-11-19, base 03d30cf: Linux ARM64 in the
pinned Alpine image, nightly-2026-11-09, default and all features, all targets.
Both `cargo check` runs with `-W dead_code -W unreachable_pub` report no such
warnings. Cargo warns that `xkbcommon` is unused with all features: the
`oracle` integration test uses it (`tinyland/tests/kbvm_oracle.rs:41`); it is
not a runtime dependency. cargo-machete 0.10.1 found no unused dependencies.

- Removed the unread `Daemon.sock` test-harness field: both constructors pass
  the socket to the child and tests use their own path; Drop only uses the
  child (`seatd/harness/adversarial.rs:14-65`). The adversarial suite passed.
- Restricted the keyboard lookup table and `key_utf8` to unit tests: the
  keymap tests call them (`tinyland/src/input.rs:541`); the compositor sends
  keycodes and keymap to clients, and no runtime caller translates to UTF-8.
  The previous `reset_stuck` suppression was unnecessary: suspend paths call
  it (`tinyland/src/server.rs:898`, `tinyland/src/server/remote.rs:232`).
- Narrowed 109 unreachable public declarations to crate or parent visibility.
  Their modules are private (`tinyland/src/lib.rs:1-11`); the compositor server
  uses their input, damage, seat, selection, positioning, shell, shm and
  surface operations (`tinyland/src/server.rs:1-9`), and its dispatch and
  pointer helpers are internal. No exported protocol crate API was removed.
  Server lifecycle tests consume the remote transport test helpers
  (`tinyland/src/server/tests/lifecycle.rs:433` and :610).

Checked workspace manifests, entry points, feature gates, registrations,
scripts and references for the removals; no external consumer or package
reference to the removed private fields exists. `make test` and `make lint`
passed; no downstream bctl candidate test was run because its public API and
wire contracts are unchanged. Real devices, VT, VM and oracle runtime remain
unqualified. No owner walk-back was identified. The structural debloat review
was limited to the changed mechanisms and their callers; a complete structural
sweep of every subsystem was not performed.

Tests (DSLP-4): [docs/deslop-tests.md](docs/deslop-tests.md) maps the critical
contracts to their protecting tests. New boundary tests pin the per-client
region object and rectangle limits and the shm backing-file size boundary;
each fails with its fault injected. A bounded cargo-mutants trial on the
region-object guard (32 runner-seconds) found no survivor. Ephemeral coverage
reported 77.76% lines and 58.56% branches. No retained mutation or coverage
lane is added: cargo-mutants and cargo-llvm-cov are not on the crate mirror.

Ground, 2026-11-19, base 109c678: the pinned Alpine suite passed on Linux
ARM64 under Docker on macOS ARM64. With an initially empty persistent Linux
kache cache and fresh Cargo target each time, `make test` took 49.69s cold and
17.41s warm. `make lint` passed strict Clippy in 54.64s including an image
rebuild. The host was busy, so these are observations, not performance gates.
The suite leaves idle timing and keymap regeneration ignored; device, VT, guest,
oracle, and downstream qualification remain separate. No vendored source trees
are tracked; the generated keymap provenance is recorded in AGENTS.md.

Diagnose, 2026-11-19, base 5a58058: reviewed the untrusted-input and privilege
paths through the ultrareview lenses (os-boundaries, security, error-model,
lifecycle-concurrency, durable-state). Read and assessed: Wayland request
dispatch and commit transactions, shm pool and buffer geometry and fd handling,
damage arithmetic, the remote protocol reader and writer, seatd admission,
device path resolution and identity checks, the SCM_RIGHTS transport, the
session and VT state machine, and the compositor's per-client caps. Filed
DSLP-6 (P2, uncapped positioner and pointer creation), DSLP-7 (P3, seatd spins
on accept failure), and DSLP-8 (P3, remote damage area unbounded); each was
reproduced against the built product with a headless compositor client, the
pinned-image daemon, or the remote-protocol reader before filing. Not covered:
real DRM devices, VT transitions and logind signals, libseat ABI compatibility
against an external client, the WLCS adapter, the QEMU guest proofs, downstream
bctl, and performance. One observation is recorded here and not filed because it
was not reproduced: `Region::contains` (tinyland/src/surface.rs:31-44) takes the
last matching operation, so subtract-then-add of overlapping rectangles keeps
the subtracted area, unlike the order-independent set semantics of `wl_region`.

DSLP-3 closed, Types, 2026-11-19, base 7d1bb584: measured each Rust strict-profile
candidate in the pinned Alpine image using workspace all-target Clippy, one
`-W` candidate per run. Counts below are unique primary diagnostic locations
across all targets (including tests); production counts exclude harnesses, test
files and inline test modules. There are 16,643 tracked Rust lines, so more than
three false positives exceeds one per 5,000 lines. No new profile setting was
adopted; the existing `-D warnings` profile remains clean.

- `unsafe_code`: 126 total, 61 production; reject `forbid`: daemon and shared
  transport use libc for actual FD, socket, VT and device operations.
- `wildcard_enum_match_arm`: 10 total, 9 production; reject: fallback cases
  include Wayland protocol enums and intentionally ignore unsupported requests.
- `match_wildcard_for_single_variants`: 0; reject, no defect or hatch witness.
- `cast_possible_truncation`: 97 total, 51 production; reject: bounded wire
  lengths and clipped geometry yield many findings. Four geometry casts were
  replaced with checked conversions, but the setting exceeds the noise budget.
- `cast_sign_loss`: 83 total, 39 production; reject: e.g. recvmsg's negative
  result is handled before its conversion; other validated dimensions recur.
- `unwrap_used`: 666 total, 36 production; reject: guarded map/FD invariants and
  fixed-width slice conversions produce numerous findings. Tests account for
  most occurrences; no test policy was adopted.
- `expect_used`: 46 total, 7 production; reject: internal lifecycle invariants
  and infallible fixed error strings are flagged; test assertions dominate.
- `redundant_clone`: 8 total; reject: no demonstrated type defect or escape
  hatch removed by this setting; clone elimination is outside this type audit.
- `needless_pass_by_value`: 4 total; reject: ownership can itself be intentional
  (shutdown consumes the server), and no type defect or hatch was demonstrated.

The clipped damage coordinate contract formerly depended on four unchecked
signed-to-unsigned casts. `damage::clip` now uses checked conversions after its
intersection guard; its existing clipping test and a large-coordinate boundary
witness cover the result. The old bounds establish that all four conversions
succeed when the guard passes, so this does not change accepted geometry.
First-party Rust lexical counts, excluding harness/test files and text after
the first `#[cfg(test)]` in each remaining file: unsafe blocks 60 -> 60, allow
attributes 6 -> 6, unwrap/expect 44 -> 44, `as` tokens 276 -> 272. Whole
tracked Rust counts respectively: 132 -> 132, 7 -> 7, 726 -> 726, 440 -> 436.
These conservative lexical counts may include comments and `as` import aliases;
the production approximation excludes any code after an inline test module. DSLP-6, DSLP-7 and DSLP-8 remain open. Remaining
leads, ranked: seatd FD lifecycle unwraps, compositor surface-map unwraps,
and shm validated signed dimensions; these need separate contract tracing.
Real devices, VT transitions, downstream bctl and performance are unqualified;
this change preserves the public and wire contracts.

## Tickets







### DSLP-6 · P2 · A Wayland client can create unbounded xdg_positioner and wl_pointer objects

- **Area:** Security
- **Where:** tinyland/src/server/dispatch_impl.rs:997 (xdg_wm_base.create_positioner), tinyland/src/server/dispatch_impl.rs:715 (wl_seat.get_pointer) and :728 (wl_seat.get_keyboard), tinyland/src/server.rs:69-83 (per-client caps)
- **Problem:** The compositor caps the object types it retains in per-client maps (surfaces 128, toplevels 32, buffers 64, pools 16, regions 32, sources 32, devices 16, seats 8, outputs 8, callbacks 32), but two creation paths have no count check and are not charged to the per-client byte budget: `xdg_wm_base.create_positioner` inserts into `State::positioners` (dispatch_impl.rs:997-1000), and `wl_seat.get_pointer`/`get_keyboard` push into `SeatData::pointers`/`keyboards` (dispatch_impl.rs:715-756) for each of up to eight seats. Reproduced against the headless compositor: one client created 300000 positioners, received no error, and still completed a wl_display.sync round trip, while resident memory grew by 61.4 MiB (about 215 bytes each); the same client created 300000 pointers on one seat, again with no error, growing memory by 39.5 MiB. The same harness shows the region cap rejecting the 33rd region with `too many regions for client`, so the policy is real for the other object types. A buggy or hostile client can exhaust compositor memory and end the session; 1M each would be roughly 200 MiB and 130 MiB.
- **Recommendation:** Add a per-client positioner cap and a per-seat pointer and keyboard cap beside the existing limits, and reject past each with the same wl_display NoMemory error the other caps use, so the offender is disconnected. Do not count an object after the client destroys it. Verify with headless tests that the create past each limit errors and the client keeps its earlier objects.
- **Fix with:** lifecycle-concurrency
- **Effort:** S


### DSLP-7 · P3 · seatd spins and refuses new clients when accept fails

- **Area:** Reliability
- **Where:** seatd/src/loop.rs:137-146 (handle_listen), seatd/src/loop.rs:274-292 (listen revents), seatd/src/admit.rs:300-317 (accept_client)
- **Problem:** accept_client returns None when accept4 fails, after logging; handle_listen then leaves the pending connection queued and sets neither `dirty` nor any backoff. The listen descriptor stays readable, so poll returns immediately and the daemon spends a full core on failing accepts while admitting nobody. Reproduced in the pinned Alpine image with the daemon at a 24-descriptor limit (`ulimit -n 24`), holding 36 client connections: 747 connects were refused over 4 s, daemon CPU was 0.99 CPU-seconds per wall second, and a fresh client's connect was refused; after the held sockets closed, CPU returned to 0.00 and a fresh client received its PONG. At the default log level each iteration also writes one line, so the spin floods the log. The trigger is the daemon's descriptor budget, so the shipped 1024-descriptor default needs about 1020 held connections; any process that can connect, which the README names as the admission boundary, can reach it. Already-accepted clients keep being serviced, so impact is refusal of new sessions plus CPU and log burn until descriptors free.
- **Recommendation:** On accept failure, stop selecting the listen descriptor, or back off to a deadline, until a client slot frees, and bound the error log rate. Verify with a socket test that the daemon does not spin and that admission recovers when descriptors free.
- **Fix with:** error-model
- **Effort:** S


### DSLP-8 · P3 · Remote damage messages can materialize far more pixels than one frame

- **Area:** Data
- **Where:** remote-protocol/src/lib.rs:135-211 (read_compositor tags 3 and 4), remote-protocol/src/lib.rs:314-323 (damage_header), remote-protocol/src/lib.rs:276-312 (write_tiled_damage)
- **Problem:** read_compositor bounds each damage rectangle to the output Size and the rectangle count to MAX_DAMAGE_RECTS (64), but never bounds the sum of the rectangle areas. Overlapping and repeated rectangles are accepted, and a tiled rectangle (tag 4) encodes a solid 64x64 tile in five bytes, so a small message materializes a large Vec per rectangle. Reproduced by feeding hand-built messages to read_compositor for Size 4096x4096: 1,291,269 wire bytes carrying 63 full-output solid-tile rectangles returned 4,227,858,432 damage bytes (4.03 GiB, about 3270x the input) and raised the reader's resident set by 4032 MiB; eight such rectangles needed 163,989 wire bytes for 512 MiB. That contradicts the module's own intent that a damage message is bounded by one output, with excess rectangles collapsed to a bounding box. A hostile or compromised compositor can exhaust the reading host's memory; a reader-side bound also protects other future senders.
- **Recommendation:** Reject a damage message whose rectangle areas sum past size.pixel_len(), or require rectangles to be disjoint, in both tag 3 and tag 4; keep the writers' current invariants. Add tests for overlapping and repeated rectangles in both encodings.
- **Fix with:** os-boundaries
- **Effort:** S

