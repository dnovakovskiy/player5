//! Following an external tempo source.
//!
//! Every external source (Ableton Link, Pro DJ Link, Opus Quad, MIDI clock,
//! the browser bridge, tap) is reduced to [`Observation`]s: "at this sample
//! position the source was at this phase, at about this tempo". A
//! [`FollowerClock`] turns a stream of observations into a smooth timeline
//! the scheduler can use. Design, tuning table and snap policy: ADR-0006.
//!
//! Two stages, so that noise rejection and timeline continuity can be tuned
//! separately and the whole thing stays unconditionally stable (no feedback
//! from the output back into the estimate):
//!
//! 1. **Estimator** — an alpha-beta tracker modelling the source's timeline
//!    on our sample clock. The reported tempo is fed forward; each phase
//!    residual moves the estimated phase (proportional term) and a small
//!    learned tempo correction, `drift` (integral term), which absorbs
//!    clock skew between the source and our audio device and tempo
//!    quantisation in the reports. Residuals beyond a per-precision
//!    threshold are outliers; a consistent one held long enough is a jump.
//! 2. **Output timeline** — what [`ClockSource`] exposes. It runs at the
//!    estimated tempo and closes any gap to the estimate with a bounded,
//!    self-terminating tempo deviation (a slew), re-planned from the
//!    current sample at every observation and every [`FollowerClock::advance`].
//!    It only ever jumps on a snap, and every snap is reported once by
//!    [`FollowerClock::take_discontinuity`].
//!
//! Control-thread code: no allocation, and only `+ - * /`, `sqrt` and
//! `rem_euclid`, so results are bit-identical on every platform.

use crate::ClockSource;

/// Where in the bar or beat the source was at an observation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    /// Position within a 4-beat bar, `0.0..4.0` (`0.0` = downbeat).
    Bar(f64),
    /// Position within a beat, `0.0..1.0` (`0.0` = on the beat). Bar
    /// alignment unknown.
    Beat(f64),
    /// No phase information (tempo-only sources).
    TempoOnly,
}

/// How trustworthy a source's timing is. Selects the PLL tuning.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Precision {
    /// Sub-millisecond, regular (Ableton Link, CDJ-3000 precise position).
    Exact,
    /// A few milliseconds of network jitter (Pro DJ Link beat packets, the
    /// browser bridge).
    Fine,
    /// Phase only good to about ±200 ms (Opus Quad); interpolate heavily.
    Coarse,
    /// Frequent but individually jittery (MIDI clock at 24 ppqn).
    Jittery,
}

/// One timing report from a source, already mapped onto the consumer's
/// sample clock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Observation {
    /// Sample position the report refers to (may be slightly in the past).
    pub sample: f64,
    /// Phase at that sample.
    pub phase: Phase,
    /// Tempo, if the source reports one.
    pub bpm: Option<f64>,
}

/// Seconds without an observation after which the source counts as lost
/// ([`FollowerClock::is_locked`] turns `false`; the clock free-runs).
/// Observations further than this from `now` are ignored as stale.
pub const LOCK_TIMEOUT_S: f64 = 2.0;

/// Smallest observation spacing (beats) used to scale the integral term,
/// so two reports a few samples apart cannot kick the tempo.
const MIN_UPDATE_BEATS: f64 = 1.0 / 96.0;

/// Weight of the newest spacing in the running mean of observation
/// spacings that sets the loop gains.
const SPACING_SMOOTHING: f64 = 1.0 / 16.0;

/// Loop tuning for one [`Precision`]. ADR-0006 has the table and the
/// reasoning behind each number.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Tuning {
    /// Phase observations averaged before the first lock snaps.
    acquire_obs: u32,
    /// Estimator phase time constant, in beats (larger = smoother).
    phase_tau: f64,
    /// Integral gain as a fraction of critical damping (`<= 1`).
    drift_gain: f64,
    /// Largest learned tempo correction, as a fraction of the tempo.
    max_drift: f64,
    /// Output slew time constant, in beats.
    slew_tau: f64,
    /// Largest output tempo deviation while slewing, as a fraction.
    max_slew: f64,
    /// Output deadband, in seconds: smaller gaps are left alone.
    deadband_s: f64,
    /// Residual (seconds) beyond which an observation is an outlier.
    jump_s: f64,
    /// A consistent outlier must persist this many beats...
    jump_hold_beats: f64,
    /// ...and this many observations before the timeline snaps to it.
    jump_hold_obs: u32,
}

impl Precision {
    const fn tuning(self) -> Tuning {
        match self {
            Self::Exact => Tuning {
                acquire_obs: 1,
                phase_tau: 0.25,
                drift_gain: 1.0,
                max_drift: 0.01,
                slew_tau: 0.125,
                max_slew: 0.05,
                deadband_s: 0.0,
                jump_s: 0.020,
                jump_hold_beats: 0.25,
                jump_hold_obs: 2,
            },
            Self::Fine => Tuning {
                acquire_obs: 1,
                phase_tau: 0.75,
                drift_gain: 0.1,
                max_drift: 0.01,
                slew_tau: 0.5,
                max_slew: 0.04,
                deadband_s: 0.000_5,
                jump_s: 0.050,
                jump_hold_beats: 1.5,
                jump_hold_obs: 2,
            },
            Self::Coarse => Tuning {
                acquire_obs: 8,
                phase_tau: 32.0,
                drift_gain: 0.02,
                max_drift: 0.002,
                slew_tau: 4.0,
                max_slew: 0.01,
                deadband_s: 0.010,
                jump_s: 0.300,
                jump_hold_beats: 4.0,
                jump_hold_obs: 4,
            },
            Self::Jittery => Tuning {
                acquire_obs: 1,
                phase_tau: 1.0,
                drift_gain: 0.5,
                max_drift: 0.02,
                slew_tau: 0.5,
                max_slew: 0.02,
                deadband_s: 0.000_5,
                jump_s: 0.040,
                jump_hold_beats: 1.0,
                jump_hold_obs: 12,
            },
        }
    }
}

/// Where the follower is in acquiring the source's phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Lock {
    /// Never aligned since creation or [`FollowerClock::reset`]: phase
    /// observations are averaged, then the timeline snaps to them.
    Acquire,
    /// Lost the source (or its precision changed): the next phase
    /// observation snaps only if it is beyond the jump threshold.
    Reacquire,
    /// Following: residuals steer the estimate, the output slews.
    Track,
}

/// A run of mutually consistent outliers: a candidate jump.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Jump {
    since: f64,
    count: u32,
    offset: f64,
}

/// A [`ClockSource`] steered by observations.
///
/// Call [`FollowerClock::observe`] for every report and
/// [`FollowerClock::advance`] once per control tick; both take `now`, the
/// consumer's current sample position. The timeline is continuous except
/// at snaps (first lock, [`FollowerClock::request_resync`], a sustained
/// jump of the source, [`FollowerClock::reset`]), each of which is reported
/// once by [`FollowerClock::take_discontinuity`].
#[derive(Clone, Debug)]
pub struct FollowerClock {
    sample_rate: f64,
    precision: Precision,
    tuning: Tuning,
    // Output timeline: from (`anchor_sample`, `anchor_beat`) at
    // `slew_rate` beats per sample until `slew_end`, then at `rate`.
    anchor_sample: f64,
    anchor_beat: f64,
    slew_rate: f64,
    slew_end: f64,
    rate: f64,
    // Estimate of the source's timeline: `est_beat` at `est_sample`, at
    // `bpm` (fed forward) times `1 + drift` (learned).
    bpm: f64,
    drift: f64,
    est_sample: f64,
    est_beat: f64,
    updates: u32,
    // Lock state.
    lock: Lock,
    locked: bool,
    acquired: u32,
    jump: Option<Jump>,
    discontinuity: bool,
    resync_requested: bool,
    last_observation: Option<f64>,
    last_phase_observation: Option<f64>,
    mean_spacing: Option<f64>,
    last_phase_value: f64,
    phase_error: f64,
}

impl FollowerClock {
    /// Lowest tempo accepted from a source.
    pub const MIN_BPM: f64 = 20.0;
    /// Highest tempo accepted from a source.
    pub const MAX_BPM: f64 = 400.0;

    /// An unlocked follower free-running at `initial_bpm`, beat 0 at
    /// sample 0.
    #[must_use]
    pub fn new(sample_rate: f64, initial_bpm: f64, precision: Precision) -> Self {
        let bpm = if initial_bpm.is_finite() {
            initial_bpm.clamp(Self::MIN_BPM, Self::MAX_BPM)
        } else {
            120.0
        };
        let rate = bpm / (60.0 * sample_rate);
        Self {
            sample_rate,
            precision,
            tuning: precision.tuning(),
            anchor_sample: 0.0,
            anchor_beat: 0.0,
            slew_rate: rate,
            slew_end: 0.0,
            rate,
            bpm,
            drift: 0.0,
            est_sample: 0.0,
            est_beat: 0.0,
            updates: 0,
            lock: Lock::Acquire,
            locked: false,
            acquired: 0,
            jump: None,
            discontinuity: false,
            resync_requested: false,
            last_observation: None,
            last_phase_observation: None,
            mean_spacing: None,
            last_phase_value: 0.0,
            phase_error: 0.0,
        }
    }

    /// Current tuning.
    #[must_use]
    pub fn precision(&self) -> Precision {
        self.precision
    }

    /// Changes the tuning (e.g. a source switched from beat packets to
    /// precise position packets). Keeps the timeline and the estimate; the
    /// next phase observation snaps if it disagrees by more than the new
    /// precision's jump threshold, otherwise tracking simply continues.
    pub fn set_precision(&mut self, precision: Precision) {
        if precision == self.precision {
            return;
        }
        self.precision = precision;
        self.tuning = precision.tuning();
        self.drift = self
            .drift
            .clamp(-self.tuning.max_drift, self.tuning.max_drift);
        self.jump = None;
        if self.lock == Lock::Track {
            self.lock = Lock::Reacquire;
        }
    }

    /// Whether the follower is tracking a live source: it has aligned to
    /// the source (or taken its tempo, for tempo-only sources) and the last
    /// observation is less than [`LOCK_TIMEOUT_S`] old.
    #[must_use]
    pub fn is_locked(&self) -> bool {
        self.locked
    }

    /// Samples since the last observation, if any arrived.
    #[must_use]
    pub fn observation_age(&self, now: f64) -> Option<f64> {
        self.last_observation.map(|s| now - s)
    }

    /// Our phase error at the last phase observation, in beats: how far the
    /// source was ahead of our timeline (negative = behind). Diagnostic,
    /// e.g. for a sync meter.
    #[must_use]
    pub fn phase_error(&self) -> f64 {
        self.phase_error
    }

    /// Asks for the next observation to be applied as a hard phase snap
    /// (quantized re-sync) rather than a gradual correction.
    pub fn request_resync(&mut self) {
        self.resync_requested = true;
    }

    /// Returns `true` once after the timeline jumped (phase snap), so the
    /// caller can flush and re-sync its scheduler.
    pub fn take_discontinuity(&mut self) -> bool {
        core::mem::take(&mut self.discontinuity)
    }

    /// Feeds one observation. `now` is the consumer's current sample
    /// position; observations may refer to the recent past or future, but
    /// ones more than [`LOCK_TIMEOUT_S`] away from `now`, duplicates and
    /// ones older than the newest already seen are ignored.
    pub fn observe(&mut self, obs: &Observation, now: f64) {
        let s = obs.sample;
        if !s.is_finite() || !now.is_finite() {
            return;
        }
        let timeout = LOCK_TIMEOUT_S * self.sample_rate;
        if (s - now).abs() > timeout {
            return;
        }
        if let Some(last) = self.last_observation {
            if last - s > timeout {
                // Our own sample clock went backwards (the consumer was
                // restarted): forget the old source history.
                self.last_observation = None;
                self.last_phase_observation = None;
                self.mean_spacing = None;
                self.locked = false;
                self.lock = Lock::Acquire;
                self.acquired = 0;
            } else if s <= last {
                // A duplicate or an out-of-order report: older news than
                // what we already used.
                return;
            }
        }
        let target = match obs.phase {
            Phase::Bar(p) if p.is_finite() => Some((p, 4.0)),
            Phase::Beat(p) if p.is_finite() => Some((p, 1.0)),
            Phase::Bar(_) | Phase::Beat(_) | Phase::TempoOnly => None,
        };
        let bpm = obs
            .bpm
            .filter(|b| b.is_finite() && *b > 0.0)
            .map(|b| b.clamp(Self::MIN_BPM, Self::MAX_BPM))
            .or_else(|| target.and_then(|(p, m)| self.tempo_from_phase(s, p, m)));
        if let Some((p, _)) = target {
            self.last_phase_value = p;
        }

        // Carry the estimate forward to `s`. The tempo report describes the
        // tempo at `s`; across a change, average old and new (exact for a
        // linear ramp, half-way for a step at an unknown moment).
        let old_rate = self.est_rate();
        if let Some(bpm) = bpm {
            self.bpm = bpm;
        }
        let new_rate = self.est_rate();
        self.est_beat += (s - self.est_sample) * 0.5 * (old_rate + new_rate);
        self.est_sample = s;
        self.last_observation = Some(s);

        match target {
            Some((phase, modulus)) => self.observe_phase(s, phase, modulus),
            None => {
                // Tempo-only: nothing to align, but the source is alive.
                if self.lock == Lock::Reacquire {
                    self.lock = Lock::Track;
                }
                self.locked = true;
            }
        }
        self.steer(now);
    }

    /// Advances internal state to `now` (call once per control tick):
    /// declares the source lost when observations stop arriving, and
    /// re-plans the output slew from `now`.
    pub fn advance(&mut self, now: f64) {
        if !now.is_finite() {
            return;
        }
        if let Some(age) = self.observation_age(now) {
            if self.locked && age > LOCK_TIMEOUT_S * self.sample_rate {
                self.locked = false;
                self.jump = None;
                if self.lock == Lock::Track {
                    self.lock = Lock::Reacquire;
                }
            }
        }
        self.steer(now);
    }

    /// Re-anchors so that `beat` falls at `sample`, keeping tempo. Reported
    /// as a discontinuity. The next phase observation is treated as a first
    /// lock: following a source means the source owns the grid.
    pub fn reset(&mut self, sample: f64, beat: f64) {
        if !sample.is_finite() || !beat.is_finite() {
            return;
        }
        self.restart_estimate(sample, beat);
        self.set_output(sample, beat);
        self.lock = Lock::Acquire;
        self.acquired = 0;
        self.discontinuity = true;
    }

    /// For a phase report without a tempo: the tempo implied by the phase
    /// advance since the previous phase report, blended into the current
    /// estimate (a source that never reports tempo can still be followed;
    /// one that does is better).
    fn tempo_from_phase(&self, s: f64, phase: f64, modulus: f64) -> Option<f64> {
        let last = self.last_phase_observation?;
        let samples = s - last;
        if samples <= 0.0 || self.lock != Lock::Track {
            return None;
        }
        // Unwrap the phase advance around what the current tempo predicts.
        let expected = samples * self.est_rate();
        let beats = expected + wrap(phase - self.last_phase_value - expected, modulus);
        let measured = beats / samples * 60.0 * self.sample_rate / (1.0 + self.drift);
        if !(Self::MIN_BPM..=Self::MAX_BPM).contains(&measured) {
            return None;
        }
        let weight = expected / (expected + 4.0 * self.tuning.phase_tau);
        Some(self.bpm + (measured - self.bpm) * weight)
    }

    /// Estimated source tempo in beats per sample.
    fn est_rate(&self) -> f64 {
        self.bpm / (60.0 * self.sample_rate) * (1.0 + self.drift)
    }

    fn est_beat_at(&self, sample: f64) -> f64 {
        self.est_beat + (sample - self.est_sample) * self.est_rate()
    }

    fn slew_end_beat(&self) -> f64 {
        self.anchor_beat + (self.slew_end - self.anchor_sample) * self.slew_rate
    }

    /// A phase observation at `s` (the estimate has been carried to `s`).
    fn observe_phase(&mut self, s: f64, phase: f64, modulus: f64) {
        let ours = self.beat_at_sample(s);
        let out_err = wrap(phase - ours, modulus);
        let est_err = wrap(phase - self.est_beat, modulus);
        let threshold = (self.tuning.jump_s * self.est_rate() * self.sample_rate)
            .min(0.45 * modulus);
        // The gain must not depend on this report's own timing error: a
        // late-stamped report makes its own spacing longer, and weighting
        // it more biases the estimate late by about rate * jitter^2 /
        // spacing (tens of ms at Coarse noise levels). So the gain uses the
        // running mean of the previous spacings, unless this one is a gap.
        let raw = self
            .last_phase_observation
            .map(|last| (s - last) * self.est_rate());
        let spacing = match (self.mean_spacing, raw) {
            (Some(mean), Some(raw)) if raw <= 2.0 * mean => mean,
            (_, Some(raw)) => raw,
            (_, None) => f64::INFINITY,
        };
        if let Some(raw) = raw {
            self.mean_spacing = Some(
                self.mean_spacing
                    .map_or(raw, |mean| mean + (raw - mean) * SPACING_SMOOTHING),
            );
        }
        self.last_phase_observation = Some(s);
        self.phase_error = out_err;

        if core::mem::take(&mut self.resync_requested) {
            self.restart_estimate(s, ours + out_err);
            self.snap_output();
            return;
        }
        match self.lock {
            Lock::Acquire => {
                if self.acquired == 0 {
                    self.restart_estimate(s, ours + out_err);
                } else {
                    self.update(est_err, spacing);
                }
                self.acquired += 1;
                if self.acquired >= self.tuning.acquire_obs {
                    self.snap_output();
                }
            }
            Lock::Reacquire => {
                self.locked = true;
                if out_err.abs() > threshold {
                    self.restart_estimate(s, ours + out_err);
                    self.snap_output();
                } else {
                    self.lock = Lock::Track;
                    self.update(est_err, spacing);
                }
            }
            Lock::Track => {
                self.locked = true;
                if est_err.abs() <= threshold {
                    self.jump = None;
                    self.update(est_err, spacing);
                    return;
                }
                // An outlier: ignored by the estimate, but if the same
                // offset keeps coming back the source jumped (the DJ cued).
                let jump = match self.jump {
                    Some(j) => {
                        let delta = wrap(est_err - j.offset, modulus);
                        if delta.abs() <= threshold {
                            let count = j.count + 1;
                            Jump {
                                since: j.since,
                                count,
                                offset: j.offset + delta / f64::from(count),
                            }
                        } else {
                            Jump {
                                since: s,
                                count: 1,
                                offset: est_err,
                            }
                        }
                    }
                    None => Jump {
                        since: s,
                        count: 1,
                        offset: est_err,
                    },
                };
                self.jump = Some(jump);
                let held = (s - jump.since) * self.est_rate() >= self.tuning.jump_hold_beats;
                if held && jump.count >= self.tuning.jump_hold_obs {
                    // Snap to the mean of the run: as good an estimate of
                    // the new grid as that many observations allow.
                    let beat = self.est_beat + jump.offset;
                    self.restart_estimate(s, beat);
                    self.updates = jump.count - 1;
                    self.snap_output();
                }
            }
        }
    }

    /// One alpha-beta update with residual `err` (beats) after `spacing`
    /// beats since the previous phase observation.
    fn update(&mut self, err: f64, spacing: f64) {
        let tau = self.tuning.phase_tau;
        // 1 - exp(-x) ≈ x / (1 + x): the same first-order fading memory,
        // without a transcendental, and well-behaved for any spacing.
        let fade = if spacing.is_finite() {
            spacing / (spacing + tau)
        } else {
            1.0
        };
        self.updates = self.updates.saturating_add(1);
        // Expanding memory first (a plain running mean of the residuals,
        // optimal while the tempo is known), fading memory once it is as
        // smooth as the fade allows.
        let alpha = fade.max(1.0 / f64::from(self.updates + 1));
        self.est_beat += alpha * err;
        if spacing.is_finite() && spacing > 0.0 {
            // Critically damped alpha-beta: beta = (1 - sqrt(1 - alpha))^2.
            let theta = (1.0 - fade).sqrt();
            let beta = self.tuning.drift_gain * (1.0 - theta) * (1.0 - theta);
            let max = self.tuning.max_drift;
            self.drift = (self.drift + beta * err / spacing.max(MIN_UPDATE_BEATS)).clamp(-max, max);
        }
    }

    fn restart_estimate(&mut self, sample: f64, beat: f64) {
        self.est_sample = sample;
        self.est_beat = beat;
        self.updates = 0;
        self.jump = None;
    }

    /// Output := the estimate, and report it.
    fn snap_output(&mut self) {
        self.set_output(self.est_sample, self.est_beat);
        self.discontinuity = true;
        self.lock = Lock::Track;
        self.locked = true;
        self.acquired = 0;
    }

    /// A straight output line through (`sample`, `beat`) at the estimated
    /// tempo, no slew.
    fn set_output(&mut self, sample: f64, beat: f64) {
        self.anchor_sample = sample;
        self.anchor_beat = beat;
        self.rate = self.est_rate();
        self.slew_rate = self.rate;
        self.slew_end = sample;
    }

    /// Re-anchors the output at `now` (continuously) and plans the slew
    /// that closes the gap to the estimate.
    fn steer(&mut self, now: f64) {
        let t = now.max(self.anchor_sample);
        let ours = self.beat_at_sample(t);
        let base = self.est_rate();
        let mut slew_rate = base;
        let mut slew_end = t;
        if self.locked && self.lock == Lock::Track {
            let gap = self.est_beat_at(t) - ours;
            let deadband = self.tuning.deadband_s * base * self.sample_rate;
            let excess = gap - gap.clamp(-deadband, deadband);
            let c = (excess / self.tuning.slew_tau)
                .clamp(-self.tuning.max_slew, self.tuning.max_slew);
            if c.abs() > 1e-12 {
                slew_rate = base * (1.0 + c);
                // The gap closes at `base * c` beats per sample; after
                // that the output runs at exactly the estimated tempo.
                slew_end = t + excess / (base * c);
            }
        }
        self.anchor_sample = t;
        self.anchor_beat = ours;
        self.rate = base;
        self.slew_rate = slew_rate;
        self.slew_end = slew_end;
    }
}

/// `x` wrapped into `[-modulus / 2, modulus / 2)`.
fn wrap(x: f64, modulus: f64) -> f64 {
    let e = x.rem_euclid(modulus);
    if e >= modulus / 2.0 {
        e - modulus
    } else {
        e
    }
}

impl ClockSource for FollowerClock {
    fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    /// The source's tempo as it reports it (or as estimated from its phase
    /// when it reports none): what a tempo display should show. The
    /// timeline itself runs at this times `1 + drift`, the learned
    /// correction for clock skew between the source and our sample clock,
    /// plus any slew in progress; [`ClockSource::samples_per_beat`] gives
    /// the timeline's actual long-run rate.
    fn tempo_bpm(&self) -> f64 {
        self.bpm
    }

    fn beat_at_sample(&self, sample: f64) -> f64 {
        if sample <= self.slew_end {
            self.anchor_beat + (sample - self.anchor_sample) * self.slew_rate
        } else {
            self.slew_end_beat() + (sample - self.slew_end) * self.rate
        }
    }

    fn sample_at_beat(&self, beat: f64) -> f64 {
        let end_beat = self.slew_end_beat();
        if beat <= end_beat {
            self.anchor_sample + (beat - self.anchor_beat) / self.slew_rate
        } else {
            self.slew_end + (beat - end_beat) / self.rate
        }
    }

    fn samples_per_beat(&self) -> f64 {
        1.0 / self.rate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 48_000.0;
    /// Control tick in the simulations (samples).
    const TICK: f64 = 256.0;

    /// Deterministic xorshift64 noise.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> f64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            (x >> 11) as f64 / (1u64 << 53) as f64
        }

        /// Uniform in `[-a, a)`.
        fn sym(&mut self, a: f64) -> f64 {
            (2.0 * self.next() - 1.0) * a
        }
    }

    #[derive(Clone, Copy, PartialEq)]
    enum Kind {
        Bar,
        Beat,
    }

    /// A simulated source and how it is observed.
    #[derive(Clone, Copy)]
    struct Scenario {
        precision: Precision,
        bpm: f64,
        /// Source clock vs ours: the true tempo on our clock is the
        /// reported one times `1 + skew`.
        skew: f64,
        /// Beats between observations.
        every: f64,
        /// Uniform ± error on each reported sample position, seconds.
        jitter_s: f64,
        /// Delivery delay after the event, seconds (observations are late).
        latency_s: f64,
        kind: Kind,
        seconds: f64,
        /// `(start_s, end_s, to_bpm)`: a linear pitch-fader move.
        ramp: Option<(f64, f64, f64)>,
        /// `(at_s, beats)`: the source's phase jumps (nudge or cue).
        shift: Option<(f64, f64)>,
        /// Initial follower beat offset from the source (beats).
        start_offset: f64,
        seed: u64,
    }

    impl Scenario {
        fn new(precision: Precision) -> Self {
            Self {
                precision,
                bpm: 124.0,
                skew: 40e-6,
                every: 1.0,
                jitter_s: 0.0,
                latency_s: 0.002,
                kind: Kind::Bar,
                seconds: 30.0,
                ramp: None,
                shift: None,
                start_offset: 0.37,
                seed: 0x9E37_79B9_7F4A_7C15,
            }
        }

        fn reported_bpm(&self, t: f64) -> f64 {
            match self.ramp {
                Some((a, b, to)) if t > a => {
                    let x = ((t - a) / (b - a)).min(1.0);
                    self.bpm + (to - self.bpm) * x
                }
                _ => self.bpm,
            }
        }
    }

    struct Run {
        follower: FollowerClock,
        /// `(seconds, error in ms)` per tick, once locked.
        errors: Vec<(f64, f64)>,
        /// Seconds at which a discontinuity was reported.
        snaps: Vec<f64>,
        /// Largest deviation of our local tempo from the source's, as a
        /// fraction, over ticks without a snap.
        max_rate_dev: f64,
    }

    impl Run {
        fn max_abs_error_after(&self, t: f64) -> f64 {
            self.errors
                .iter()
                .filter(|(s, _)| *s >= t)
                .map(|(_, e)| e.abs())
                .fold(0.0, f64::max)
        }

        fn rms_error_after(&self, t: f64) -> f64 {
            let v: Vec<f64> = self
                .errors
                .iter()
                .filter(|(s, _)| *s >= t)
                .map(|(_, e)| e * e)
                .collect();
            (v.iter().sum::<f64>() / v.len() as f64).sqrt()
        }

        fn snaps_after(&self, t: f64) -> usize {
            self.snaps.iter().filter(|s| **s > t).count()
        }
    }

    fn run(sc: &Scenario) -> Run {
        let mut rng = Rng(sc.seed);
        let mut f = FollowerClock::new(SR, 120.0, sc.precision);
        f.reset(0.0, sc.start_offset);
        let _ = f.take_discontinuity();
        let modulus = if sc.kind == Kind::Bar { 4.0 } else { 1.0 };
        // True source beat at our sample `now`.
        let mut beat = 0.0;
        let mut next_obs = 0.0;
        let mut pending: Vec<(f64, Observation)> = Vec::new();
        let mut errors = Vec::new();
        let mut snaps = Vec::new();
        let mut max_rate_dev: f64 = 0.0;
        let mut shifted = false;
        let mut now = 0.0;
        let mut prev_ours: Option<f64> = None;
        while now < sc.seconds * SR {
            let t = now / SR;
            // Source: integrate tempo over the tick (trapezoid; exact for
            // the linear ramps used here).
            let bpm0 = sc.reported_bpm(t) * (1.0 + sc.skew);
            let bpm1 = sc.reported_bpm(t + TICK / SR) * (1.0 + sc.skew);
            let step = 0.5 * (bpm0 + bpm1) / 60.0 * TICK / SR;
            let beat1 = beat + step;
            // Observations due in this tick.
            while next_obs < beat1 {
                let frac = (next_obs - beat) / step;
                let at = now + frac * TICK;
                let sample = at + rng.sym(sc.jitter_s) * SR;
                let phase = match sc.kind {
                    Kind::Bar => Phase::Bar(next_obs.rem_euclid(4.0)),
                    Kind::Beat => Phase::Beat(next_obs.rem_euclid(1.0)),
                };
                let obs = Observation {
                    sample,
                    phase,
                    bpm: Some(sc.reported_bpm(at / SR)),
                };
                pending.push((at + sc.latency_s * SR, obs));
                next_obs += sc.every;
            }
            beat = beat1;
            now += TICK;
            if let Some((at, shift)) = sc.shift {
                if !shifted && now / SR >= at {
                    shifted = true;
                    beat += shift;
                    next_obs = (beat / sc.every).ceil() * sc.every;
                }
            }
            // Deliver, then tick, the way engine::Control does.
            pending.sort_by(|a, b| a.0.total_cmp(&b.0));
            while pending.first().is_some_and(|(due, _)| *due <= now) {
                let (_, obs) = pending.remove(0);
                f.observe(&obs, now);
            }
            f.advance(now);
            let snapped = f.take_discontinuity();
            if snapped {
                snaps.push(now / SR);
            }
            let ours = f.beat_at_sample(now);
            if std::env::var_os("P5_TRACE").is_some() && (now as u64) % 6_144 == 0 && now / SR > 9.5 && now / SR < 16.0 {
                println!(
                    "{:.2}s est {:+.2} ms out {:+.2} ms drift {:+.6} n {}",
                    now / SR,
                    wrap(f.est_beat_at(now) - beat, modulus) * 60_000.0 / sc.bpm,
                    wrap(ours - beat, modulus) * 60_000.0 / sc.bpm,
                    f.drift,
                    f.updates
                );
            }
            if f.is_locked() {
                let err = wrap(ours - beat, modulus);
                let bpm = sc.reported_bpm(now / SR) * (1.0 + sc.skew);
                errors.push((now / SR, err * 60_000.0 / bpm));
                if let (Some(p), false) = (prev_ours, snapped) {
                    let dev = (ours - p) / (bpm / 60.0 * TICK / SR) - 1.0;
                    max_rate_dev = max_rate_dev.max(dev.abs());
                }
            }
            prev_ours = Some(ours);
        }
        Run {
            follower: f,
            errors,
            snaps,
            max_rate_dev,
        }
    }

    #[test]
    fn follows_tempo_and_bar_phase() {
        let mut f = FollowerClock::new(SR, 120.0, Precision::Exact);
        assert!(!f.is_locked());
        f.observe(
            &Observation {
                sample: 10_000.0,
                phase: Phase::Bar(1.0),
                bpm: Some(124.0),
            },
            10_000.0,
        );
        assert!(f.is_locked());
        assert!(f.take_discontinuity(), "first lock is a snap");
        assert!((f.tempo_bpm() - 124.0).abs() < 1e-9);
        let beat = f.beat_at_sample(10_000.0);
        assert!(((beat.rem_euclid(4.0)) - 1.0).abs() < 1e-9, "{beat}");
    }

    #[test]
    fn round_trip_through_a_slew() {
        let mut f = FollowerClock::new(SR, 120.0, Precision::Fine);
        f.observe(
            &Observation {
                sample: 0.0,
                phase: Phase::Bar(0.0),
                bpm: Some(120.0),
            },
            0.0,
        );
        // Source a little ahead: the output starts a slew.
        f.observe(
            &Observation {
                sample: 24_000.0,
                phase: Phase::Bar(1.02),
                bpm: Some(120.0),
            },
            24_000.0,
        );
        assert!(f.slew_end > f.anchor_sample, "a slew is planned");
        for beat in [0.5, 1.0, 1.01, 1.5, 3.0, 100.25] {
            let back = f.beat_at_sample(f.sample_at_beat(beat));
            assert!((back - beat).abs() < 1e-9, "{beat} -> {back}");
        }
    }

    #[test]
    fn wrap_is_symmetric() {
        assert_eq!(wrap(0.25, 1.0), 0.25);
        assert_eq!(wrap(0.75, 1.0), -0.25);
        assert_eq!(wrap(-3.5, 4.0), 0.5);
        assert_eq!(wrap(2.0, 4.0), -2.0);
    }

    /// The typical observation pattern for each precision.
    fn typical(p: Precision) -> Scenario {
        let mut sc = Scenario::new(p);
        match p {
            // Link / precise position: ~20 ms reports, sub-ms error.
            Precision::Exact => {
                sc.every = 0.04;
                sc.jitter_s = 0.000_1;
            }
            // Beat packets: one per beat, a few ms of network jitter.
            Precision::Fine => sc.jitter_s = 0.003,
            // Opus Quad: one per beat, phase good to ±200 ms.
            Precision::Coarse => {
                sc.jitter_s = 0.2;
                sc.seconds = 180.0;
            }
            // MIDI clock: 24 ppqn, ±1 ms.
            Precision::Jittery => {
                sc.every = 1.0 / 24.0;
                sc.jitter_s = 0.001;
            }
        }
        sc
    }

    #[test]
    #[ignore = "trace; run with P5_TRACE=1 --ignored --nocapture"]
    fn trace() {
        let p = match std::env::var("P5_TRACE").as_deref() {
            Ok("exact") => Precision::Exact,
            Ok("fine") => Precision::Fine,
            Ok("jittery") => Precision::Jittery,
            _ => Precision::Coarse,
        };
        let mut sc = typical(p);
        if std::env::var_os("P5_NUDGE").is_some() {
            sc.shift = Some((10.0, 0.6 * p.tuning().jump_s * sc.bpm / 60.0));
        }
        let r = run(&sc);
        println!("snaps {:?}", r.snaps);
    }

    #[test]
    #[ignore = "tuning report; run with --ignored --nocapture"]
    fn tuning_report() {
        for p in [
            Precision::Exact,
            Precision::Fine,
            Precision::Coarse,
            Precision::Jittery,
        ] {
            for (name, edit) in [
                ("steady", (|_: &mut Scenario| {}) as fn(&mut Scenario)),
                ("nudge", |sc| sc.shift = Some((10.0, 0.6 * sc.precision.tuning().jump_s * sc.bpm / 60.0))),
                ("ramp", |sc| sc.ramp = Some((10.0, 14.0, sc.bpm + 6.0))),
                ("cue", |sc| sc.shift = Some((10.0, 1.5))),
            ] {
                let mut worst: (f64, f64, f64, f64, usize, f64) = (0.0, 0.0, 0.0, 0.0, 0, 0.0);
                for seed in 1..=6u64 {
                    let mut sc = typical(p);
                    sc.seed = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15);
                    edit(&mut sc);
                    let r = run(&sc);
                    let after = sc.shift.map_or(5.0, |(at, _)| at);
                    worst.0 = worst.0.max(r.max_abs_error_after(5.0));
                    worst.1 = worst.1.max(r.max_abs_error_after(after + 4.0));
                    worst.2 = worst.2.max(r.rms_error_after(60.0f64.min(sc.seconds / 2.0)));
                    worst.3 = worst.3.max(r.max_rate_dev);
                    worst.4 = worst.4.max(r.snaps.len());
                    worst.5 = worst.5.max((r.follower.tempo_bpm() / (sc.reported_bpm(sc.seconds) * (1.0 + sc.skew)) - 1.0).abs());
                }
                println!(
                    "{p:?} {name}: max after 5 s {:.3} ms, max 4 s after event {:.3} ms, rms late {:.3} ms, rate dev {:.4}, snaps {}, tempo err {:.6}",
                    worst.0, worst.1, worst.2, worst.3, worst.4, worst.5
                );
            }
        }
    }
}
