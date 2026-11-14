# TINY-14: Incremental Wayland damage and scanout qualification

Status: planned, P2, independent upstream campaign. This document is an execution contract, not evidence that optimizations are implemented. Downstream integration: joshuarli/bctl BCTL-22. Keep existing TINY-11–13 work independent.

## Why
Current wl_surface Damage and DamageBuffer requests are discarded. SHM commits snapshot entire buffers; composition allocates/clears an entire output; presentation copies full output into DRM dumb buffers; cursor and popup changes trigger whole-output work. Event-driven pending_present prevents an unconditional render loop but is not incremental rendering. Fix at the source, without changing the deliberately minimal software-only/DRM/seatd architecture or requiring EGL, GBM, Vulkan, or GPU passthrough.

## Ownership and invariants
1. Track pending surface-local damage (Damage and DamageBuffer with correct coordinate semantics, scaling, transforms and clipping), committed state, synchronized subsurface state, and output-space accumulated damage separately. A commit is the transactional boundary; multiple commits before a flip must union, never replace, dirty regions. New buffers, size/scale/transform changes, first presentation, invalidated contents and unknown age conservatively mark full extents.
2. Preserve surface and SHM buffer lifetime: snapshot only safely accessible dirty pixels into owned persistent storage, never retain borrowed mappings past buffer release, never read freed/unmapped data, and preserve exact pixels outside the dirty region. Define and test invalid, empty, negative, overflowed, fragmented and out-of-bounds rectangles; cap complexity and fall back to full damage.
3. For stacking, movement, destruction, occlusion, opacity and popup transitions, damage both old and new visible extents. Correct alpha blending requires recompositing the affected scene region from all intersecting surfaces in stacking order; do not just blit the top changed surface. Cursor old/new footprints must be dirtied, including cursor removal.
4. Maintain persistent output composition storage and persistent per-scanout-buffer generation/valid-region state. A scanout back buffer may lag one or more generations: repair the union of missed changes, not merely the newest rectangle. Unknown age, resize, modeset, failed/aborted flip, suspend/resume or reset requires explicit conservative invalidation. Full refresh must reestablish valid generations so subsequent sparse frames recover; never remain permanently full-dirty.
5. Keep pending_present, flip-in-flight and frame-callback semantics correct. A callback-only commit progresses without unnecessary pixel copies. A commit arriving during an in-flight flip must not be lost or acknowledged early. Complete callbacks only for the appropriate confirmed presentation.
6. No protocol regressions: preserve synchronized subsurfaces, opaque regions, transforms, popup positioning, pointer focus and damage clipping. Respect explicit regions when safe; do not infer opaque pixels without proof.

## Parallel slices
**A. Protocol and model (one owner):** introduce small typed rectangles/regions, bounded union/intersection/translation, pending/committed damage, buffer-space conversion, surface-tree old/new extents and generation accounting. Unit and property tests precede integration. Document exactly which state owns each dirty region.

**B. SHM and composition (second owner, coordinated on region API):** reusable owned surface snapshots, row-range partial copy, persistent composition framebuffer, dirty-region clear/recompose from all intersecting surfaces, alpha/opaque fast paths only after equivalence tests. Record bytes read, copied, cleared, blended and allocated per frame.

**C. DRM presentation (third owner, coordinated on generation API):** dirty-region copy into each scanout buffer with accumulated missed regions; explicit initial/full refresh and fallback; nonblocking event and flip completion behavior preserved. Track buffer generation, copied bytes, flip submission and callback timestamps.

**D. Differential and fault qualification (independent owner):** build a full-render reference oracle independent of optimized dirty-region logic; compare exact output pixels and callback/event ordering. Start with one 16x16 update on a 1280x800 output and two alternating buffers, then add randomized deterministic sequences and injected lost/late flips, allocation failures and invalidation. Verify that intentionally broken accumulation fails the harness.

## Required fixtures
- Initial full frame then one small SHM rectangle; two/three alternating scanout buffers, several commits before flip, slow or skipped frames.
- Cursor move/shape/hide; popup move/resize/close; toplevel swap, overlap, transparency, occlusion, expose and destruction.
- Synchronized/desynchronized subsurfaces, buffer attach/replace/release, scale/transform/viewport behavior where supported.
- Damage and DamageBuffer coordinate conversions, clipped and fragmented rectangles, zero-size and overflow attempts.
- Flip timeout, page-flip failure, modeset, VT loss/restore, reconnect and output resize; full-refresh recovery to sparse operation.
- Callback-only commits and no-op frames; ensure no spurious full copy or incorrect early callback.

## Evidence and gates
Capture reproducible before/after fixtures, command lines, host and guest versions, screenshots or hashes, exact pixel diffs, frame latencies, compositor CPU, allocations, memory, copied/cleared/blended bytes and fallback counts. Distinguish compositor, DRM, host adapter and viewer stages; VNC payload alone is not proof. Proposed performance targets on a quiet repeatable sparse fixture: >=80% less compositor CPU, substantial proportional reduction in bytes touched; no >5% sustained fullscreen regression. These are goals, not results. Exact pixels and callback ordering are hard gates, with bounded memory and no new stalls. Run existing unit/integration/device gates; explicitly label unavailable physical DRM or macOS SmolVM tests as pending. Do not close TINY-14 until a real double-buffered path is qualified and downstream BCTL-22 integration is demonstrated.

## Coordination
Tinyland owns Wayland protocol semantics, scene damage and guest scanout. bctl owns libkrun/SmolVM host display, immutable publication and VNC. Freeze a small documented region/generation contract before integration; one integration owner updates the pinned tinyland revision only after upstream tests pass. No speculative GPU migration in this campaign.
