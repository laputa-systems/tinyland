# Roadmap

This is the single plan for this repository: all current and future work, and the handoff for resuming it. Campaigns are sections below, and their items interleave in **Sequence**. "Resume the roadmap" means continue from **Status**.

## Status

- State: active; campaign: incremental Wayland damage and scanout qualification (TINY-14).
- In progress: remaining differential, fault, device, and downstream qualification; no individual slice is recorded as underway.
- Last accepted revision: not recorded; integration branch green: not recorded.
- Parked branches: none recorded.
- Next slice: extend differential and fault qualification for exact pixels and callback ordering.
- Resume prompt: “resume the roadmap” (“resume the campaign” means the same).

## Sequence

1. **TINY-14-D · Incremental damage:** Extend the independent full-render oracle and fault fixtures for exact pixels, callback ordering, and conservative recovery. Owned files: `tinyland/src/server/tests/damage.rs`, `tinyland/src/output.rs`, and related compositor tests. Prerequisites: the implemented remote and scanout damage paths. Acceptance command: `make test`; also demonstrate that broken accumulation fails the harness. See the campaign's required fixtures and hard gates.
2. **TINY-19 · Incremental damage:** Establish representative compositor performance evidence with pinned candidate and reference, completed frames, pixel checks, interleaved gated samples, stage CPU and byte counters, and separate attribution. Owned files: `tinyland/src/server/tests/damage.rs` and the downstream headed benchmark. Prerequisites: a repeatable workload and a quiet qualified host. Acceptance: run `perf-gate-ready` immediately before and `perf-gate-ready -1` immediately after the interleaved measurements; compare the results against the campaign gates. The workload command and pinned reference still need to be established.
3. **TINY-14-C · Incremental damage:** Qualify the double-buffered device path, VT transitions, and pinned bctl integration. Owned files: `tinyland/src/output.rs`, device qualification artifacts, and the downstream bctl pin. Prerequisites: upstream tests pass and the region/generation contract is documented. Acceptance: run the applicable device and guest procedures in [compositor verification](../tinyland/docs/verification.md), then demonstrate BCTL-22 integration; an exact device-specific command remains to be established.

## Campaign: Incremental Wayland damage and scanout qualification

### Goal and state

**Active · TINY-14 · P2.** Complete transactional damage and scanout qualification without changing the minimal software-only, DRM, and seatd architecture. Downstream integration is tracked by joshuarli/bctl BCTL-22 and BCTL-28. Keep TINY-11–13 independent.

### Decisions

1. Track pending surface-local damage (Damage and DamageBuffer with correct coordinate semantics, scaling, transforms and clipping), committed state, synchronized subsurface state, and output-space accumulated damage separately. A commit is the transactional boundary; multiple commits before a flip must union, never replace, dirty regions. New buffers, size/scale/transform changes, first presentation, invalidated contents and unknown age conservatively mark full extents.
2. Preserve surface and SHM buffer lifetime: snapshot only safely accessible dirty pixels into owned persistent storage, never retain borrowed mappings past buffer release, never read freed or unmapped data, and preserve exact pixels outside the dirty region. Define and test invalid, empty, negative, overflowed, fragmented and out-of-bounds rectangles; cap complexity and fall back to full damage.
3. For stacking, movement, destruction, occlusion, opacity and popup transitions, damage both old and new visible extents. Recompose the affected scene region from all intersecting surfaces in stacking order for correct alpha blending. Dirty cursor old and new footprints, including cursor removal.
4. Maintain persistent output composition storage and persistent per-scanout-buffer generation and valid-region state. A back buffer may lag several generations: repair the union of missed changes. Unknown age, resize, modeset, failed or aborted flip, suspend/resume or reset requires conservative invalidation. Full refresh must reestablish valid generations so sparse frames recover.
5. Keep pending_present, flip-in-flight and frame-callback semantics correct. A callback-only commit progresses without unnecessary pixel copies. A commit during an in-flight flip must not be lost or acknowledged early. Complete callbacks only for the appropriate confirmed presentation.
6. Preserve synchronized subsurfaces, opaque regions, transforms, popup positioning, pointer focus and damage clipping. Respect explicit regions when safe; do not infer opaque pixels without proof.

### Baseline

The remote path tracks Damage and DamageBuffer, patches damaged rows of retained surface pixels, recomposes damaged output rectangles into a persistent framebuffer, and sends Damage messages paced to 30 frames a second ([session contract](../tinyland/docs/session.md#remote-session)). A differential test checks every presented frame of random damage, move and resize sequences against independent full composition. In bctl's headed benchmark on 2026-11-18, idle, typing, scrolling and a small animation matched a Sway and wayvnc reference in CPU, and the remote path sent fewer bytes. TINY-19 records that this historical comparison lacks a reproducible pinned baseline and retained samples here.

The DRM path recomposes output damage into the persistent framebuffer and repairs missed damage separately for each scanout buffer. Initial and invalidated buffers receive a full copy. A deterministic differential scanout test exercises delayed and failed flips, and a 1280x800 fixture checks two 16x16 changes across alternating buffers. bctl's SmolVM virtio-gpu double-buffered scanout ran idle, typing, scrolling, animation, video and dashboard pages with content checks on 2026-11-18. Exact-pixel comparison through a real device, physical DRM devices, VT and downstream qualification remain open.

The original full-frame path discarded surface damage, copied whole SHM buffers, cleared and composed full outputs, and copied full frames into DRM dumb buffers. Event-driven pending_present avoided an unconditional render loop but did not make rendering incremental.

### Acceptance

Maintain the protocol and model slice's typed bounded region operations, pending and committed damage, buffer-space conversion, surface-tree extents and generation accounting. Preserve reusable owned surface snapshots, row-range copies and persistent partial composition; use alpha or opaque fast paths only after equivalence tests. Record per-frame bytes read, copied, cleared, blended and allocated. For DRM track buffer generation, copied bytes, flip submission and callback timestamps.

Use an independent full-render oracle to compare exact output pixels and callback/event ordering. Begin with a 16x16 update on a 1280x800 output and two alternating buffers, then use deterministic randomized sequences and injected lost or late flips, allocation failures and invalidation. Verify that deliberately broken accumulation fails the harness.

Required fixtures:

- Initial full frame and one small SHM rectangle; two or three alternating buffers, multiple commits before a flip, and slow or skipped frames.
- Cursor move, shape and hide; popup move, resize and close; toplevel swap, overlap, transparency, occlusion, expose and destruction.
- Synchronized and desynchronized subsurfaces, buffer attach, replace and release, and supported scale, transform and viewport behavior.
- Damage and DamageBuffer coordinate conversions, clipped and fragmented rectangles, zero-size and overflow attempts.
- Flip timeout, page-flip failure, modeset, VT loss and restore, reconnect and output resize; full-refresh recovery to sparse operation.
- Callback-only commits and no-op frames without spurious full copies or early callbacks.

Capture reproducible before/after fixtures, commands, host and guest versions, screenshots or hashes, exact pixel diffs, frame latencies, compositor CPU, allocations, memory, copied, cleared and blended bytes, and fallback counts. Distinguish compositor, DRM, host adapter and viewer stages; VNC payload alone is not proof. Proposed quiet-host sparse-fixture targets are at least 80% less compositor CPU, substantially fewer bytes touched, and no more than 5% sustained fullscreen regression; these are goals, not results. Exact pixels and callback ordering are hard gates, along with bounded memory and no new stalls. Run existing unit, integration and device gates. Do not close TINY-14 until a real double-buffered path and downstream BCTL-22 integration are demonstrated.

### Needs environment

Real-device, physical DRM, VT and downstream qualification remain pending. Label unavailable physical DRM or macOS SmolVM tests as pending. Performance measurement requires a quiet repeatable host and the gates in the sequence.

### Execution boundary and non-goals

Tinyland owns Wayland protocol semantics, scene damage and guest scanout. bctl owns libkrun/SmolVM host display, immutable publication and VNC. Freeze a small documented region/generation contract before integration; one integration owner updates the pinned Tinyland revision only after upstream tests pass. No EGL, GBM, Vulkan, GPU passthrough or speculative GPU migration is required by this campaign.

### Reference

See [TINY-14 and TINY-19](../TODO.md) for open qualification and performance evidence gaps, and [compositor verification](../tinyland/docs/verification.md) for device and guest procedures.

## Later

- Retire seat bindings while preserving live input accounting (TINY-11, TINY-21).
- Prevent direct DRM reads from blocking past flip deadlines (TINY-12).
- Admit the maximum legal device path frame (TINY-13).
- Provide and qualify a remote-only build (TINY-15).
- Diagnose and fix the intermittent crash-reclaim admission test (TINY-16).
- Prevent accept-failure spin and restore admission (TINY-17).
- Bound aggregate decoded remote damage pixels (TINY-18).
- Measure client density, seatd responsiveness and startup (TINY-20).
