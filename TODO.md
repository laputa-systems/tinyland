Prefix: TINY
Next: 14

# Open work

Priorities are assigned from impact and likelihood. Source-confirmed findings
below include a concrete event sequence; they are not claims of device or VM
qualification. Runtime proof gaps are stated in each ticket.

### TINY-1 · P2 · Flip completion drops newer redraws and completes their callbacks early

- **Area:** Concurrency
- **Where:** `tinyland/src/server.rs:373`, `tinyland/src/server.rs:699`, `tinyland/src/server.rs:981`, `tinyland/src/server.rs:1066`, `tinyland/src/server.rs:1070`, `tinyland/src/output.rs:399`.
- **Problem:** Submit scene A, then let a Wayland event wake the brokered, nonblocking loop before A's flip. Commit B updates the scene while `pending_present` is already true; submission is skipped because A remains in flight. A's completion clears that same flag and `presented()` drains callbacks from the current scene, including B. With no later activity, B is never displayed although its callbacks can report completion. Window retirement and cursor changes have the same lost-update path. This is source-confirmed; a running DRM reproduction remains unproven. The headless pump collapses submission and completion into one step and cannot expose the interval.
- **Recommendation:** Consume dirty state on submission, preserve later changes, and capture the callback batch belonging to that submitted scene. Completion must consume only that batch. Add a controlled A-submit/B-commit/A-complete regression that leaves B dirty and its callbacks pending, then proves B completes only after its own submission.
- **Fix with:** `make-invalid-states-unrepresentable`
- **Effort:** M (days)
- **Design:** [Presentation ownership](docs/rfc/TINY-1-presentation-state.md).

### TINY-2 · P2 · Disconnecting painted clients leaves the global memory budget charged

- **Area:** Concurrency
- **Where:** `tinyland/src/server/dispatch_impl.rs:197`, `tinyland/src/server/dispatch_impl.rs:205`, `tinyland/src/server.rs:132`, `tinyland/src/server.rs:145`, `Cargo.lock:808`.
- **Problem:** A secondary client commits pixels and disconnects with live surfaces. The pinned Rust backend removes the client before invoking object destructors. The surface destructor then calls `resource.client()`, gets `None`, and skips refunds for current and cached pixels. The vectors are freed but `total_bytes` and the disconnected client's ledger retain phantom charges. Repeated disconnects can exhaust the 256 MiB global budget and reject healthy clients. Exact `wayland-backend 0.3.17` source was checked against the lockfile checksum: `ClientStore::cleanup` removes the client before `State::cleanup` invokes destructors; `get_client` checks the client store. Explicit surface destruction while the connection remains alive does refund bytes with this backend. Socket-level reproduction remains unproven.
- **Recommendation:** Use the `ClientId` supplied to `destroyed` and refund both current and cached ownership exactly once. Exercise real explicit destruction and whole-client disconnect separately, keeping another client alive. Assert both ledgers return to their prior values and the healthy client can still commit; do not seed counters directly.
- **Fix with:** `lifecycle-concurrency`
- **Effort:** S (hours)

### TINY-3 · P2 · Concurrent restart can unlink a newly live daemon before taking the startup lock

- **Area:** Concurrency
- **Where:** `seatd/src/main.rs:31`, `seatd/src/main.rs:55`, `seatd/src/main.rs:72`, `seatd/src/admit.rs:22`, `seatd/src/admit.rs:136`, `seatd/harness/adversarial.rs:20`.
- **Problem:** The executable probes and removes a stale socket before `open_socket` acquires the startup lock. Two starters can both classify the old socket as stale. A pauses after probing; B removes it, binds, and starts listening; A resumes and unlinks B's live socket before binding its own. Both daemons can remain alive with separate session authority. This needs no hostile directory writer. The concurrency unit test calls only `open_socket`; the adversarial spawn helper unlinks the path itself, so neither proves executable startup ownership. The source interleaving is established; the real socket race remains unproven in this environment.
- **Recommendation:** Let one admission operation hold the lock from path inspection through reclamation and listen, with the `-z` policy passed explicitly. Add a deterministic two-process stale-restart case that pauses one contender before reclamation and verifies one owner, preserved live/non-socket paths, and correct `-z` refusal.
- **Fix with:** `lifecycle-concurrency`
- **Effort:** M (days)

### TINY-4 · P2 · A disable received during revoke recovery is still treated as an unplug

- **Area:** Concurrency
- **Where:** `tinyland/src/server.rs:949`, `tinyland/src/server.rs:998`, `tinyland/src/server.rs:999`, `tinyland/src/server.rs:1033`, `tinyland/src/server.rs:1043`, `tinyland/src/seat.rs:175`, `tinyland/src/seat.rs:432`.
- **Problem:** With one keyboard, the initial broker dispatch can precede a VT disable message while the next evdev read already returns ENODEV. `dispatch_wait(100)` then receives the disable and sets its flag, but the caller still queues the device as lost, removes it, and returns failure because no inputs remain. The normal suspend handler runs only at the next loop top, which is never reached. The source event order is established; the kernel/VT reproduction remains unproven.
- **Recommendation:** Apply a newly received disable before interpreting lost-device results. Share one suspend transition between normal dispatch and revoke recovery. Verify the single-device ordering keeps the compositor alive, drops devices, acknowledges disable, and resumes; retain a control where ENODEV without disable is a real unplug.
- **Fix with:** `lifecycle-concurrency`
- **Effort:** M (days)

### TINY-5 · P2 · A connected but stalled broker prevents compositor shutdown

- **Area:** Concurrency
- **Where:** `tinyland/src/seat.rs:332`, `tinyland/src/seat.rs:342`, `tinyland/src/seat.rs:364`, `tinyland/src/seat.rs:452`, `tinyland/src/server.rs:901`.
- **Problem:** Synchronous exchanges wait indefinitely for readability, and blocked writes wait indefinitely for writability. Both poll loops retry EINTR without checking the compositor's shutdown flag. A broker that accepts a request and stops replying therefore freezes startup, VT switching, release, or resume. TERM only sets a flag that the outer loop cannot inspect until the request returns. Disconnection detection does not cover a still-connected peer. The wait/cancellation failure is source-confirmed; a live socket stall reproduction remains unproven.
- **Recommendation:** Give each transaction one monotonic deadline and a cancellation path connected to shutdown, or drive the same transaction incrementally from the main poll loop. Use the existing fake-broker pattern to keep a request unanswered and assert bounded cancellation, including saturated writes. Interruptions must not restart the deadline.
- **Fix with:** `lifecycle-concurrency`
- **Effort:** M (days)

### TINY-6 · P2 · Device loss and suspension leave delivered keys and pointer grabs pressed

- **Area:** Concurrency
- **Where:** `tinyland/src/server.rs:657`, `tinyland/src/server.rs:949`, `tinyland/src/server.rs:959`, `tinyland/src/server.rs:1010`, `tinyland/src/server.rs:1041`, `tinyland/src/input.rs:144`, `tinyland/src/server/pointer.rs:100`, `tinyland/src/server/pointer.rs:186`.
- **Problem:** Removing input resets KBVM without sending releases or modifiers to clients. Suspend drops descriptors without reconciling delivered presses; resume explicitly releases only the compositor's Ctrl/Alt/Fn chord. Hold a key or button, then remove its device or release it while externally switched away: the release never reaches the client. A key can keep repeating, and `PointerState.pressed` can retain an implicit grab that prevents refocusing. The mixed event list also drops device identity, so one device cannot release only its own state. These transitions are source-confirmed; device-level reproduction remains unproven.
- **Recommendation:** Own delivered keys and buttons per input device, retain that identity through dispatch, and reconcile aggregate seat state on device loss and suspend. Verify releases, modifiers, pointer refocusing, and resume with held inputs. Include two devices holding the same code so removing one does not release the other's press.
- **Fix with:** `lifecycle-concurrency`
- **Effort:** M (days)
- **Design:** [Input ownership](docs/rfc/TINY-6-input-state.md).

### TINY-7 · P2 · Rendered subsurfaces cannot receive pointer focus

- **Area:** Architecture
- **Where:** `tinyland/src/server/pointer.rs:39`, `tinyland/src/server/pointer.rs:46`, `tinyland/src/server/pointer.rs:66`, `tinyland/src/server.rs:710`, `tinyland/src/server/popup.rs:25`.
- **Problem:** Rendering recursively visits committed subsurface stacking and positions, but `surface_at` visits only popup roots and the active toplevel. A visible child over its parent sends clicks to the parent with parent-local coordinates; a child extending outside it has no target. `surface_origin` also lacks subsurface ancestry, so adding children to the candidate list alone is insufficient. The mismatch is source-confirmed; a real client reproduction remains unproven.
- **Recommendation:** Traverse the committed surface tree in reverse presentation order with accumulated origins for hit-testing and coordinate conversion. Preserve implicit grabs. Exercise an overlapping child, an out-of-parent child, and a nested child, asserting entered surface IDs and local coordinates through the Wayland boundary.
- **Fix with:** `single-source-of-truth`
- **Effort:** M (days)

### TINY-8 · P2 · Ignored input regions let excluded pixels intercept clicks

- **Area:** Types
- **Where:** `tinyland/src/server/dispatch_impl.rs:94`, `tinyland/src/server/dispatch_impl.rs:104`, `tinyland/src/server/dispatch_impl.rs:159`, `tinyland/src/surface.rs:5`, `tinyland/src/server/pointer.rs:46`.
- **Problem:** `set_input_region` and region subtraction are no-ops, and no surface state retains the assigned region. A transparent popup with an empty or restricted input region still captures input across its full buffer rectangle. The [Wayland input-region contract](https://wayland.freedesktop.org/docs/html/apa.html#protocol-spec-wl_surface-request-set_input_region) requires excluded pixels to try the next surface. Unlike ignoring opaque regions, this changes input delivery. Source confirms the missing state and no-op handlers; runtime reproduction remains unproven.
- **Recommendation:** Copy region state on assignment, preserve null as infinite and an empty region as no input, implement subtraction, and apply the region at the appropriate surface commit boundary. Use it in hit-testing. Verify click-through, a subtracted hole, destruction of the source region after assignment, and synchronized/unsynchronized commit timing.
- **Fix with:** `make-invalid-states-unrepresentable`
- **Effort:** M (days)

### TINY-9 · P2 · A closed readiness descriptor can close the daemon's signal descriptor

- **Area:** Errors
- **Where:** `seatd/src/cli.rs:95`, `seatd/src/loop.rs:31`, `seatd/src/main.rs:92`, `seatd/src/loop.rs:249`, `seatd/src/loop.rs:267`.
- **Problem:** `-n 3` is accepted when descriptor 3 is absent. With only standard descriptors open, `Server::new` reuses 3 for its signalfd. The later readiness write fails with EINVAL and unconditionally closes that FD while the server still owns it. Poll then returns POLLNVAL, which the signal branch ignores, leaving a busy loop with TERM/INT blocked and no usable signalfd. A source-inclusion probe reproduced the real allocation, production readiness block, and three successful `poll_once` returns with `running == true`. It used the pinned compiler's GNU sysroot libc, simplified session initialization, and unexercised connection/device adapters, not a complete workspace build.
- **Recommendation:** Validate and acquire the inherited readiness descriptor before creating internal descriptors. Reject a closed descriptor before setup and keep readiness in an owned type. Treat POLLNVAL on a required internal FD as terminal. Verify a valid inherited pipe and an absent FD that would otherwise be reused by signalfd.
- **Fix with:** `lifecycle-concurrency`
- **Effort:** S (hours)

### TINY-10 · P2 · Client geometry overflows before clipping

- **Area:** Errors
- **Where:** `tinyland/src/positioner.rs:119`, `tinyland/src/positioner.rs:150`, `tinyland/src/positioner.rs:165`, `tinyland/src/server/dispatch_impl.rs:1183`, `tinyland/src/server/popup.rs:64`, `tinyland/src/server/pointer.rs:148`.
- **Problem:** Popup placement adds, subtracts, and negates client-controlled i32 coordinates without widening. A probe including the unchanged positioner, with compile-only protocol enum shims, panics with overflow checks and wraps when optimized. One input is size 8x8, anchor rectangle (40,40,4,4), default anchor/gravity, offset (i32::MAX,0), and bounds (0,0,100,100). Another uses size 2x2, anchor (0,0,2,2), offset (i32::MIN,0), and FlipX. The first optimized result places x at -2147483611. Cursor hotspot subtraction and parent-origin negation share the unsafe arithmetic boundary. No optimized compositor crash is claimed; the full client path remains unproven at runtime.
- **Recommendation:** Widen geometry arithmetic, perform constraint decisions and clipping before checked conversion, and report invalid requests through the protocol. Cover extreme coordinates, offsets, hotspots, and nested popup origins in both overflow-checking and optimized builds. Do not clamp early enough to change constraint-adjustment decisions.
- **Fix with:** `os-boundaries`
- **Effort:** S (hours)

### TINY-11 · P3 · Retired seat bindings remain counted and retained

- **Area:** Concurrency
- **Where:** `tinyland/src/input.rs:7`, `tinyland/src/server/dispatch_impl.rs:669`, `tinyland/src/server/dispatch_impl.rs:687`, `tinyland/src/server/dispatch_impl.rs:745`, `tinyland/src/server/dispatch_impl.rs:751`.
- **Problem:** The seat destructor never retires `SeatData`. In the pinned Rust backend, `resource.client()` still succeeds for a destroyed factory while its connection remains alive, so eight bind/release cycles with no surviving children still consume the eight-seat limit. A ninth bind disconnects the client with a memory error. After disconnect, records stop counting toward that client but remain in the vector traversed by input and cleanup. No consumer that routinely rebinds was established; socket reproduction remains unproven. Surviving keyboards and pointers do not lose client identity merely because the factory is released.
- **Recommendation:** Track factory retirement separately from child lifetime, count live factory bindings, and retire records when the factory and children are all gone. Preserve surviving keyboard and pointer objects. Verify rebinding after eight complete retirements and return to baseline record counts after disconnect.
- **Fix with:** `lifecycle-concurrency`
- **Effort:** M (days)

### TINY-12 · P3 · Direct DRM event reads can block past the flip deadline

- **Area:** Concurrency
- **Where:** `tinyland/src/output.rs:23`, `tinyland/src/output.rs:33`, `tinyland/src/output.rs:423`, `tinyland/src/server.rs:919`, `tinyland/src/server.rs:977`, `seatd/src/effect.rs:30`.
- **Problem:** Direct `Card::open` uses a blocking file. The shared poll loop discards readiness details and calls `poll_flip` whenever a flip is pending, including after an unrelated Wayland/input wake. Exact locked `drm 0.15.0` source confirms `receive_events` immediately performs `rustix::io::read`. The read can block before the two-second deadline check. The normal brokered path supplies O_NONBLOCK, so this affects direct/proof operation. Source and dependency behavior are established; hardware reproduction remains unproven.
- **Recommendation:** Establish nonblocking mode for every adopted DRM descriptor and preserve readiness-aware event draining. Verify that an unready descriptor returns promptly and unrelated poll activity cannot postpone a missing-flip timeout.
- **Fix with:** `os-boundaries`
- **Effort:** S (hours)

### TINY-13 · P3 · Legal long device paths exceed the transport's complete-frame capacity

- **Area:** Data
- **Where:** `seatd-protocol/src/conn.rs:8`, `seatd-protocol/src/conn.rs:258`, `seatd/src/loop.rs:163`, `seatd/src/session.rs:461`, `tinyland/src/seat.rs:241`.
- **Problem:** The wire and client accept a path field of up to 256 bytes including NUL. The daemon buffers at most 256 bytes total and restores the four-byte header until the entire body is available. A path of 251 bytes plus its two-byte length and four-byte header already requires 257 bytes. The full ring cannot dispatch that frame, and the next read returns EOVERFLOW and disconnects the client. Paths up to 250 bytes fit. This is source-confirmed; no live socket reproduction was run. Normal device paths are much shorter, so this is a boundary compatibility issue.
- **Recommendation:** Keep transport storage bounded while admitting the maximum legal frame. Use a power-of-two ring capacity large enough for framing overhead, as its mask requires, or incrementally consume validated frame fields. Verify 250, 251, and 256-byte path fields, fragmented delivery, and rejection above the wire limit without corrupting a following frame.
- **Fix with:** `durable-state`
- **Effort:** S (hours)
