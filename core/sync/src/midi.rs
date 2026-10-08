//! MIDI clock input (24 pulses per quarter note), from CoreMIDI on Apple
//! platforms or Web MIDI in Chromium. Tempo only, individually jittery:
//! the follower smooths pulse intervals; phase comes from Start/Continue
//! and the pulse count. See `docs/protocols/midi-clock.md`.
//!
//! PLACEHOLDER IMPLEMENTATION: averages the last 24 pulse intervals. The
//! public API is fixed.

use crate::follower::{Observation, Phase};

/// Pulses per quarter note.
pub const PPQN: u32 = 24;

/// The MIDI System Real-Time messages that matter for clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MidiMessage {
    /// Timing Clock (0xF8).
    Clock,
    /// Start (0xFA): the next Clock is the first pulse of beat 0.
    Start,
    /// Continue (0xFB): resume counting from the stopped position.
    Continue,
    /// Stop (0xFC).
    Stop,
}

impl MidiMessage {
    /// Parses a status byte; `None` for anything that is not clock-related.
    #[must_use]
    pub const fn from_status(status: u8) -> Option<Self> {
        match status {
            0xF8 => Some(Self::Clock),
            0xFA => Some(Self::Start),
            0xFB => Some(Self::Continue),
            0xFC => Some(Self::Stop),
            _ => None,
        }
    }
}

/// Turns timestamped MIDI clock messages into [`Observation`]s.
#[derive(Clone, Debug)]
pub struct MidiClockFollower {
    sample_rate: f64,
    running: bool,
    pulses: u64,
    last_pulse: Option<f64>,
    intervals: [f64; PPQN as usize],
    filled: usize,
    next: usize,
}

impl MidiClockFollower {
    /// A stopped follower.
    #[must_use]
    pub fn new(sample_rate: f64) -> Self {
        Self {
            sample_rate,
            running: false,
            pulses: 0,
            last_pulse: None,
            intervals: [0.0; PPQN as usize],
            filled: 0,
            next: 0,
        }
    }

    /// Whether Start/Continue was seen without a later Stop.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Handles one message received at `sample` (consumer's sample clock).
    /// Returns an observation when there is something to report.
    pub fn handle(&mut self, message: MidiMessage, sample: f64) -> Option<Observation> {
        match message {
            MidiMessage::Start => {
                self.running = true;
                self.pulses = 0;
                self.last_pulse = None;
                None
            }
            MidiMessage::Continue => {
                self.running = true;
                self.last_pulse = None;
                None
            }
            MidiMessage::Stop => {
                self.running = false;
                None
            }
            MidiMessage::Clock => {
                if let Some(prev) = self.last_pulse {
                    let interval = sample - prev;
                    if interval > 0.0 {
                        self.intervals[self.next] = interval;
                        self.next = (self.next + 1) % self.intervals.len();
                        self.filled = (self.filled + 1).min(self.intervals.len());
                    }
                }
                self.last_pulse = Some(sample);
                let pulse = self.pulses;
                self.pulses += 1;
                if self.filled == 0 {
                    return None;
                }
                let mean = self.intervals[..self.filled].iter().sum::<f64>() / self.filled as f64;
                let bpm = 60.0 * self.sample_rate / (mean * f64::from(PPQN));
                let phase = if self.running {
                    let beat = pulse as f64 / f64::from(PPQN);
                    Phase::Bar(beat.rem_euclid(4.0))
                } else {
                    Phase::TempoOnly
                };
                Some(Observation {
                    sample,
                    phase,
                    bpm: Some(bpm),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steady_clock_reports_tempo() {
        let sr = 48_000.0;
        let mut m = MidiClockFollower::new(sr);
        // 120 BPM: 24 pulses per 24 000 samples = 1 000 samples per pulse.
        m.handle(MidiMessage::Start, 0.0);
        let mut last = None;
        for k in 0..96 {
            last = m.handle(MidiMessage::Clock, k as f64 * 1_000.0);
        }
        let obs = last.unwrap();
        assert!((obs.bpm.unwrap() - 120.0).abs() < 1e-6);
        assert_eq!(obs.phase, Phase::Bar(95.0 / 24.0));
    }

    #[test]
    fn status_bytes() {
        assert_eq!(MidiMessage::from_status(0xF8), Some(MidiMessage::Clock));
        assert_eq!(MidiMessage::from_status(0x90), None);
    }
}
