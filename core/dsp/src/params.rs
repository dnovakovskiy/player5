//! The TR-style per-voice controls, shared by every voice.

/// Which control.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Param {
    /// Pitch.
    Tune,
    /// Ring-down time.
    Decay,
    /// Timbre: brightness / filter position (voice-specific).
    Tone,
    /// Noise ("snares") amount; meaningful on the snare.
    Snappy,
    /// Output level.
    Level,
}

impl Param {
    /// Every control, in a stable order.
    pub const ALL: [Param; 5] = [
        Param::Tune,
        Param::Decay,
        Param::Tone,
        Param::Snappy,
        Param::Level,
    ];
}

/// Normalised controls, every field `0..=1`. Each voice reads the ones it
/// supports and ignores the rest.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoiceParams {
    /// Pitch.
    pub tune: f32,
    /// Ring-down time.
    pub decay: f32,
    /// Timbre.
    pub tone: f32,
    /// Noise amount.
    pub snappy: f32,
    /// Linear output level.
    pub level: f32,
}

impl Default for VoiceParams {
    fn default() -> Self {
        Self {
            tune: 0.5,
            decay: 0.5,
            tone: 0.5,
            snappy: 0.5,
            level: 1.0,
        }
    }
}

impl VoiceParams {
    /// Reads one control.
    #[must_use]
    pub fn get(&self, param: Param) -> f32 {
        match param {
            Param::Tune => self.tune,
            Param::Decay => self.decay,
            Param::Tone => self.tone,
            Param::Snappy => self.snappy,
            Param::Level => self.level,
        }
    }

    /// Writes one control, clamped to `0..=1`.
    pub fn set(&mut self, param: Param, value: f32) {
        let value = if value.is_finite() {
            value.clamp(0.0, 1.0)
        } else {
            0.0
        };
        match param {
            Param::Tune => self.tune = value,
            Param::Decay => self.decay = value,
            Param::Tone => self.tone = value,
            Param::Snappy => self.snappy = value,
            Param::Level => self.level = value,
        }
    }
}
