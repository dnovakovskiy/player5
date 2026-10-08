//! Hand clap: band-passed noise with a burst of 3–4 fast re-triggered envelopes (the "flutter") followed by a longer diffuse tail. Controls: `tone` (band centre), `decay` (tail), `level`.
//!
//! STUB: produces silence until the voice is implemented. The public API
//! (type name, `new`, the `Voice` impl) is fixed; internals are free.

use crate::voice::Voice;
use crate::VoiceParams;

/// See the [module docs](self).
#[derive(Clone, Debug)]
pub struct Clap {
    sample_rate: f32,
    params: VoiceParams,
    active: bool,
}

impl Clap {
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

impl Voice for Clap {
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
