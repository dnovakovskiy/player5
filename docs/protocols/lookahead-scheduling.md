# Lookahead scheduling ("A Tale of Two Clocks")

Source: Chris Wilson, *A Tale of Two Clocks – Scheduling Web Audio with
Precision* (2013), https://web.dev/articles/audio-scheduling. Written for
Web Audio but the pattern is platform-independent; it is how the
`sequencer` crate works on every target.

## The problem

Two clocks exist: the UI/control thread's wall clock (coarse, jittery, may
stall) and the audio hardware's sample clock (exact, drives the callback).
Scheduling a hit "when the timer fires" inherits the timer's jitter, and
the audio callback cannot make decisions that depend on the UI.
[source: §"The Problem" and §"Two Clocks"]

## The pattern

1. The control thread runs a periodic tick (the article uses ~25 ms).
2. On each tick it looks *ahead* on the audio clock by a lookahead window
   (the article uses 100 ms) and schedules every note whose time falls
   inside that window, with its exact audio-clock timestamp.
3. Notes are scheduled at most once; the tick keeps a "next note time"
   cursor that advances as notes are emitted.
4. Because the lookahead is longer than the tick interval, a delayed tick
   still has notes in flight; audio never starves. Because notes carry an
   exact timestamp, timer jitter does not reach the audio.
[source: §"The Lookahead Scheduler" and the accompanying metronome demo]

Trade-off: a longer lookahead tolerates more control-thread stall but adds
latency to live changes (a step toggled inside the window has already been
scheduled). The article recommends the shortest lookahead that survives the
platform's worst timer stall. [source: §"Some Tips for Your Own Scheduler"]

## How player5 applies it

- `Control::schedule_ahead(now)` is the tick body; `now` is the renderer's
  sample position and the horizon is `now + lookahead` (default 100 ms:
  4 800 samples at 48 kHz, scaled with sample rate).
- `Scheduler` keeps the "next step" cursor and emits `Event::Trigger` with
  the exact sample stamp `round(clock.sample_at_beat(step_beat))`. Shuffle
  is applied in beats before the conversion.
- The renderer fires each event at its sample offset inside the block, so
  block size never affects timing (tested: identical PCM at 97, 256 and
  4 096-sample blocks).
- Live pattern edits take effect from the next unscheduled step, i.e. after
  at most the lookahead window. That is the accepted trade-off.
- The queue is an SPSC ring; the article's `setTimeout`-driven scheduler is
  replaced on native targets by a control-thread timer, and in the browser
  by exactly the article's mechanism feeding the worklet through a port.
