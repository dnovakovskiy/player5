//! Rimshot: two short resonant pings (a few hundred Hz and ~1.5–2 kHz) excited by a click, high-passed, very short. Controls: `tune`, `decay` (subtle), `level`.
//!
//! STUB: produces silence until the voice is implemented. The public API
//! (type name, `new`, the `Voice` impl) is fixed; internals are free.

use crate::voice::Voice;
use crate::VoiceParams;

/// See the [module docs](self).
#[derive(Clone, Debug)]
pub struct Rim {
    sample_rate: f32,
    params: VoiceParams,
    active: bool,
}

impl Rim {
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

impl Voice for Rim {
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
