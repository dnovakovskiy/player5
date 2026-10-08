//! Hi-hats: six detuned square oscillators (the classic inharmonic "metal"
//! cluster) summed, band-passed and high-passed, with a short (closed) or
//! long (open) envelope. A closed-hat trigger chokes a ringing open hat.
//! Controls: `decay`, `tone` (filter brightness), `level`.
//!
//! STUB: produces silence until the voices are implemented. The public API
//! (type names, `new`, the `Voice` impls, `OpenHat::choke`) is fixed;
//! internals are free.

use crate::voice::Voice;
use crate::VoiceParams;

/// Closed hi-hat. See the [module docs](self).
#[derive(Clone, Debug)]
pub struct ClosedHat {
    sample_rate: f32,
    params: VoiceParams,
    active: bool,
}

impl ClosedHat {
    /// Creates an idle voice for the given sample rate.
    #[must_use]
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            params: VoiceParams::default(),
            active: false,
        }
    }
}

impl Voice for ClosedHat {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.active = false;
    }

    fn apply_params(&mut self, params: &VoiceParams) {
        self.params = *params;
    }

    fn trigger(&mut self, _velocity: f32) {}

    fn process(&mut self) -> f32 {
        0.0
    }

    fn is_active(&self) -> bool {
        self.active
    }
}

/// Open hi-hat. See the [module docs](self).
#[derive(Clone, Debug)]
pub struct OpenHat {
    sample_rate: f32,
    params: VoiceParams,
    active: bool,
}

impl OpenHat {
    /// Creates an idle voice for the given sample rate.
    #[must_use]
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            params: VoiceParams::default(),
            active: false,
        }
    }
}

impl Voice for OpenHat {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.active = false;
    }

    fn apply_params(&mut self, params: &VoiceParams) {
        self.params = *params;
    }

    fn trigger(&mut self, _velocity: f32) {}

    fn process(&mut self) -> f32 {
        0.0
    }

    fn is_active(&self) -> bool {
        self.active
    }

    fn choke(&mut self) {}
}
