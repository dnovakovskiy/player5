use crate::VoiceId;

/// Something that happens at an exact sample position on the render thread.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Event {
    /// Absolute position on the audio sample clock.
    pub sample: u64,
    /// What happens.
    pub kind: EventKind,
}

/// The payload of an [`Event`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EventKind {
    /// Start a hit on a voice.
    Trigger {
        /// Which voice.
        voice: VoiceId,
        /// `0..=1`; `1.0` is a full accent.
        velocity: f32,
    },
    /// Change a parameter. Parameters travel through the same queue as
    /// triggers so the render thread never reads shared mutable state.
    Param {
        /// Which parameter.
        target: ParamTarget,
        /// New normalised value.
        value: f32,
    },
    /// Discard every *trigger* the renderer already holds that is stamped at
    /// or after this event's sample. Sent when the timeline jumps (a clock
    /// re-sync) so steps scheduled against the old timeline do not play
    /// twice. Applied as soon as it is received, not at its sample.
    Flush,
}

/// Addressable parameters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamTarget {
    /// A per-voice control.
    Voice(VoiceId, VoiceParam),
    /// A master-section control.
    Master(MasterParam),
}

/// TR-style per-voice controls. Not every voice has every control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceParam {
    /// Pitch.
    Tune,
    /// Ring-down time.
    Decay,
    /// Output level.
    Level,
    /// Noise amount (snare).
    Snappy,
    /// Timbre / filter position.
    Tone,
}

/// Master-section controls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MasterParam {
    /// Linear output gain.
    OutputGain,
    /// Soft safety limiter on/off (`value > 0.5` is on).
    Limiter,
}

impl Event {
    /// A trigger event.
    #[must_use]
    pub fn trigger(sample: u64, voice: VoiceId, velocity: f32) -> Self {
        Self {
            sample,
            kind: EventKind::Trigger { voice, velocity },
        }
    }

    /// A flush of pending triggers at or after `sample`.
    #[must_use]
    pub fn flush(sample: u64) -> Self {
        Self {
            sample,
            kind: EventKind::Flush,
        }
    }

    /// A parameter-change event.
    #[must_use]
    pub fn param(sample: u64, target: ParamTarget, value: f32) -> Self {
        Self {
            sample,
            kind: EventKind::Param { target, value },
        }
    }
}
