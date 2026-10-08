//! Toms: a pitched, decaying sine body with a downward pitch bend, a little
//! filtered noise on the attack and soft saturation; one type, three ranges.
//! Controls: `tune` (pitch within the range), `decay`, `level`.
//!
//! STUB: produces silence until the voice is implemented. The public API
//! (type names, `new`, the `Voice` impl) is fixed; internals are free.

use crate::voice::Voice;
use crate::VoiceParams;

/// Which tom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TomRange {
    /// Floor tom (lowest).
    Low,
    /// Middle tom.
    Mid,
    /// High tom.
    High,
}

/// See the [module docs](self).
#[derive(Clone, Debug)]
pub struct Tom {
    sample_rate: f32,
    range: TomRange,
    params: VoiceParams,
    active: bool,
}

impl Tom {
    /// Creates an idle tom of the given range.
    #[must_use]
    pub fn new(sample_rate: f32, range: TomRange) -> Self {
        Self {
            sample_rate,
            range,
            params: VoiceParams::default(),
            active: false,
        }
    }

    /// The tom's range.
    #[must_use]
    pub fn range(&self) -> TomRange {
        self.range
    }
}

impl Voice for Tom {
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
