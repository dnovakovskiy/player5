//! Cowbell: two detuned square oscillators (roughly a perfect-fifth-ish ratio, ~540 Hz and ~800 Hz at centre tune) through a band-pass, with a short attack peak and a longer tail. Controls: `tune`, `decay`, `tone`, `level`.
//!
//! STUB: produces silence until the voice is implemented. The public API
//! (type name, `new`, the `Voice` impl) is fixed; internals are free.

use crate::voice::Voice;
use crate::VoiceParams;

/// See the [module docs](self).
#[derive(Clone, Debug)]
pub struct Cowbell {
    sample_rate: f32,
    params: VoiceParams,
    active: bool,
}

impl Cowbell {
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

impl Voice for Cowbell {
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
