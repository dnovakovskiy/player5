//! Tap tempo: taps become a tempo and a beat phase.
//!
//! Human taps are off by tens of milliseconds and sometimes plainly wrong
//! (a double tap, a missed beat), so the tempo is the median of the recent
//! intervals, which ignores a single bad one outright. A tap much closer to
//! the previous one than the current tempo allows is a bounce and is
//! dropped. Two intervals in a row that disagree with the median but agree
//! with each other mean the tapper changed tempo: the sequence restarts
//! from them. Two that disagree with each other too (one long, one short)
//! mean the tap between them was displaced: it is dropped. A pause longer
//! than [`TAP_TIMEOUT_S`] starts over.
//!
//! The reported beat position is not the raw last tap but the median phase
//! of all taps in the sequence (each tap's offset from its nearest beat at
//! the median tempo), so one early or late tap, even the newest, does not
//! drag the grid.

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
                        let previous = last - self.taps[self.count - 2];
                        if (interval / previous - 1.0).abs() <= OUTLIER_FRACTION {
                            // Two like intervals off the median: a new
                            // tempo. Keep only the taps that define it.
                            let keep = [self.taps[self.count - 2], last];
                            self.taps[..2].copy_from_slice(&keep);
                            self.count = 2;
                        } else {
                            // One long, one short: the tap between them was
                            // early or late. Drop it.
                            self.count -= 1;
                        }
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

    /// Where the beat nearest the newest tap falls according to all taps:
    /// each tap's offset from the beat grid through the newest tap at
    /// `interval` (wrapped to ± half a beat), and the median of those
    /// offsets moves the grid.
    fn fitted_last_beat(&self, interval: f64) -> f64 {
        let newest = self.taps[self.count - 1];
        let mut v = [0.0; TAPS];
        let n = self.count;
        for (slot, &t) in v[..n].iter_mut().zip(&self.taps[..n]) {
            let beats = (t - newest) / interval;
            *slot = (beats - beats.round()) * interval;
        }
        v[..n].sort_unstable_by(f64::total_cmp);
        let median = if n % 2 == 1 {
            v[n / 2]
        } else {
            0.5 * (v[n / 2 - 1] + v[n / 2])
        };
        newest + median
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

    /// A tap a third of a beat late, then back on the grid: the tempo
    /// holds, the sequence is not restarted around the bad tap, and the grid
    /// stays within a few milliseconds.
    #[test]
    fn a_displaced_tap_is_dropped_not_followed() {
        for late in [0.3, 0.4, -0.3] {
            let mut t = TapTempo::new(SR);
            for k in 0..6 {
                t.tap(f64::from(k) * 24_000.0);
            }
            let bad = t.tap(6.0 * 24_000.0 + late * 24_000.0).unwrap();
            assert!((bad.bpm.unwrap() - 120.0).abs() < 1e-9, "{late}");
            // Even as the newest tap, the bad one does not move the grid.
            let off = (bad.sample / 24_000.0 - (bad.sample / 24_000.0).round()) * 500.0;
            assert!(off.abs() < 1.0, "{late}: {off} ms");
            for k in 7..10 {
                let obs = t.tap(f64::from(k) * 24_000.0).unwrap();
                assert!((obs.bpm.unwrap() - 120.0).abs() < 1e-9, "{late}");
                let err_ms = (obs.sample - f64::from(k) * 24_000.0) / 48.0;
                assert!(err_ms.abs() < 1.0, "{late}, tap {k}: {err_ms} ms");
            }
            assert!(t.count >= 5, "{late}: sequence kept ({} taps)", t.count);
        }
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
        assert!(
            (first.bpm.unwrap() - 120.0).abs() < 1e-9,
            "one tap is an outlier"
        );
        at += 32_000.0;
        let second = t.tap(at).unwrap();
        assert!((second.bpm.unwrap() - 90.0).abs() < 1e-9, "{second:?}");
    }
}
