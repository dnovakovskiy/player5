# ADR-0011: Two kinds of realign, whole flams, flushes that always land

Status: Accepted (amends ADR-0006 §5)

## Context

ADR-0006 §5 defined how `engine::Control` realigns the scheduler when a
follower snaps: flush at the *commit point*, keep what was heard, never
land within half a step of the last step heard. A final end-to-end review
(`core/engine/tests/timing.rs`: minutes of audio in lockstep and split
mode under jitter, ramps, stalls, cue jumps, transport storms, re-syncs,
taps and clock switches) found the rest of the timeline changes did not
follow the same rules:

1. The internal clock's `start` and `resync` restarted the bar at `now`.
   On the split path `now` is the renderer's stale published block start,
   so the downbeat played a block late and steps the renderer had already
   committed to were flushed "from now" (too early to take them back) and
   doubled. A `start` while playing (or within the lookahead of a stop)
   left the old run's queued steps to play over the new run.
2. An internal tap moved the grid without the no-double rule: a tap just
   behind the beat replayed the step just heard.
3. A step's flam grace note sits before its grid hit. A commit point
   between the two flushed the hit, the realign replayed the step, and the
   grace note sounded twice.
4. A flush push was not checked; parameter changes could fill the queue.
5. `Scheduler::set_stop_after` counted absolute steps, so a one-shot that
   joined a following timeline at step 53 with a one-bar limit stopped at
   once.

## Decision

- **Every timeline change acts at the commit point** (`now` in lockstep,
  two learned render blocks after the published position when split), in
  both clock modes.
- **Two kinds of realign.**
  - A *restart* (internal `start`, internal `resync`) puts step 0 exactly
    on the commit point, after latency compensation and nudge, so the
    downbeat sounds when it was asked for. Everything queued with its grid
    hit at or after the commit point is flushed, even when that lands the
    downbeat close after a step just heard: the user asked for the bar to
    begin now. A flam whose grace note was already heard loses its hit; the
    grace note becomes a pickup into the new downbeat.
  - A *continuation* (follower snap, internal tap, `start` while
    following) keeps everything heard and continues on the new timeline
    no sooner than half a step after the last step heard (ADR-0006 §5).
- **Flams are whole.** The queued-step log records each step's earliest
  event. A continuation treats a step whose grace note is before the
  commit point as heard in full and moves the flush past its hit.
- **Flushes always land.** The scheduler and parameter changes leave
  `sequencer::FLUSH_RESERVE` (2) queue slots free. A flush that still does
  not fit (only possible right after another flush, which already covered
  it) is retried at the next tick before anything else is scheduled.
- **Stop-after counts from the run's first step** (`start` / `start_at`);
  realigns keep the number of steps still to play (`Scheduler::seek`); set
  while playing it counts from the next step.
- MIDI Song Position Pointer stays unsupported (no `MidiMessage` variant;
  an ABI change for a later session). After SPP + Continue the bar phase
  continues from our pulse count; the realign Continue triggers still never
  doubles or drops a step.

## Consequences

- Lockstep (the browser, offline renders) is unchanged wherever the old
  rules already held; the golden masters are bit-identical.
- On the split path a start or re-sync sounds two render blocks after the
  call (about 21 ms at 512 frames), on time, instead of one block late.
- `core/engine/tests/timing.rs` checks, per run: every source step heard
  exactly once (outside stops, cue jumps and hand-backs), every hit on the
  source's grid (Exact 3 ms, MIDI 4 ms, Fine 6 ms; a step a realign left
  just behind plays at most 20 ms late), bars aligned, every flam whole,
  restarts exactly on `commit + k × step`, stop-after exact.
  `core/ffi/src/split.rs` checks that the split API is bit-identical to the
  lockstep engine started and re-synced at the commit point.
- The commit point relies on the smallest published block step. A host
  whose callback size varies (e.g. 471/512 frames) can put it up to twice
  the difference (here 82 samples) too early, so a step queued in that
  sliver could be both heard and replayed; publishing the block length
  with the position would make it exact.
