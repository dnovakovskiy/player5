//! Tap tempo: taps become a tempo and a beat phase.
//!
//! PLACEHOLDER IMPLEMENTATION: mean of the last intervals, reset after a
//! long pause. The public API is fixed.

use crate::follower::{Observation, Phase};

/// Taps further apart than this (seconds) start a new tap sequence.
pub const TAP_TIMEOUT_S: f64 = 2.0;

/// Collects taps and reports tempo plus "a beat is here".
#[derive(Clone, Debug)]
pub struct TapTempo {
    sample_rate: f64,
    taps: [f64; 8],
    count: usize,
}

impl TapTempo {
    /// No taps yet.
    #[must_use]
    pub fn new(sample_rate: f64) -> Self {
        Self {
            sample_rate,
            taps: [0.0; 8],
            count: 0,
        }
    }

    /// Records a tap at `sample`. From the second tap on, returns an
    /// observation: the tap is on a beat, at the averaged tempo.
    pub fn tap(&mut self, sample: f64) -> Option<Observation> {
        if self.count > 0 {
            let last = self.taps[(self.count - 1) % self.taps.len()];
            if sample - last > TAP_TIMEOUT_S * self.sample_rate || sample <= last {
                self.count = 0;
            }
        }
        self.taps[self.count % self.taps.len()] = sample;
        self.count += 1;
        let n = self.count.min(self.taps.len());
        if n < 2 {
            return None;
        }
        let newest = self.taps[(self.count - 1) % self.taps.len()];
        let oldest = self.taps[(self.count - n) % self.taps.len()];
        let interval = (newest - oldest) / (n - 1) as f64;
        Some(Observation {
            sample,
            phase: Phase::Beat(0.0),
            bpm: Some(60.0 * self.sample_rate / interval),
        })
    }

    /// Forgets all taps.
    pub fn reset(&mut self) {
        self.count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taps_give_tempo() {
        let mut t = TapTempo::new(48_000.0);
        assert!(t.tap(0.0).is_none());
        let obs = t.tap(24_000.0).unwrap();
        assert!((obs.bpm.unwrap() - 120.0).abs() < 1e-9);
        let obs = t.tap(48_000.0).unwrap();
        assert!((obs.bpm.unwrap() - 120.0).abs() < 1e-9);
        // A long pause starts over.
        assert!(t.tap(48_000.0 * 10.0).is_none());
    }
}
