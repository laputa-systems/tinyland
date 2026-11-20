Prefix: TINY
Next: 22

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

### TINY-17 · P3 · seatd spins and refuses new clients when accept fails

- **Area:** Reliability
- **Where:** seatd/src/loop.rs:137-146 (handle_listen), seatd/src/loop.rs:274-292 (listen revents), seatd/src/admit.rs:300-317 (accept_client)
- **Problem:** accept_client returns None when accept4 fails, after logging; handle_listen then leaves the pending connection queued and sets neither `dirty` nor any backoff. The listen descriptor stays readable, so poll returns immediately and the daemon spends a full core on failing accepts while admitting nobody. Reproduced in the pinned Alpine image with the daemon at a 24-descriptor limit (`ulimit -n 24`), holding 36 client connections: 747 connects were refused over 4 s, daemon CPU was 0.99 CPU-seconds per wall second, and a fresh client's connect was refused; after the held sockets closed, CPU returned to 0.00 and a fresh client received its PONG. At the default log level each iteration also writes one line, so the spin floods the log. The trigger is the daemon's descriptor budget, so the shipped 1024-descriptor default needs about 1020 held connections; any process that can connect, which the README names as the admission boundary, can reach it. Already-accepted clients keep being serviced, so impact is refusal of new sessions plus CPU and log burn until descriptors free.
- **Recommendation:** On accept failure, stop selecting the listen descriptor, or back off to a deadline, until a client slot frees, and bound the error log rate. Verify with a socket test that the daemon does not spin and that admission recovers when descriptors free.
- **Fix with:** error-model
- **Effort:** S

### TINY-18 · P3 · Remote damage messages can materialize far more pixels than one frame

- **Area:** Data
- **Where:** remote-protocol/src/lib.rs:135-211 (read_compositor tags 3 and 4), remote-protocol/src/lib.rs:314-323 (damage_header), remote-protocol/src/lib.rs:276-312 (write_tiled_damage)
- **Problem:** read_compositor bounds each damage rectangle to the output Size and the rectangle count to MAX_DAMAGE_RECTS (64), but never bounds the sum of the rectangle areas. Overlapping and repeated rectangles are accepted, and a tiled rectangle (tag 4) encodes a solid 64x64 tile in five bytes, so a small message materializes a large Vec per rectangle. Reproduced by feeding hand-built messages to read_compositor for Size 4096x4096: 1,291,269 wire bytes carrying 63 full-output solid-tile rectangles returned 4,227,858,432 damage bytes (4.03 GiB, about 3270x the input) and raised the reader's resident set by 4032 MiB; eight such rectangles needed 163,989 wire bytes for 512 MiB. That contradicts the module's own intent that a damage message is bounded by one output, with excess rectangles collapsed to a bounding box. A hostile or compromised compositor can exhaust the reading host's memory; a reader-side bound also protects other future senders.
- **Recommendation:** Reject a damage message whose rectangle areas sum past size.pixel_len(), or require rectangles to be disjoint, in both tag 3 and tag 4; keep the writers' current invariants. Add tests for overlapping and repeated rectangles in both encodings.
- **Fix with:** os-boundaries
- **Effort:** S

### TINY-19 · P3 · Establish representative compositor performance evidence

- **Area:** Performance
- **Where:** docs/CAMPAIGN-DAMAGE.md; tinyland/src/server/tests/damage.rs:88
- **Problem:** The 300-step headless damage differential checks pixels and sparse updates but does not measure shipped frame throughput, latency, CPU, allocations or bytes. The headed bctl comparison is historical and lacks a reproducible pinned baseline and retained samples here. No current evidence detects a performance regression or attributes it across compositor, guest and viewer.
- **Recommendation:** Extend the existing headed damage campaign with pinned candidate and reference, completed-frame and pixel checks, interleaved gated samples, stage CPU and byte counters, and separate sampling or tracing for attribution. Qualify actual devices and downstream separately.
- **Fix with:** bench-pyramid
- **Effort:** L

### TINY-20 · P3 · Measure client density, seatd responsiveness and startup

- **Area:** Performance
- **Where:** seatd/harness/storm.rs:201; seatd/doc/validation.md:55; tinyland/docs/verification.md:92
- **Problem:** The switch storm checks 60 completed transitions and post-cleanup daemon RSS and descriptors, not per-client retained or peak memory, latency or throughput. The ignored idle test samples only daemon CPU ticks and voluntary switches over five seconds. Neither component has a startup measurement or an interleaved baseline; regressions in these outcomes can go unnoticed.
- **Recommendation:** Add representative client-density, request-response and startup journeys with completion checks, per-process and container memory scope, operation counts, gated interleaved samples and an accepted reference. Use separate CPU sampling, allocation and syscall tracing for attribution; document overhead and host validity.
- **Fix with:** bench-pyramid
- **Effort:** M

### TINY-21 · P3 · Retire destroyed seats without losing input object accounting

- **Area:** Resource lifecycle
- **Where:** tinyland/src/server/dispatch_impl.rs:675-703, :717-740, :772-779, :796-831
- **Problem:** The seat destroyed callback does nothing, so State.seats retains seat records after release or disconnect. On seat release, input children may remain live and their vectors remain attached to the retained seat. The new pointer and keyboard limits are per seat, while seat admission counts only resources whose client lookup succeeds. Rebinding after release may therefore evade the intended per-client resource bound; repeated disconnects also retain seat records. The current disconnect test checks emptied input vectors but not removal of the seat. The rebind impact has not been reproduced.
- **Recommendation:** Exercise release, rebind and disconnect with live pointer and keyboard children, then account for their actual lifetimes and retire seat state without losing live input resources. Assert bounded retained state and continued input delivery.
- **Fix with:** lifecycle-concurrency
- **Effort:** M

