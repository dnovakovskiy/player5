use crate::ClockSource;

/// The free-running internal clock: a fixed tempo anchored to a sample
/// position. Tempo changes re-anchor at the moment of the change so the beat
/// count stays continuous.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InternalClock {
    sample_rate: f64,
    bpm: f64,
    anchor_sample: f64,
    anchor_beat: f64,
}

impl InternalClock {
    /// Lowest accepted tempo.
    pub const MIN_BPM: f64 = 20.0;
    /// Highest accepted tempo.
    pub const MAX_BPM: f64 = 400.0;

    /// A clock at `bpm` with beat 0 at sample 0.
    #[must_use]
    pub fn new(sample_rate: f64, bpm: f64) -> Self {
        Self {
            sample_rate,
            bpm: bpm.clamp(Self::MIN_BPM, Self::MAX_BPM),
            anchor_sample: 0.0,
            anchor_beat: 0.0,
        }
    }

    /// Restarts the beat count: beat 0 falls on `sample`.
    pub fn reset(&mut self, sample: f64) {
        self.anchor_sample = sample;
        self.anchor_beat = 0.0;
    }

    /// Changes tempo without a jump in the beat count: the beat at `now`
    /// stays where it is and later beats follow the new tempo.
    pub fn set_tempo(&mut self, bpm: f64, now_sample: f64) {
        self.anchor_beat = self.beat_at_sample(now_sample);
        self.anchor_sample = now_sample;
        self.bpm = bpm.clamp(Self::MIN_BPM, Self::MAX_BPM);
    }

    /// Changes the sample rate, keeping the anchor at the same beat.
    pub fn set_sample_rate(&mut self, sample_rate: f64, now_sample: f64) {
        let beat = self.beat_at_sample(now_sample);
        self.sample_rate = sample_rate;
        self.anchor_beat = beat;
        self.anchor_sample = now_sample;
    }
}

impl ClockSource for InternalClock {
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
    use crate::{AdjustedClock, ClockControls};

    #[test]
    fn beats_map_to_samples_at_120_bpm() {
        let c = InternalClock::new(48_000.0, 120.0);
        assert_eq!(c.samples_per_beat(), 24_000.0);
        assert_eq!(c.sample_at_beat(0.0), 0.0);
        assert_eq!(c.sample_at_beat(1.0), 24_000.0);
        assert_eq!(c.sample_at_beat(0.25), 6_000.0);
        assert_eq!(c.beat_at_sample(12_000.0), 0.5);
    }

    #[test]
    fn round_trip() {
        let c = InternalClock::new(44_100.0, 133.7);
        for beat in [0.0, 0.125, 3.0, 17.75, 1_000.5] {
            let back = c.beat_at_sample(c.sample_at_beat(beat));
            assert!((back - beat).abs() < 1e-9, "{beat} -> {back}");
        }
    }

    #[test]
    fn tempo_change_is_continuous() {
        let mut c = InternalClock::new(48_000.0, 120.0);
        let now = 30_000.0; // beat 1.25
        let before = c.beat_at_sample(now);
        c.set_tempo(60.0, now);
        let after = c.beat_at_sample(now);
        assert!((before - after).abs() < 1e-12);
        // One more beat now takes 48 000 samples.
        assert!((c.sample_at_beat(2.25) - (now + 48_000.0)).abs() < 1e-9);
    }

    #[test]
    fn reset_moves_beat_zero() {
        let mut c = InternalClock::new(48_000.0, 120.0);
        c.reset(1_000.0);
        assert_eq!(c.sample_at_beat(0.0), 1_000.0);
        assert_eq!(c.beat_at_sample(1_000.0), 0.0);
    }

    #[test]
    fn tempo_is_clamped() {
        let c = InternalClock::new(48_000.0, 1_000.0);
        assert_eq!(c.tempo_bpm(), InternalClock::MAX_BPM);
    }

    #[test]
    fn controls_shift_the_grid() {
        let c = InternalClock::new(48_000.0, 120.0);
        let adjusted = AdjustedClock::new(
            &c,
            ClockControls {
                nudge_beats: 0.5,
                latency_ms: 10.0,
            },
        );
        // Beat 0 lands at source beat 0.5 (12 000 samples) minus 10 ms (480).
        assert_eq!(adjusted.sample_at_beat(0.0), 11_520.0);
        assert!((adjusted.beat_at_sample(11_520.0)).abs() < 1e-12);
        assert_eq!(adjusted.tempo_bpm(), 120.0);
    }
}
