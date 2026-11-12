# TINY-1: Presentation ownership

Status: draft

## Summary

Separate scene changes waiting for submission from the frame already queued
to DRM. Each submitted frame owns its completion callbacks. A flip completion
must not clear newer work or acknowledge a commit absent from that frame.

## Motivation

The runtime currently uses one `pending_present` bit across commit, submission,
and completion. `State::presented` selects callbacks from the current scene.
The [ticket](../../TODO.md#tiny-1--p2--flip-completion-drops-newer-redraws-and-completes-their-callbacks-early)
records the A-submit/B-commit/A-complete sequence that loses B. The headless
driver completes a frame in one pump and does not model this interval.

## Design

Keep the existing two scanout buffers and single outstanding DRM flip. The
compositor owns dirty scene state and an optional submitted-frame record;
`DrmOutput` continues to own kernel resources and the flip deadline.

When no flip is pending, capture the visible committed scene and its eligible
callback batch, compose it, and submit it. Consume only the dirty state covered
by that submission. Commits, cursor changes, and surface retirement afterward
set dirty state independently. A failed submission produces no callback done
events.

On the matching kernel event, complete only the submitted batch, skipping
callback resources that have already been destroyed. Drop the submitted
record, then submit the latest scene if it became dirty. Do not re-enumerate
the current visible scene to decide which old callbacks completed. Preserve
pending/cached surface transaction rules when selecting callbacks.

Represent submission and completion as distinct operations usable by the
existing headless fixture. Do not add a general renderer framework, an async
runtime, or another framebuffer copy. A narrow presentation record and explicit
transitions are sufficient.

On suspend, failed flips must not acknowledge callbacks. Retain still-live
uncompleted callbacks for a later eligible presentation and retire the
submitted record through the existing output/device teardown. Release the old
buffer ownership before reuse and dirty the scene on resume. This design does
not assume a kernel flip-cancellation API. Disconnect and surface destruction
must retire their resources without requiring a future flip.

Acceptance includes A/B interleaving, a window close during A, failed submission,
timeout, and suspend before completion. Assert both which callbacks are sent
and which scene remains pending. Keep device qualification separate from these
controlled transition checks.

## Alternatives considered

- Clearing the bit at submission fixes lost redraws but leaves callback
  completion attached to the wrong scene.
- Blocking client dispatch until the flip completes avoids the interval by
  delaying input and broker handling, defeating the responsive poll loop.
- Queueing every client commit adds unbounded work. Coalesce unsent updates
  into the next frame while retaining their valid callback obligations.

## Open questions

No product decision is required to restore the existing presentation contract.
Use the smallest internal representation that records submitted ownership;
a frame generation is useful only if needed to select the callback batch.

## Decision

Draft recommendation. Implementation and device qualification remain deferred
to TINY-1.
