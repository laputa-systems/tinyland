Prefix: TINY
Next: 14

# Open work

Priorities are assigned from impact and likelihood. Source-confirmed findings
below include a concrete event sequence; they are not claims of device or VM
qualification. Runtime proof gaps are stated in each ticket.

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
