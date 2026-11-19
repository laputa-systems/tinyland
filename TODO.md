Prefix: TINY
Next: 17

# Open work

### TINY-14 · P2 · Incremental Wayland damage is discarded across composition and scanout

- **Area:** Performance
- **Where:** `tinyland/src/server/dispatch_impl.rs`, `tinyland/src/shm.rs`, `tinyland/src/server.rs`, `tinyland/src/output.rs`
- **Problem:** The remote and DRM paths now track incremental damage (2026-11-18); DRM repairs missed damage independently for each scanout buffer. On 2026-11-18, bctl's SmolVM runtime ran it on virtio-gpu's double-buffered scanout for idle, typing, scrolling, animation, video and dashboard pages. Every page loaded and passed its content checks (joshuarli/bctl tests/bench/results/2026-11-18-realistic-headed/#smolvm). Exact-pixel comparison through a real device, physical DRM devices and VT transitions remain pending. The remote path: Damage and DamageBuffer are tracked, commits patch only damaged rows, and only damaged output rectangles are recomposed and sent, paced to 30 frames a second.
- **Recommendation:** Continue [the upstream damage campaign](docs/CAMPAIGN-DAMAGE.md): qualify real DRM devices, VT transitions, callback ordering and recovery, and the pinned downstream bctl integration. Complete the remaining fixture and performance gates before closing.
- **Fix with:** `make-it-fast`
- **Effort:** L

Priorities are assigned from impact and likelihood. Source-confirmed findings
below include a concrete event sequence; they are not claims of device or VM
qualification. Runtime proof gaps are stated in each ticket.

### TINY-11 · P3 · Retired seat bindings remain counted and retained

- **Area:** Concurrency
- **Where:** `tinyland/src/input.rs:7`, `tinyland/src/server/dispatch_impl.rs:669`, `tinyland/src/server/dispatch_impl.rs:687`, `tinyland/src/server/dispatch_impl.rs:745`, `tinyland/src/server/dispatch_impl.rs:751`.
- **Problem:** The seat destructor never retires `SeatData`. In the pinned Rust backend, `resource.client()` still succeeds for a destroyed factory while its connection remains alive, so eight bind/release cycles with no surviving children still consume the eight-seat limit. A ninth bind disconnects the client with a memory error. After disconnect, records stop counting toward that client but remain in the vector traversed by input and cleanup. No consumer that routinely rebinds was established; socket reproduction remains unproven. Surviving keyboards and pointers do not lose client identity merely because the factory is released.
- **Recommendation:** Track factory retirement separately from child lifetime, count live factory bindings, and retire records when the factory and children are all gone. Preserve surviving keyboard and pointer objects. Verify rebinding after eight complete retirements and return to baseline record counts after disconnect.
- **Fix with:** `lifecycle-concurrency`
- **Effort:** M

### TINY-12 · P3 · Direct DRM event reads can block past the flip deadline

- **Area:** Concurrency
- **Where:** `tinyland/src/output.rs:23`, `tinyland/src/output.rs:33`, `tinyland/src/output.rs:423`, `tinyland/src/server.rs:919`, `tinyland/src/server.rs:977`, `seatd/src/effect.rs:30`.
- **Problem:** Direct `Card::open` uses a blocking file. The shared poll loop discards readiness details and calls `poll_flip` whenever a flip is pending, including after an unrelated Wayland/input wake. Exact locked `drm 0.15.0` source confirms `receive_events` immediately performs `rustix::io::read`. The read can block before the two-second deadline check. The normal brokered path supplies O_NONBLOCK, so this affects direct/proof operation. Source and dependency behavior are established; hardware reproduction remains unproven.
- **Recommendation:** Establish nonblocking mode for every adopted DRM descriptor and preserve readiness-aware event draining. Verify that an unready descriptor returns promptly and unrelated poll activity cannot postpone a missing-flip timeout.
- **Fix with:** `os-boundaries`
- **Effort:** S

### TINY-13 · P3 · Legal long device paths exceed the transport's complete-frame capacity

- **Area:** Data
- **Where:** `seatd-protocol/src/conn.rs:8`, `seatd-protocol/src/conn.rs:258`, `seatd/src/loop.rs:163`, `seatd/src/session.rs:461`, `tinyland/src/seat.rs:241`.
- **Problem:** The wire and client accept a path field of up to 256 bytes including NUL. The daemon buffers at most 256 bytes total and restores the four-byte header until the entire body is available. A path of 251 bytes plus its two-byte length and four-byte header already requires 257 bytes. The full ring cannot dispatch that frame, and the next read returns EOVERFLOW and disconnects the client. Paths up to 250 bytes fit. This is source-confirmed; no live socket reproduction was run. Normal device paths are much shorter, so this is a boundary compatibility issue.
- **Recommendation:** Keep transport storage bounded while admitting the maximum legal frame. Use a power-of-two ring capacity large enough for framing overhead, as its mask requires, or incrementally consume validated frame fields. Verify 250, 251, and 256-byte path fields, fragmented delivery, and rejection above the wire limit without corrupting a following frame.
- **Fix with:** `durable-state`
- **Effort:** S

### TINY-15 · P3 · offer a remote-only Tinyland build without physical device dependencies

- **Area:** Build
- **Where:** tinyland/Cargo.toml; tinyland/src/server/remote.rs; tinyland/src/seat.rs; tinyland/src/output.rs; bctl guest/tinyland.rev integration
- **Problem:** The Nanos appliance uses only Tinyland's remote backend and opens no physical seat, DRM/KMS or evdev device. Its compositor executable still includes physical-backend code and dependency obligations. The actual removable size is unmeasured; physical SmolVM and native users still require those backends.
- **Recommendation:** Add an explicit upstream remote-only build configuration that excludes unused physical device plumbing without duplicating compositor ownership in bctl. Preserve the physical backend and seatd consumers. Verify Linux remote workspace/render/input/clipboard/lifecycle tests, inspect dependency closure, then pin and qualify the downstream Nanos image in both modes.
- **Fix with:** `api-surface-and-module-boundaries`
- **Effort:** M

### TINY-16 · P3 · seatd's crash-reclaim admission test fails intermittently

- **Area:** Tests
- **Where:** `seatd/src/admit.rs`, `admit::tests::crash_without_unlink_is_reclaimed_by_next_owner`
- **Problem:** On 2026-11-18 one `cargo test --locked --workspace` run in the pinned test image failed this test at `seatd/src/admit.rs:434`, and an immediate rerun of `cargo test -p seatd --lib` passed all 49 tests. No seatd code had changed. A flaky test can hide a real admission regression or block unrelated changes.
- **Recommendation:** Run the test in a loop to measure its failure rate, find the timing or shared-state assumption that the assertion depends on, and make the test deterministic without weakening what it checks.
- **Fix with:** `root-cause`
- **Effort:** S

