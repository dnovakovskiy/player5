//! The full kit: every voice, mixed to mono in a fixed order.
//!
//! Slots are indexed by `usize` so this crate stays independent of the
//! sequencer; the engine maps `sequencer::VoiceId::index()` onto [`slot`].
//! The summation order is fixed, which keeps renders bit-reproducible.

use crate::voice::Voice;
use crate::{
    Clap, ClosedHat, Cowbell, Kick, OpenHat, Param, Rim, Snare, Tom, TomRange, VoiceParams,
};

/// Number of voices in the kit.
pub const VOICE_COUNT: usize = 10;

/// Slot indices. These must match `sequencer::VoiceId` discriminants.
pub mod slot {
    /// Bass drum.
    pub const KICK: usize = 0;
    /// Snare drum.
    pub const SNARE: usize = 1;
    /// Low tom.
    pub const LOW_TOM: usize = 2;
    /// Mid tom.
    pub const MID_TOM: usize = 3;
    /// High tom.
    pub const HIGH_TOM: usize = 4;
    /// Rimshot.
    pub const RIM: usize = 5;
    /// Hand clap.
    pub const CLAP: usize = 6;
    /// Closed hi-hat.
    pub const CLOSED_HAT: usize = 7;
    /// Open hi-hat.
    pub const OPEN_HAT: usize = 8;
    /// Cowbell.
    pub const COWBELL: usize = 9;
}

/// Every voice plus its current controls.
#[derive(Clone, Debug)]
pub struct Kit {
    /// Bass drum.
    pub kick: Kick,
    /// Snare drum.
    pub snare: Snare,
    /// Low tom.
    pub low_tom: Tom,
    /// Mid tom.
    pub mid_tom: Tom,
    /// High tom.
    pub high_tom: Tom,
    /// Rimshot.
    pub rim: Rim,
    /// Hand clap.
    pub clap: Clap,
    /// Closed hi-hat.
    pub closed_hat: ClosedHat,
    /// Open hi-hat.
    pub open_hat: OpenHat,
    /// Cowbell.
    pub cowbell: Cowbell,
    params: [VoiceParams; VOICE_COUNT],
}

impl Kit {
    /// Every voice idle, default controls.
    #[must_use]
    pub fn new(sample_rate: f32) -> Self {
        let mut kit = Self {
            kick: Kick::new(sample_rate),
            snare: Snare::new(sample_rate),
            low_tom: Tom::new(sample_rate, TomRange::Low),
            mid_tom: Tom::new(sample_rate, TomRange::Mid),
            high_tom: Tom::new(sample_rate, TomRange::High),
            rim: Rim::new(sample_rate),
            clap: Clap::new(sample_rate),
            closed_hat: ClosedHat::new(sample_rate),
            open_hat: OpenHat::new(sample_rate),
            cowbell: Cowbell::new(sample_rate),
            params: [VoiceParams::default(); VOICE_COUNT],
        };
        for s in 0..VOICE_COUNT {
            let p = kit.params[s];
            kit.voice_mut(s).apply_params(&p);
        }
        kit
    }

    fn voice_mut(&mut self, slot: usize) -> &mut dyn Voice {
        match slot {
            slot::KICK => &mut self.kick,
            slot::SNARE => &mut self.snare,
            slot::LOW_TOM => &mut self.low_tom,
            slot::MID_TOM => &mut self.mid_tom,
            slot::HIGH_TOM => &mut self.high_tom,
            slot::RIM => &mut self.rim,
            slot::CLAP => &mut self.clap,
            slot::CLOSED_HAT => &mut self.closed_hat,
            slot::OPEN_HAT => &mut self.open_hat,
            _ => &mut self.cowbell,
        }
    }

    /// Reconfigures every voice for a new sample rate (silences them).
    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        for s in 0..VOICE_COUNT {
            self.voice_mut(s).set_sample_rate(sample_rate);
        }
    }

    /// Current controls for a slot.
    #[must_use]
    pub fn params(&self, slot: usize) -> VoiceParams {
        self.params.get(slot).copied().unwrap_or_default()
    }

    /// Sets one control on one slot. Out-of-range slots are ignored.
    pub fn set_param(&mut self, slot: usize, param: Param, value: f32) {
        if slot >= VOICE_COUNT {
            return;
        }
        self.params[slot].set(param, value);
        let p = self.params[slot];
        self.voice_mut(slot).apply_params(&p);
    }

    /// Starts a hit on a slot. A closed hat chokes the open hat.
    pub fn trigger(&mut self, slot: usize, velocity: f32) {
        if slot >= VOICE_COUNT {
            return;
        }
        if slot == slot::CLOSED_HAT {
            self.open_hat.choke();
        }
        self.voice_mut(slot).trigger(velocity);
    }

    /// Whether any voice is sounding.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.kick.is_active()
            || self.snare.is_active()
            || self.low_tom.is_active()
            || self.mid_tom.is_active()
            || self.high_tom.is_active()
            || self.rim.is_active()
            || self.clap.is_active()
            || self.closed_hat.is_active()
            || self.open_hat.is_active()
            || self.cowbell.is_active()
    }

    /// Renders one mono sample: the sum of every voice, in slot order.
    #[inline]
    pub fn process(&mut self) -> f32 {
        let mut sum = 0.0f32;
        sum += self.kick.process();
        sum += self.snare.process();
        sum += self.low_tom.process();
        sum += self.mid_tom.process();
        sum += self.high_tom.process();
        sum += self.rim.process();
        sum += self.clap.process();
        sum += self.closed_hat.process();
        sum += self.open_hat.process();
        sum += self.cowbell.process();
        sum
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_kit_is_silent() {
        let mut kit = Kit::new(48_000.0);
        assert!(!kit.is_active());
        for _ in 0..1_000 {
            assert_eq!(kit.process(), 0.0);
        }
    }

    #[test]
    fn kick_slot_matches_bare_kick() {
        // The mixer must not change a lone voice's samples at all.
        let mut kit = Kit::new(48_000.0);
        let mut kick = Kick::new(48_000.0);
        kit.trigger(slot::KICK, 1.0);
        kick.trigger(1.0);
        for _ in 0..24_000 {
            assert_eq!(kit.process().to_bits(), kick.process().to_bits());
        }
    }

    #[test]
    fn every_voice_is_bounded_and_goes_idle() {
        for slot in 0..VOICE_COUNT {
            let mut kit = Kit::new(48_000.0);
            for param in Param::ALL {
                kit.set_param(slot, param, 1.0);
            }
            kit.trigger(slot, 1.0);
            for _ in 0..(48_000 * 6) {
                let s = kit.process();
                assert!(s.is_finite() && s.abs() <= 1.0, "slot {slot}: {s}");
            }
            assert!(!kit.is_active(), "slot {slot} still active after 6 s");
        }
    }

    #[test]
    fn params_are_clamped_and_stored() {
        let mut kit = Kit::new(48_000.0);
        kit.set_param(slot::SNARE, Param::Snappy, 2.0);
        assert_eq!(kit.params(slot::SNARE).snappy, 1.0);
        kit.set_param(99, Param::Level, 0.0); // ignored
    }
}
