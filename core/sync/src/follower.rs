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

/// Integral gain (fraction of critical damping) that refines the tempo of a
/// source that reports none.
const NO_TEMPO_GAIN: f64 = 0.25;

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
                phase_tau: 0.1,
                drift_gain: 1.0,
                max_drift: 0.01,
                slew_tau: 0.1,
                max_slew: 0.05,
                deadband_s: 0.0,
                jump_s: 0.020,
                jump_hold_beats: 0.25,
                jump_hold_obs: 2,
            },
            Self::Fine => Tuning {
                acquire_obs: 1,
                phase_tau: 0.5,
                drift_gain: 0.05,
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
                deadband_s: 0.000_25,
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
    tempo_reported: bool,
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
            tempo_reported: false,
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
        let reported = obs
            .bpm
            .filter(|b| b.is_finite() && *b > 0.0)
            .map(|b| b.clamp(Self::MIN_BPM, Self::MAX_BPM));
        self.tempo_reported = reported.is_some();
        // A source without tempo reports: right after a snap, take the
        // tempo from the phase advance since that snap (the integral path
        // refines it from then on).
        let acquired_tempo = match (reported, target) {
            (None, Some((p, m))) if self.updates == 0 => self.tempo_from_phase(s, p, m),
            _ => None,
        };

        // Carry the estimate forward to `s`. The tempo report describes the
        // tempo at `s`; across a change, average old and new (exact for a
        // linear ramp, half-way for a step at an unknown moment).
        let old_rate = self.est_rate();
        if let Some(bpm) = reported.or(acquired_tempo) {
            self.bpm = bpm;
        }
        let new_rate = self.est_rate();
        // An acquired tempo is the average since the snap by construction.
        let mean_rate = if acquired_tempo.is_some() {
            new_rate
        } else {
            0.5 * (old_rate + new_rate)
        };
        self.est_beat += (s - self.est_sample) * mean_rate;
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

    /// The source tempo implied by the phase advance from the estimate's
    /// anchor (the last snap) to this report, unwrapped around what the
    /// current tempo predicts. `None` without a usable anchor.
    fn tempo_from_phase(&self, s: f64, phase: f64, modulus: f64) -> Option<f64> {
        if self.lock != Lock::Track && self.acquired == 0 {
            return None;
        }
        let samples = s - self.est_sample;
        if samples <= 0.0 || self.last_phase_observation.is_none() {
            return None;
        }
        let expected = samples * self.est_rate();
        let anchor_phase = self.est_beat.rem_euclid(modulus);
        let beats = expected + wrap(phase - anchor_phase - expected, modulus);
        let bpm = beats / samples * 60.0 * self.sample_rate / (1.0 + self.drift);
        (Self::MIN_BPM..=Self::MAX_BPM)
            .contains(&bpm)
            .then_some(bpm)
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
        let threshold =
            (self.tuning.jump_s * self.est_rate() * self.sample_rate).min(0.45 * modulus);
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
            let beta = (1.0 - theta) * (1.0 - theta);
            let slope = err / spacing.max(MIN_UPDATE_BEATS);
            if self.tempo_reported {
                // The integral only corrects the reported tempo for clock
                // skew and quantisation: small, slow, clamped.
                let max = self.tuning.max_drift;
                self.drift = (self.drift + self.tuning.drift_gain * beta * slope).clamp(-max, max);
            } else {
                // No tempo report: the integral path is the tempo estimate
                // (learning `drift` too would split one error between two
                // integrators). Overdamped: the tempo was acquired at the
                // snap, this only refines it, and a display wants it calm.
                let gain = NO_TEMPO_GAIN * beta * slope;
                self.bpm = (self.bpm * (1.0 + gain)).clamp(Self::MIN_BPM, Self::MAX_BPM);
            }
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
            let c =
                (excess / self.tuning.slew_tau).clamp(-self.tuning.max_slew, self.tuning.max_slew);
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
        /// Whether observations carry the tempo.
        reports_bpm: bool,
        /// The source's true beat at sample 0 (negative: counting in).
        source_start: f64,
        /// `(period_s, length_s)`: delivery stalls for `length_s` at the
        /// start of every `period_s` (a congested network); what was due
        /// arrives in one burst when the stall ends.
        stall: Option<(f64, f64)>,
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
                reports_bpm: true,
                source_start: 0.0,
                stall: None,
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
        fn max_abs_error_between(&self, from: f64, to: f64) -> f64 {
            self.errors
                .iter()
                .filter(|(t, _)| *t >= from && *t < to)
                .map(|(_, e)| e.abs())
                .fold(0.0, f64::max)
        }

        fn max_abs_error_after(&self, from: f64) -> f64 {
            self.max_abs_error_between(from, f64::INFINITY)
        }

        fn rms_error_after(&self, from: f64) -> f64 {
            let v: Vec<f64> = self
                .errors
                .iter()
                .filter(|(t, _)| *t >= from)
                .map(|(_, e)| e * e)
                .collect();
            (v.iter().sum::<f64>() / v.len() as f64).sqrt()
        }

        /// Seconds after `from` until the error stays within `tol_ms`.
        fn settle_time(&self, from: f64, tol_ms: f64) -> f64 {
            let last_bad = self
                .errors
                .iter()
                .filter(|(t, e)| *t >= from && e.abs() > tol_ms)
                .map(|(t, _)| *t)
                .fold(from, f64::max);
            last_bad - from
        }
    }

    fn run(sc: &Scenario) -> Run {
        let mut rng = Rng(sc.seed);
        let mut f = FollowerClock::new(SR, 120.0, sc.precision);
        f.reset(0.0, sc.start_offset);
        let _ = f.take_discontinuity();
        let modulus = if sc.kind == Kind::Bar { 4.0 } else { 1.0 };
        // True source beat at our sample `now`.
        let mut beat = sc.source_start;
        let mut next_obs = (sc.source_start / sc.every).ceil() * sc.every;
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
                    bpm: sc.reports_bpm.then(|| sc.reported_bpm(at / SR)),
                };
                let mut due = at + sc.latency_s * SR;
                if let Some((period, length)) = sc.stall {
                    let phase = (due / SR).rem_euclid(period);
                    if phase < length {
                        due += (length - phase) * SR;
                    }
                }
                pending.push((due, obs));
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

    /// A few seeds per scenario so no assertion rests on one lucky draw.
    fn seeds(sc: Scenario) -> impl Iterator<Item = Run> {
        (1..=4u64).map(move |k| {
            let mut sc = sc;
            sc.seed = k.wrapping_mul(0x9E37_79B9_7F4A_7C15);
            run(&sc)
        })
    }

    fn obs(sample: f64, phase: Phase, bpm: f64) -> Observation {
        Observation {
            sample,
            phase,
            bpm: Some(bpm),
        }
    }

    /// Feeds a perfect 120 BPM bar-phase source with one report per beat
    /// from beat `from` to `to`, ticking in between.
    fn feed_perfect(f: &mut FollowerClock, from: u32, to: u32) {
        for k in from..to {
            let s = f64::from(k) * 24_000.0;
            f.observe(&obs(s, Phase::Bar(f64::from(k % 4)), 120.0), s);
            f.advance(s + 12_000.0);
        }
    }

    // ---- basics ----------------------------------------------------------

    #[test]
    fn follows_tempo_and_bar_phase() {
        let mut f = FollowerClock::new(SR, 120.0, Precision::Exact);
        assert!(!f.is_locked());
        f.observe(&obs(10_000.0, Phase::Bar(1.0), 124.0), 10_000.0);
        assert!(f.is_locked());
        assert!(f.take_discontinuity(), "first lock is a snap");
        assert!((f.tempo_bpm() - 124.0).abs() < 1e-9);
        let beat = f.beat_at_sample(10_000.0);
        assert!(((beat.rem_euclid(4.0)) - 1.0).abs() < 1e-9, "{beat}");
    }

    #[test]
    fn round_trip_through_a_slew() {
        let mut f = FollowerClock::new(SR, 120.0, Precision::Fine);
        f.observe(&obs(0.0, Phase::Bar(0.0), 120.0), 0.0);
        // Source a little ahead: the output starts a slew.
        f.observe(&obs(24_000.0, Phase::Bar(1.02), 120.0), 24_000.0);
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

    #[test]
    fn beat_phase_keeps_our_bar_count_and_bar_phase_aligns_bars() {
        // Our timeline sits at beat 6.02 (bar position 2.02) at sample 0.
        let mut f = FollowerClock::new(SR, 120.0, Precision::Fine);
        f.reset(0.0, 6.02);
        f.observe(&obs(0.0, Phase::Beat(0.0), 120.0), 0.0);
        assert!((f.beat_at_sample(0.0) - 6.0).abs() < 1e-9);

        let mut f = FollowerClock::new(SR, 120.0, Precision::Fine);
        f.reset(0.0, 6.02);
        f.observe(&obs(0.0, Phase::Bar(0.0), 120.0), 0.0);
        let beat = f.beat_at_sample(0.0);
        assert!(beat.rem_euclid(4.0).abs() < 1e-9, "{beat}");
        assert!((beat - 6.02).abs() <= 2.0, "nearest downbeat: {beat}");
    }

    #[test]
    fn tempo_only_moves_tempo_continuously() {
        let mut f = FollowerClock::new(SR, 120.0, Precision::Jittery);
        let _ = f.take_discontinuity();
        let before = f.beat_at_sample(30_000.0);
        f.observe(&obs(30_000.0, Phase::TempoOnly, 90.0), 30_000.0);
        assert!(!f.take_discontinuity());
        assert!(f.is_locked());
        assert!((f.beat_at_sample(30_000.0) - before).abs() < 1e-12);
        assert!((f.tempo_bpm() - 90.0).abs() < 1e-9);
        assert!((f.samples_per_beat() - 32_000.0).abs() < 1e-6);
    }

    // ---- convergence and steady state per precision -----------------------

    /// Exact (Link, CDJ-3000 precise position): sub-millisecond steady
    /// state; after a 10 ms nudge, back under 1 ms within a beat.
    #[test]
    fn exact_is_sub_ms_and_relocks_within_a_beat() {
        let mut sc = typical(Precision::Exact);
        sc.shift = Some((10.0, 0.010 * sc.bpm / 60.0));
        let beat_s = 60.0 / sc.bpm;
        for r in seeds(sc) {
            assert_eq!(r.snaps.len(), 1, "only the first lock snaps");
            assert!(r.max_abs_error_between(0.5, 10.0) < 0.5);
            let settle = r.settle_time(10.0, 1.0);
            assert!(settle < beat_s, "{settle} s");
            assert!(r.max_abs_error_after(10.0 + 2.0 * beat_s) < 0.5);
        }
    }

    /// Fine (beat packets, ±3 ms jitter): a few ms at worst, about a
    /// millisecond rms; a 30 ms nudge is absorbed within about two beats.
    #[test]
    fn fine_tracks_jitter_and_relocks_within_two_beats() {
        let mut sc = typical(Precision::Fine);
        sc.shift = Some((10.0, 0.030 * sc.bpm / 60.0));
        let beat_s = 60.0 / sc.bpm;
        for r in seeds(sc) {
            assert_eq!(r.snaps.len(), 1, "jitter and nudges never snap");
            assert!(r.max_abs_error_between(1.0, 10.0) < 3.0);
            let rms = r.rms_error_after(20.0);
            assert!(rms < 1.5, "{rms} ms");
            let settle = r.settle_time(10.0, 5.0);
            assert!(settle < 2.5 * beat_s, "{settle} s");
            assert!(r.max_abs_error_after(20.0) < 3.0);
        }
    }

    /// Coarse (Opus Quad, ±200 ms): trust the tempo, average the phase
    /// over many beats, never jump after the first lock.
    #[test]
    fn coarse_phase_noise_is_averaged_without_jumps() {
        let sc = typical(Precision::Coarse);
        for r in seeds(sc) {
            assert_eq!(r.snaps.len(), 1, "the timeline never jumps after lock");
            let first = r.max_abs_error_after(0.0);
            assert!(first < 100.0, "{first} ms");
            let later = r.max_abs_error_after(60.0);
            assert!(later < 50.0, "{later} ms");
            let rms = r.rms_error_after(60.0);
            assert!(rms < 25.0, "{rms} ms");
            // Slewing only, within the precision's bound (plus the tempo
            // skew between the clocks).
            assert!(r.max_rate_dev < 0.0105, "{}", r.max_rate_dev);
        }
    }

    /// Jittery (MIDI clock, 24 ppqn, ±1 ms): well under a millisecond.
    #[test]
    fn jittery_pulses_are_smoothed() {
        for r in seeds(typical(Precision::Jittery)) {
            assert_eq!(r.snaps.len(), 1);
            let worst = r.max_abs_error_after(2.0);
            assert!(worst < 1.0, "{worst} ms");
            assert!(r.rms_error_after(2.0) < 0.5);
        }
    }

    // ---- tempo ramps --------------------------------------------------------

    /// A pitch-fader move of +6 BPM over four seconds.
    #[test]
    fn tempo_ramps_are_tracked() {
        for (p, tol_ms) in [
            (Precision::Exact, 0.5),
            (Precision::Fine, 4.0),
            (Precision::Jittery, 1.5),
        ] {
            let mut sc = typical(p);
            sc.ramp = Some((10.0, 14.0, sc.bpm + 6.0));
            for r in seeds(sc) {
                assert_eq!(r.snaps.len(), 1, "{p:?}");
                let worst = r.max_abs_error_after(2.0);
                assert!(worst < tol_ms, "{p:?}: {worst} ms");
                assert!((r.follower.tempo_bpm() - (sc.bpm + 6.0)).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn tempo_is_estimated_when_the_source_reports_none() {
        let mut sc = typical(Precision::Fine);
        sc.reports_bpm = false;
        sc.bpm = 131.0; // the follower starts at 120
        for r in seeds(sc) {
            let bpm = r.follower.tempo_bpm();
            assert!((bpm - 131.0).abs() < 0.1, "{bpm}");
            let worst = r.max_abs_error_after(15.0);
            assert!(worst < 3.0, "{worst} ms");
        }
    }

    // ---- snaps --------------------------------------------------------------

    #[test]
    fn resync_snaps_on_the_next_observation() {
        let mut f = FollowerClock::new(SR, 120.0, Precision::Fine);
        feed_perfect(&mut f, 0, 8);
        assert!(f.take_discontinuity(), "first lock");
        // The source is now 10 ms (0.02 beat) ahead: normally a slew...
        let s = 8.0 * 24_000.0;
        f.observe(&obs(s, Phase::Bar(0.02), 120.0), s);
        assert!(!f.take_discontinuity());
        assert!((f.beat_at_sample(s) - 8.0).abs() < 1e-6);
        // ...but after a resync request the next observation snaps.
        f.request_resync();
        let s = 9.0 * 24_000.0;
        f.observe(&obs(s, Phase::Bar(1.02), 120.0), s);
        assert!(f.take_discontinuity());
        assert!((f.beat_at_sample(s) - 9.02).abs() < 1e-9);
        assert!(!f.take_discontinuity(), "reported once");
    }

    /// The DJ cues 1.5 beats away: ignored as an outlier at first, snapped
    /// to once it has held for the precision's hold time.
    #[test]
    fn a_cue_jump_snaps_after_the_hold_time() {
        let mut sc = typical(Precision::Fine);
        sc.shift = Some((10.0, 1.5));
        let beat_s = 60.0 / sc.bpm;
        for r in seeds(sc) {
            assert_eq!(r.snaps.len(), 2, "{:?}", r.snaps);
            let delay = r.snaps[1] - 10.0;
            assert!(delay >= 1.5 * beat_s && delay < 3.0 * beat_s, "{delay}");
            assert!(r.max_abs_error_after(r.snaps[1]) < 3.0);
        }
        // Same for a bar-blind source that jumps by a fraction of a beat.
        let mut sc = typical(Precision::Exact);
        sc.kind = Kind::Beat;
        sc.shift = Some((10.0, 0.3));
        for r in seeds(sc) {
            assert_eq!(r.snaps.len(), 2, "{:?}", r.snaps);
            assert!(r.max_abs_error_after(r.snaps[1] + 0.01) < 0.5);
        }
    }

    #[test]
    fn a_single_outlier_is_ignored() {
        let mut f = FollowerClock::new(SR, 120.0, Precision::Fine);
        feed_perfect(&mut f, 0, 8);
        let _ = f.take_discontinuity();
        // One report 150 ms late (a network hiccup).
        let s = 8.0 * 24_000.0 + 7_200.0;
        f.observe(&obs(s, Phase::Bar(0.0), 120.0), s);
        f.advance(s + 1_000.0);
        assert!(!f.take_discontinuity());
        feed_perfect(&mut f, 9, 12);
        assert!(!f.take_discontinuity());
        assert!((f.beat_at_sample(12.0 * 24_000.0) - 12.0).abs() < 1e-3);
    }

    // ---- delivery quirks ----------------------------------------------------

    #[test]
    fn late_duplicate_and_out_of_order_observations() {
        let mut f = FollowerClock::new(SR, 120.0, Precision::Fine);
        feed_perfect(&mut f, 0, 4);
        let _ = f.take_discontinuity();
        let snapshot = f.clone();
        // A duplicate of the last report and an older one change nothing.
        let last = 3.0 * 24_000.0;
        f.observe(&obs(last, Phase::Bar(3.0), 120.0), last + 5_000.0);
        f.observe(
            &obs(last - 24_000.0, Phase::Bar(0.5), 120.0),
            last + 5_000.0,
        );
        assert_eq!(f.beat_at_sample(1e6), snapshot.beat_at_sample(1e6));
        // A late one (300 ms old when it arrives) is used at its own sample:
        // a perfect report must not disturb a perfect lock.
        let s = 4.0 * 24_000.0;
        f.observe(&obs(s, Phase::Bar(0.0), 120.0), s + 14_400.0);
        assert!(!f.take_discontinuity());
        assert!((f.beat_at_sample(s + 14_400.0) - 4.6).abs() < 1e-9);
        // Reports more than the lock timeout away from now are stale.
        f.observe(&obs(s + 1_000.0, Phase::Bar(1.0), 140.0), s + 3.0 * SR);
        assert!((f.tempo_bpm() - 120.0).abs() < 1e-9);
    }

    #[test]
    fn loses_lock_when_observations_stop_and_reacquires() {
        let mut f = FollowerClock::new(SR, 120.0, Precision::Fine);
        f.observe(&obs(0.0, Phase::Beat(0.0), 126.0), 0.0);
        let _ = f.take_discontinuity();
        f.advance(SR);
        assert!(f.is_locked());
        f.advance(3.0 * SR);
        assert!(!f.is_locked());
        // Free-runs at the last tempo, continuously.
        assert!((f.tempo_bpm() - 126.0).abs() < 1e-9);
        let a = f.beat_at_sample(3.0 * SR);
        let b = f.beat_at_sample(4.0 * SR);
        assert!((b - a - 126.0 / 60.0).abs() < 1e-9);
        assert!(!f.take_discontinuity());
        // The source comes back on our grid (within the jump threshold):
        // tracking resumes without a snap.
        let s = 4.0 * SR;
        let p = (f.beat_at_sample(s) + 0.01).rem_euclid(1.0);
        f.observe(&obs(s, Phase::Beat(p), 126.0), s);
        assert!(f.is_locked());
        assert!(!f.take_discontinuity());
        // Lost again; this time it comes back somewhere else: snap.
        f.advance(7.0 * SR);
        assert!(!f.is_locked());
        let s = 7.5 * SR;
        let p = (f.beat_at_sample(s) + 0.4).rem_euclid(1.0);
        f.observe(&obs(s, Phase::Beat(p), 126.0), s);
        assert!(f.take_discontinuity());
        let got = f.beat_at_sample(s).rem_euclid(1.0);
        assert!((got - p).abs() < 1e-9, "{got} vs {p}");
    }

    #[test]
    fn precision_change_keeps_the_timeline() {
        let mut f = FollowerClock::new(SR, 120.0, Precision::Fine);
        feed_perfect(&mut f, 0, 4);
        let _ = f.take_discontinuity();
        let before = f.beat_at_sample(100_000.0);
        f.set_precision(Precision::Exact);
        assert_eq!(f.precision(), Precision::Exact);
        assert!(f.is_locked());
        feed_perfect(&mut f, 4, 8);
        assert!(!f.take_discontinuity());
        assert!((f.beat_at_sample(100_000.0) - before).abs() < 1e-9);
    }

    #[test]
    #[ignore = "exploration"]
    fn explore_adversarial() {
        type Edit = fn(&mut Scenario);
        let edits: [(&str, Edit); 12] = [
            ("steady", |_| {}),
            ("60bpm", |sc| sc.bpm = 60.0),
            ("200bpm", |sc| sc.bpm = 200.0),
            ("ramp120-128/8s", |sc| {
                sc.bpm = 120.0;
                sc.ramp = Some((10.0, 18.0, 128.0));
            }),
            ("ramp128-120/8s", |sc| {
                sc.bpm = 128.0;
                sc.ramp = Some((10.0, 18.0, 120.0));
            }),
            ("stall300ms/2s", |sc| sc.stall = Some((2.0, 0.3))),
            ("stall300ms/0.7s", |sc| sc.stall = Some((0.7, 0.3))),
            ("half-beat", |sc| sc.shift = Some((10.0, 0.5))),
            ("half-beat-Beat", |sc| {
                sc.kind = Kind::Beat;
                sc.shift = Some((10.0, 0.5));
            }),
            ("negative", |sc| sc.source_start = -7.3),
            ("bar-wrap 2 beats", |sc| sc.shift = Some((10.0, 2.0))),
            ("no-bpm 60", |sc| {
                sc.reports_bpm = false;
                sc.bpm = 60.0;
            }),
        ];
        for p in [
            Precision::Exact,
            Precision::Fine,
            Precision::Coarse,
            Precision::Jittery,
        ] {
            for (name, edit) in edits {
                let mut sc = typical(p);
                edit(&mut sc);
                let mut worst = [0.0f64; 4];
                let mut snaps = (usize::MAX, 0);
                let mut bpm_err: f64 = 0.0;
                for r in seeds(sc) {
                    worst[0] = worst[0].max(r.max_abs_error_between(3.0, 10.0));
                    worst[1] = worst[1].max(r.max_abs_error_after(10.0));
                    worst[2] = worst[2].max(r.max_abs_error_after(sc.seconds - 5.0));
                    worst[3] = worst[3].max(r.max_rate_dev);
                    snaps.0 = snaps.0.min(r.snaps.len());
                    snaps.1 = snaps.1.max(r.snaps.len());
                    let want = sc.reported_bpm(sc.seconds);
                    bpm_err = bpm_err.max((r.follower.tempo_bpm() - want).abs());
                }
                println!(
                    "{p:?} {name}: 3-10s {:.2} ms, after 10s {:.2} ms, end {:.2} ms, rate dev {:.4}, snaps {:?}, bpm err {:.4}",
                    worst[0], worst[1], worst[2], worst[3], snaps, bpm_err
                );
            }
        }
    }

    #[test]
    #[ignore = "exploration"]
    fn explore_no_bpm() {
        let mut f = FollowerClock::new(SR, 120.0, Precision::Fine);
        f.reset(0.0, 0.37);
        let _ = f.take_discontinuity();
        for k in 0..12u32 {
            let s = f64::from(k) * 48_000.0 + 100.0;
            f.observe(
                &Observation {
                    sample: s,
                    phase: Phase::Bar(f64::from(k % 4)),
                    bpm: None,
                },
                s + 96.0,
            );
            f.advance(s + 200.0);
            println!(
                "k {k}: bpm {:.4} snap {} ours {:.4} err {:.4} lock {:?} upd {}",
                f.tempo_bpm(),
                f.take_discontinuity(),
                f.beat_at_sample(s),
                f.phase_error(),
                f.lock,
                f.updates
            );
        }
    }

    // ---- tuning report (not a test) -----------------------------------------

    #[test]
    #[ignore = "tuning report; run with --ignored --nocapture"]
    fn tuning_report() {
        type Edit = fn(&mut Scenario);
        let edits: [(&str, Edit); 4] = [
            ("steady", |_| {}),
            ("nudge", |sc| {
                sc.shift = Some((10.0, 0.6 * sc.precision.tuning().jump_s * sc.bpm / 60.0));
            }),
            ("ramp", |sc| sc.ramp = Some((10.0, 14.0, sc.bpm + 6.0))),
            ("cue", |sc| sc.shift = Some((10.0, 1.5))),
        ];
        for p in [
            Precision::Exact,
            Precision::Fine,
            Precision::Coarse,
            Precision::Jittery,
        ] {
            for (name, edit) in edits {
                let mut sc = typical(p);
                edit(&mut sc);
                let event = sc.shift.map_or(10.0, |(at, _)| at);
                let mut worst = [0.0f64; 4];
                let mut snaps = 0;
                for r in seeds(sc) {
                    let steady = r.max_abs_error_between(5.0, event);
                    worst[0] = worst[0].max(steady);
                    worst[1] = worst[1].max(r.settle_time(event, 2.0 * steady));
                    worst[2] = worst[2].max(r.rms_error_after(sc.seconds / 2.0));
                    worst[3] = worst[3].max(r.max_rate_dev);
                    snaps = snaps.max(r.snaps.len());
                }
                println!(
                    "{p:?} {name}: steady max {:.3} ms, settle {:.2} s, late rms {:.3} ms, rate dev {:.4}, snaps {snaps}",
                    worst[0], worst[1], worst[2], worst[3]
                );
            }
        }
    }
}
