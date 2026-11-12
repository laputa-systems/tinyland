# TINY-6: Input ownership across device loss

Status: draft

## Summary

Retain the originating device for every delivered key and button. Derive the
logical seat's pressed state from those owners, and reconcile it before a
device disappears or the seat suspends.

## Motivation

The runtime merges input events and discards device identity. Removing an
input resets KBVM but cannot release that device's delivered state. Pointer
buttons remain in the implicit-grab state. See the
[ticket](../../TODO.md#tiny-6--p2--device-loss-and-suspension-leave-delivered-keys-and-pointer-grabs-pressed)
for the concrete failure sequence.

## Design

Give each opened input a stable identity for its lifetime. Preserve it while
sorting events by timestamp; vector positions are unsuitable because removal
changes them. Track the keys and buttons owned by each device, with aggregate
seat state derived from those records.

Emit a logical press when the first device acquires a code and a release when
its last owner releases it. Drive KBVM and Wayland delivery from those same
logical transitions. Keep keyboard keys and pointer buttons distinct. An
unplug removes only that device's ownership, so another keyboard holding the
same key remains pressed.

Discard already-collected events from retired device identities before further
dispatch. Cleanup must not emit releases and then replay an old queued press.

Before suspend drops descriptors, release all delivered ownership, publish
the resulting modifiers, and terminate implicit pointer grabs. Resume starts
with newly opened device identities and an explicit logical state. Reconcile
any recovered physical state through the same transition path. Do not invent
release events from a hardcoded list of modifier and function-key codes.

Keep the existing lock-reset policy for this repair. Preserving Caps Lock or
other locked modifiers across VT changes is a separate user-visible choice.
Focus entry should report the logical held-key set from this owner, and the
existing focused-client delivery rules must remain intact.

Apply the same cleanup through actual unplug, broker disable, and session
shutdown. A disable received during ENODEV recovery must run the suspend
transition before classifying a device as permanently lost, as required by
TINY-4. Broker cancellation and deadlines remain TINY-5's responsibility.

Acceptance covers a held repeating key, a held pointer button, external VT
switching followed by release while suspended, two devices owning the same
key, and queued input when disable arrives. Also cover a key or button still
physically held when devices reopen. Verify releases, modifiers, refocusing,
and later input, including removal of an unrelated device. Use controlled
input/broker events first and qualify the device path separately.

## Alternatives considered

- Resetting only KBVM cannot stop client repeat or clear a pointer grab.
- Releasing every code on any unplug breaks presses owned by other devices.
- Keeping only one global pressed set cannot distinguish those owners.

## Open questions

Lock persistence across VT changes may be considered later. The recommendation
is to retain the current reset behavior for this fix, so no owner decision
blocks implementation of delivered-input cleanup.

## Decision

Draft recommendation. Implementation and device qualification remain deferred
to TINY-6, coordinated with the suspend-order repair in TINY-4.
