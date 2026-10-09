# Lookahead scheduling ("A Tale of Two Clocks")

Source: Chris Wilson, *A tale of two clocks – Scheduling web audio with
precision* (2013), https://web.dev/articles/audio-scheduling. Read from the
article's source in the web.dev repository
(`src/site/content/en/blog/audio-scheduling/index.md` on `main` of
https://github.com/GoogleChrome/web.dev, fetched through
`raw.githubusercontent.com` on 2026-10-09, because web.dev itself was not
reachable from the build container). Section names below are the article's
own headings. Written for Web Audio, but the pattern is
platform-independent; it is how the `sequencer` crate works on every target.

## The problem

Two clocks exist. The audio clock (`AudioContext.currentTime`) is the
audio subsystem's hardware clock, precise enough to align individual
samples. [source: §"The Best of Times - the Web Audio Clock"] The
JavaScript timers (`setTimeout`, `setInterval`) can be skewed by tens of
milliseconds or more by layout, rendering, garbage collection and other
work on the main thread. [source: §"The Worst of Times - the JavaScript
Clock"] Starting sounds directly from a timer callback therefore inherits
that jitter. [source: §"Using JavaScript setTimeout() in Audio Apps"]
Scheduling everything far ahead instead makes tempo changes and stopping
impossible, so one should not look ahead too far.
[source: §"The Best of Times - the Web Audio Clock"]

## The pattern

1. A timer fires every so often and, on each call, schedules every note
   whose time falls before `currentTime + scheduleAheadTime`, at that
   note's exact audio-clock time. [source: §"Obtaining Rock-Solid Timing By
   Looking Ahead", the `scheduler()` loop]
2. A "next note time" cursor advances as notes are emitted, so each note is
   scheduled once; the cursor picks up the current tempo at every step.
   [source: same section, `nextNote()`]
3. The lookahead overlaps the next timer call so that a delayed call still
   finds its notes already scheduled; the demo uses a 25 ms interval and a
   100 ms lookahead and survives a callback that arrives 50 ms late.
   [source: same section, the timing diagrams]
4. The overall lookahead sets how quickly tempo and other live changes take
   effect; the interval trades latency against CPU. To be resilient on slow
   machines, use a large lookahead and a reasonably short interval; "a good
   place to start is probably 100ms of 'lookahead' time, with intervals set
   to 25ms", raising the lookahead for complex apps or lowering it for
   tighter control at the cost of resilience. [source: same section]

## How player5 applies it

- `Control::schedule_ahead(now)` is the tick body; `now` is the renderer's
  sample position and the horizon is `now + lookahead` (default 100 ms:
  4 800 samples at 48 kHz, scaled with sample rate;
  `engine::DEFAULT_LOOKAHEAD_SAMPLES`).
- `Scheduler` keeps the "next step" cursor and emits `Event::Trigger` with
  the exact sample stamp `round(clock.sample_at_beat(step_beat))`. Shuffle
  is applied in beats before the conversion.
- The renderer fires each event at its sample offset inside the block, so
  block size never affects timing (tested: identical PCM at 97, 256 and
  4 096-sample blocks, `core/render/tests/golden.rs`).
- Live pattern edits take effect from the next unscheduled step, i.e. after
  at most the lookahead window. That is the accepted trade-off.
- Where control and render run on separate threads (the `core/ffi` split
  API used by the Apple shells), the article's timer becomes a 5 ms control
  timer and events cross to the render thread through the SPSC queue
  (ADR-0001, ADR-0008). In the browser no timer is involved: the whole
  engine runs inside the AudioWorklet and the control half steps in
  lockstep with the render half off the worklet's own sample clock
  (ADR-0004); the main thread only posts pattern and transport changes
  through the port.
