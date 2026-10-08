//! Following an external tempo source.
//!
//! Every external source (Ableton Link, Pro DJ Link, Opus Quad, MIDI clock,
//! the browser bridge, tap) is reduced to [`Observation`]s: "at this sample
//! position the source was at this phase, at about this tempo". A
//! [`FollowerClock`] turns a stream of observations into a smooth timeline
//! the scheduler can use, via a phase-locked loop tuned per source
//! [`Precision`].
//!
//! PLACEHOLDER IMPLEMENTATION: `observe` snaps tempo and phase directly.
//! The real PLL (ADR-0006) replaces the internals; the public API is fixed.

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

/// A [`ClockSource`] steered by observations.
#[derive(Clone, Debug)]
pub struct FollowerClock {
    sample_rate: f64,
    precision: Precision,
    bpm: f64,
    anchor_sample: f64,
    anchor_beat: f64,
    locked: bool,
    discontinuity: bool,
    resync_requested: bool,
    last_observation: Option<f64>,
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
        Self {
            sample_rate,
            precision,
            bpm: initial_bpm.clamp(Self::MIN_BPM, Self::MAX_BPM),
            anchor_sample: 0.0,
            anchor_beat: 0.0,
            locked: false,
            discontinuity: false,
            resync_requested: false,
            last_observation: None,
        }
    }

    /// Current tuning.
    #[must_use]
    pub fn precision(&self) -> Precision {
        self.precision
    }

    /// Changes the tuning (e.g. a source switched from beat packets to
    /// precise position packets).
    pub fn set_precision(&mut self, precision: Precision) {
        self.precision = precision;
    }

    /// Whether the follower is tracking a live source.
    #[must_use]
    pub fn is_locked(&self) -> bool {
        self.locked
    }

    /// Samples since the last observation, if any arrived.
    #[must_use]
    pub fn observation_age(&self, now: f64) -> Option<f64> {
        self.last_observation.map(|s| now - s)
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
    /// position (observations may be slightly in the past).
    pub fn observe(&mut self, obs: &Observation, now: f64) {
        let _ = now;
        if let Some(bpm) = obs.bpm {
            if bpm.is_finite() && bpm > 0.0 {
                // Re-anchor at the observation so the change is continuous.
                let beat = self.beat_at_sample(obs.sample);
                self.anchor_beat = beat;
                self.anchor_sample = obs.sample;
                self.bpm = bpm.clamp(Self::MIN_BPM, Self::MAX_BPM);
            }
        }
        let ours = self.beat_at_sample(obs.sample);
        let target = match obs.phase {
            Phase::Bar(p) => Some((p, 4.0)),
            Phase::Beat(p) => Some((p, 1.0)),
            Phase::TempoOnly => None,
        };
        if let Some((phase, modulus)) = target {
            let mut err = (phase - ours).rem_euclid(modulus);
            if err > modulus / 2.0 {
                err -= modulus;
            }
            if err.abs() > 1e-9 {
                self.anchor_beat = ours + err;
                self.anchor_sample = obs.sample;
                self.discontinuity = true;
            }
        }
        self.resync_requested = false;
        self.locked = true;
        self.last_observation = Some(obs.sample);
    }

    /// Advances internal state to `now` (call once per control tick).
    /// Declares the source lost when observations stop arriving.
    pub fn advance(&mut self, now: f64) {
        if let Some(age) = self.observation_age(now) {
            if age > 2.0 * self.sample_rate {
                self.locked = false;
            }
        }
    }

    /// Re-anchors so that `beat` falls at `sample`, keeping tempo.
    pub fn reset(&mut self, sample: f64, beat: f64) {
        self.anchor_sample = sample;
        self.anchor_beat = beat;
        self.discontinuity = true;
    }
}

impl ClockSource for FollowerClock {
    fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    fn tempo_bpm(&self) -> f64 {
        self.bpm
    }

    fn beat_at_sample(&self, sample: f64) -> f64 {
        self.anchor_beat + (sample - self.anchor_sample) / self.samples_per_beat()
    }

    fn sample_at_beat(&self, beat: f64) -> f64 {
        self.anchor_sample + (beat - self.anchor_beat) * self.samples_per_beat()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_tempo_and_bar_phase() {
        let sr = 48_000.0;
        let mut f = FollowerClock::new(sr, 120.0, Precision::Exact);
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
        assert!((f.tempo_bpm() - 124.0).abs() < 1e-9);
        let beat = f.beat_at_sample(10_000.0);
        assert!(((beat.rem_euclid(4.0)) - 1.0).abs() < 1e-9, "{beat}");
    }

    #[test]
    fn loses_lock_when_observations_stop() {
        let sr = 48_000.0;
        let mut f = FollowerClock::new(sr, 120.0, Precision::Fine);
        f.observe(
            &Observation {
                sample: 0.0,
                phase: Phase::Beat(0.0),
                bpm: Some(120.0),
            },
            0.0,
        );
        f.advance(sr);
        assert!(f.is_locked());
        f.advance(3.0 * sr);
        assert!(!f.is_locked());
    }
}
