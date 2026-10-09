# ADR-0006: Following an external clock

Status: Accepted (clock-following session). Implements the "all feed a PLL"
part of ADR-0001; the engine side builds on ADR-0003's event queue.

## Context

Every external tempo source (Ableton Link, Pro DJ Link, Opus Quad, MIDI
clock, the browser bridge, tap) is reduced to `sync::Observation`s: "at
sample *s* of our audio clock the source was at this bar or beat phase, at
about this tempo". The sources differ by orders of magnitude in quality:

| Source | `Precision` | What arrives |
|---|---|---|
| Ableton Link, CDJ-3000 precise position | `Exact` | frequent, sub-millisecond |
| Pro DJ Link beat packets, browser bridge | `Fine` | about one per beat, a few ms of network jitter |
| Opus Quad | `Coarse` | tempo reliable, phase only good to about ±200 ms |
| MIDI clock | `Jittery` | 24 per beat, each off by a millisecond or more |

The scheduler commits triggers about 100 ms ahead (ADR-0001), so the
timeline it reads must not move under those triggers: a jump means a flush
and a realign, which is audible. The session-2 placeholder snapped tempo and
phase on every observation, so every network hiccup became a jump and noisy
sources produced a stream of flushes. The follower runs on the control
thread, but its output decides where hits land in golden-tested renders, so
it must be deterministic across platforms (ADR-0002's spirit) and must not
allocate. Internal-clock playback must not change at all.

## Decision

### 1. Two stages: an estimate of the source, and our timeline

`sync::FollowerClock` keeps two things apart.

**The estimator** models the source's timeline on our sample clock as an
alpha-beta tracker (a second-order loop: proportional and integral):

- The reported tempo is **fed forward**: `rate = bpm / (60 · sr) · (1 +
  drift)`. Between two reports the estimate is carried forward with the
  mean of the old and new rate, which is exact for a linear pitch-fader
  ramp.
- Each phase residual `r` (wrapped into ±half a bar or beat) moves the
  estimated phase by `α·r` and the learned tempo correction `drift` by
  `κ·β·r/Δ` (`Δ` = report spacing in beats). `drift` only absorbs clock
  skew and tempo quantisation, so it is small, slow and clamped.
- Gains follow a fading memory with time constant `τ` beats:
  `fade = Δ/(Δ+τ)` (a rational stand-in for `1 − e^(−Δ/τ)`: no
  transcendental, well-behaved for any spacing), `α = max(fade, 1/(n+1))`
  (a plain running mean for the first `n` reports after a snap, then the
  fade), `β = (1 − √(1 − fade))²` (critical damping) scaled by `κ ≤ 1`.
- `Δ` for the gains is the running mean of *previous* spacings, not this
  report's own spacing. A late-stamped report has a longer spacing; using
  it would weight late reports more and bias the estimate late by about
  `rate · jitter² / spacing` (27 ms at Opus-Quad noise levels in
  simulation, gone with the running mean).
- Sources that report no tempo: right after a snap the tempo is taken from
  the phase advance since the snap, and then the integral path refines
  `bpm` itself (not `drift`; two integrators would split one error).

**The output timeline** is what `ClockSource` exposes and the scheduler
reads. It is piecewise linear and continuous: at every observation and
every `advance(now)` it is re-anchored at `now` (never in the past), runs at
the estimated rate, and closes the gap `g` to the estimate with a bounded
slew: `c = clamp((g − deadband)/τ_slew, ±max_slew)`, applied only until the
gap is closed, after which the timeline runs at exactly the estimated
tempo. A slew is therefore self-terminating: it cannot overshoot if ticks
stop arriving. The estimator never sees the output, so the cascade is
stable whatever the tuning.

`tempo_bpm()` reports the source's tempo (feed-forward, or estimated for
tempo-less sources): what a display should show. The timeline's slope
additionally contains `drift` and any slew in progress;
`samples_per_beat()` gives its long-run value.

### 2. Tuning per precision

| | Exact | Fine | Coarse | Jittery |
|---|---|---|---|---|
| reports averaged before first lock | 1 | 1 | 8 | 1 |
| phase `τ` (beats) | 0.1 | 0.5 | 32 | 1 |
| integral `κ` (× critical) | 1 | 0.05 | 0.02 | 0.5 |
| `drift` clamp | ±1 % | ±1 % | ±0.2 % | ±2 % |
| slew `τ` (beats) | 0.1 | 0.5 | 4 | 0.5 |
| slew clamp | ±5 % | ±4 % | ±1 % | ±2 % |
| output deadband | 0 | 0.5 ms | 10 ms | 0.25 ms |
| jump threshold | 20 ms | 50 ms | 300 ms | 40 ms |
| jump hold (beats and reports) | 0.25, 2 | 1.5, 2 | 4, 4 | 1, 12 |

Reasoning: `Exact` trusts every report and corrects within a beat.
`Fine` averages a few beats of network jitter but must still absorb a DJ's
jog nudge within about two beats; its integral is weak because at one
report per beat even 40 ppm of skew leaves only ~0.015 ms of static error
without it, while a strong integral rings for seconds after a nudge.
`Coarse` trusts the tempo almost completely, averages phase over tens of
beats behind a wide deadband, and slews at most 1 %. `Jittery` (MIDI)
averages many reports per beat; its integral is the strongest of the noisy
ones because MIDI's own tempo estimate lags a pitch-fader move. Jump
thresholds sit well above each source's noise (thresholds are capped at
0.45 of the phase modulus).

Measured in `core/sync/src/follower.rs` tests (seeded jitter, 124 BPM,
40 ppm skew, four seeds each):

- `Exact`, ±0.1 ms: < 0.5 ms always; a 10 ms nudge is back under 1 ms
  within one beat.
- `Fine`, ±3 ms, one report per beat: < 3 ms worst, < 1.5 ms rms; a 30 ms
  nudge is under 5 ms within 2.5 beats; +6 BPM over 4 s tracked within
  4 ms.
- `Coarse`, ±200 ms: < 100 ms from the first lock, < 50 ms after a minute,
  ≈ 10 ms rms; never a jump after lock; slew never beyond 1 %.
- `Jittery`, 24 ppqn ±1 ms: < 1 ms worst, < 0.5 ms rms; ramps within
  1.5 ms.

### 3. Snap policy

The output jumps only on a snap, and every snap is reported once by
`take_discontinuity()`; slews never are.

1. **First lock** (after `new` or `reset`): the first `acquire` phase
   reports are averaged, then the timeline snaps to the average.
2. **`request_resync()`** (quantized re-sync from the UI; MIDI Start and
   Continue): the next phase report is snapped to directly.
3. **A jump of the source** (the DJ cued or jumped): a residual beyond the
   jump threshold is an outlier and is ignored by the estimate. If outliers
   with mutually consistent offsets persist for the hold time (beats *and*
   reports), the timeline snaps to their mean. A single late packet never
   snaps.
4. **Re-acquisition** after lock loss, or after a precision change: the
   next phase report snaps only if it is beyond the jump threshold;
   otherwise tracking simply resumes.

`Phase::Bar(p)` aligns our beat modulo 4 to `p` (bars line up);
`Phase::Beat(p)` aligns modulo 1 (our bar count is kept); `TempoOnly`
changes tempo only. A snap moves our beat by the wrapped error, so the
absolute beat count stays near where it was.

### 4. Delivery and lock

Observations may be late (`sample < now`; used at their own sample) or in
the near future. Duplicates and anything older than the newest report
already used are ignored; reports more than `LOCK_TIMEOUT_S` (2 s) from
`now` are stale. If our own sample clock went backwards by more than that,
history is dropped and the follower re-acquires. With no report for 2 s,
`is_locked()` turns false and the timeline free-runs at the estimated
tempo without slewing.

### 5. Engine integration (`engine::Control`)

- `observe` keeps a copy of the follower from before the report. When the
  report snaps, it pushes a `Flush` at `now` and restarts the scheduler at
  the first step of the new timeline at or after `now − 20 ms`, but never
  at a step the old timeline already played, and never within half a step
  after the last step heard (a forward jump of a whole number of steps
  renumbers that same musical step). So nothing plays twice, and a step a
  forward jump left just behind (the downbeat after a MIDI Start) plays
  20 ms late at most rather than not at all.
- `start` in follow mode joins the source's timeline at the next step, in
  bar phase. Internal → follow starts a fresh follower on the current beat
  (continuous; the first report snaps like any first lock); switching
  between two follow precisions keeps the follower and its lock; follow →
  internal continues at the followed tempo and beat.
- MIDI Start and Continue request a re-sync. Tap in follow mode is an
  observation like any other.
- Internal mode is untouched; the golden masters are bit-identical.

### 6. MIDI clock and tap

`MidiClockFollower` fits pulse time against pulse index by least squares
over 4 beats (one slope, one intercept per run between Start/Continue),
extrapolates the period while the tempo is moving, leaves unexplainable
timestamps out of the fit, and counts phase from the first pulse after
Start. Details and sources: `docs/protocols/midi-clock.md`.
`TapTempo` uses the median interval (one bad tap changes nothing), drops
bounces, restarts on two consistent outlier intervals (a new tempo) or a
2 s pause, and reports the least-squares beat position of all taps rather
than the last raw tap.

### 7. Determinism

Only `+ − × ÷`, `sqrt` and `rem_euclid` (all correctly rounded in IEEE
754), no transcendental functions, no allocation, no clock reads. Every
test drives the follower with seeded xorshift noise.

## Consequences

- Jitter, skew and tempo ramps are absorbed without a single flush; only
  first lock, explicit re-syncs and real jumps realign the scheduler.
  `core/engine/tests/follow.rs` runs a minute of audio per scenario through
  `Control` and `Renderer` and checks that every step is heard exactly once
  and on the source's grid (Fine within 4 ms, MIDI within 2 ms, Coarse
  within 120 ms at first and 50 ms later), and that a cue jump costs
  exactly one realign.
- A `Coarse` source takes 8 reports (about 4 s) to lock and tens of beats to
  follow a nudge below its 300 ms jump threshold. That is the price of a
  phase that is only good to ±200 ms; a source adapter that knows its error
  is one-sided must remove the bias itself, because averaging cannot.
- `tempo_bpm()` and the timeline's slope differ by the learned skew
  (typically tens of ppm). Code that needs the actual mapping must use
  `beat_at_sample` / `sample_at_beat` (or `samples_per_beat`), never
  `tempo_bpm`.
- A slew changes our tempo by up to the precision's clamp for a fraction
  of a beat to a few beats; triggers already queued (≤ 100 ms ahead) keep
  their old stamps, an error of at most clamp × lookahead (≤ 5 ms for
  `Exact`, 1 ms for `Coarse`).
- New diagnostics: `FollowerClock::phase_error()` (last measured error, in
  beats). Not exposed through the FFI yet.
- Song Position Pointer is not handled (`MidiMessage` has no variant for
  it); adding it is an ABI-visible change for a later session.
