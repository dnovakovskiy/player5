//! Tap tempo: taps become a tempo and a beat phase.
//!
//! Human taps are off by tens of milliseconds and sometimes plainly wrong
//! (a double tap, a missed beat), so the tempo is the median of the recent
//! intervals, which ignores a single bad one outright. A tap much closer to
//! the previous one than the current tempo allows is a bounce and is
//! dropped. Two intervals in a row that disagree with the median mean the
//! tapper changed tempo: the sequence restarts from them. A pause longer
//! than [`TAP_TIMEOUT_S`] starts over.
//!
//! The reported beat position is not the raw last tap but the least-squares
//! phase of all taps in the sequence (each tap assigned to its nearest beat
//! at the median tempo), so one early or late tap does not drag the grid.

use crate::follower::{FollowerClock, Observation, Phase};

/// Taps further apart than this (seconds) start a new tap sequence.
pub const TAP_TIMEOUT_S: f64 = 2.0;

/// Taps remembered per sequence.
const TAPS: usize = 8;

/// An interval this far (as a fraction) from the median is an outlier.
const OUTLIER_FRACTION: f64 = 0.25;

/// An interval shorter than this fraction of the median is a bounce (well
/// below 0.5, so tapping at double tempo reads as a tempo change).
const BOUNCE_FRACTION: f64 = 0.35;

/// Collects taps and reports tempo plus "a beat is here".
#[derive(Clone, Debug)]
pub struct TapTempo {
    sample_rate: f64,
    /// Taps of the current sequence, oldest first.
    taps: [f64; TAPS],
    count: usize,
    /// Consecutive outlier intervals seen.
    outliers: u32,
}

impl TapTempo {
    /// No taps yet.
    #[must_use]
    pub fn new(sample_rate: f64) -> Self {
        Self {
            sample_rate,
            taps: [0.0; TAPS],
            count: 0,
            outliers: 0,
        }
    }

    /// Records a tap at `sample`. From the second tap on, returns an
    /// observation: a beat falls at the reported sample, at the median
    /// tempo of the sequence.
    pub fn tap(&mut self, sample: f64) -> Option<Observation> {
        if !sample.is_finite() {
            return None;
        }
        if let Some(&last) = self.taps[..self.count].last() {
            let interval = sample - last;
            if interval > TAP_TIMEOUT_S * self.sample_rate || interval < 0.0 {
                self.reset();
            } else if interval == 0.0 {
                return None;
            } else if let Some(median) = self.median_interval() {
                if interval < BOUNCE_FRACTION * median {
                    // A double tap or switch bounce: not a beat.
                    return None;
                }
                if (interval / median - 1.0).abs() > OUTLIER_FRACTION {
                    self.outliers += 1;
                    if self.outliers >= 2 {
                        // Two disagreeing intervals in a row: a new tempo.
                        // Keep only the taps that define it.
                        let keep = [self.taps[self.count - 2], last];
                        self.taps[..2].copy_from_slice(&keep);
                        self.count = 2;
                        self.outliers = 0;
                    }
                } else {
                    self.outliers = 0;
                }
            }
        }
        self.push(sample);
        let interval = self.median_interval()?;
        let bpm = 60.0 * self.sample_rate / interval;
        if !(FollowerClock::MIN_BPM..=FollowerClock::MAX_BPM).contains(&bpm) {
            return None;
        }
        Some(Observation {
            sample: self.fitted_last_beat(interval),
            phase: Phase::Beat(0.0),
            bpm: Some(bpm),
        })
    }

    /// Forgets all taps.
    pub fn reset(&mut self) {
        self.count = 0;
        self.outliers = 0;
    }

    fn push(&mut self, sample: f64) {
        if self.count == TAPS {
            self.taps.copy_within(1.., 0);
            self.count -= 1;
        }
        self.taps[self.count] = sample;
        self.count += 1;
    }

    /// Median of the intervals between consecutive taps (allocation-free:
    /// at most seven values, sorted in a fixed array).
    fn median_interval(&self) -> Option<f64> {
        if self.count < 2 {
            return None;
        }
        let mut v = [0.0; TAPS - 1];
        let n = self.count - 1;
        for (i, slot) in v[..n].iter_mut().enumerate() {
            *slot = self.taps[i + 1] - self.taps[i];
        }
        v[..n].sort_unstable_by(f64::total_cmp);
        Some(if n % 2 == 1 {
            v[n / 2]
        } else {
            0.5 * (v[n / 2 - 1] + v[n / 2])
        })
    }

    /// Where the newest beat falls according to all taps: each tap is
    /// assigned to its nearest beat counting back from the newest at
    /// `interval`, and the beat phase is the mean of their offsets.
    fn fitted_last_beat(&self, interval: f64) -> f64 {
        let newest = self.taps[self.count - 1];
        let mut sum = 0.0;
        for &t in &self.taps[..self.count] {
            let beats_back = ((newest - t) / interval).round();
            sum += t + beats_back * interval;
        }
        sum / self.count as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 48_000.0;

    #[test]
    fn taps_give_tempo() {
        let mut t = TapTempo::new(SR);
        assert!(t.tap(0.0).is_none());
        let obs = t.tap(24_000.0).unwrap();
        assert!((obs.bpm.unwrap() - 120.0).abs() < 1e-9);
        assert_eq!(obs.phase, Phase::Beat(0.0));
        let obs = t.tap(48_000.0).unwrap();
        assert!((obs.bpm.unwrap() - 120.0).abs() < 1e-9);
        assert!((obs.sample - 48_000.0).abs() < 1e-9);
        // A long pause starts over.
        assert!(t.tap(48_000.0 * 10.0).is_none());
    }

    #[test]
    fn one_bad_tap_does_not_move_the_tempo() {
        let mut t = TapTempo::new(SR);
        for k in 0..5 {
            t.tap(f64::from(k) * 24_000.0);
        }
        // 40 ms late, then back on the grid.
        let late = t.tap(5.0 * 24_000.0 + 1_920.0).unwrap();
        assert!((late.bpm.unwrap() - 120.0).abs() < 1e-9, "{late:?}");
        let back = t.tap(6.0 * 24_000.0).unwrap();
        assert!((back.bpm.unwrap() - 120.0).abs() < 1e-9);
        // The fitted beat barely moves for the late tap.
        assert!((late.sample - 5.0 * 24_000.0).abs() < 1_920.0 / 4.0);
    }

    #[test]
    fn bounces_are_ignored_and_a_missed_beat_is_tolerated() {
        let mut t = TapTempo::new(SR);
        for k in 0..4 {
            t.tap(f64::from(k) * 24_000.0);
        }
        // A bounce 30 ms after a tap.
        assert!(t.tap(3.0 * 24_000.0 + 1_440.0).is_none());
        // A missed beat (the tapper skipped one): tempo unchanged.
        let obs = t.tap(5.0 * 24_000.0).unwrap();
        assert!((obs.bpm.unwrap() - 120.0).abs() < 1e-9);
        assert!((obs.sample - 5.0 * 24_000.0).abs() < 1e-6);
    }

    #[test]
    fn a_new_tempo_takes_over_after_two_taps() {
        let mut t = TapTempo::new(SR);
        let mut at = 0.0;
        for _ in 0..6 {
            t.tap(at);
            at += 24_000.0; // 120 BPM
        }
        // Now tapping at 90 BPM (32 000 samples).
        at += 8_000.0;
        let first = t.tap(at).unwrap();
        assert!((first.bpm.unwrap() - 120.0).abs() < 1e-9, "one tap is an outlier");
        at += 32_000.0;
        let second = t.tap(at).unwrap();
        assert!((second.bpm.unwrap() - 90.0).abs() < 1e-9, "{second:?}");
    }
}
