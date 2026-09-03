# ADR-0003: Parameter changes travel through the event queue

Status: Accepted (session 1)

## Context

Voice controls (Tune, Decay, Level…) and master settings change from the UI
while audio runs. The obvious shortcut, atomics or a shared struct the
render thread reads each block, means the render thread observes changes at
block boundaries with no relation to musical time, and adds a second
communication path to reason about.

## Decision

A parameter change is an `Event` like a trigger: `EventKind::Param { target,
value }` stamped with a sample position, pushed by the control thread into
the same SPSC queue the scheduler uses. The renderer applies it at that
sample, in order with triggers.

Because parameter events are stamped "now" while triggers are scheduled up
to 100 ms ahead, the queue is not strictly time-ordered. The renderer drains
the queue into a small fixed-capacity pending list at the start of each block
and keeps it sorted by sample (insertion from the back; the list is short
and almost sorted). Late events fire immediately rather than being dropped.

## Consequences

- One path from control to audio, one set of invariants, no shared mutable
  state between threads.
- Parameter changes can be scheduled musically in the future (automation,
  quantized changes) for free.
- The pending list has a fixed capacity (256). If it fills, further queue
  events wait for the next block; a burst of that size is a bug elsewhere.
- Lossy delivery under a full queue: `Control::set_*` return `false` and the
  caller may retry. The UI layer owns that policy.
