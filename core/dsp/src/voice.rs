//! The interface every synthesized voice implements.

use crate::VoiceParams;

/// A monophonic drum voice.
///
/// All methods run on the render thread and must be real-time safe: no
/// allocation, locks, I/O or logging, and only [`crate::math`] for
/// transcendental functions. Every implementation follows the same rules,
/// which `tests/voice_contract.rs` checks for all ten voices:
///
/// * **Determinism.** Output depends only on the sample rate and the calls
///   made since construction (or since the last
///   [`Voice::set_sample_rate`]). No randomness from outside the voice.
/// * **Bounded, finite output.** Any input, including NaN or out-of-range
///   controls and velocities, yields finite samples within `[-1, 1]`.
/// * **No subnormals.** Decaying states are flushed to zero long before
///   they could go subnormal.
pub trait Voice {
    /// (Re)configures internal coefficients for a new sample rate and puts
    /// the voice back in its freshly constructed, idle state (keeping its
    /// controls). Called before any audio is processed at that rate.
    fn set_sample_rate(&mut self, sample_rate: f32);

    /// Applies the TR-style controls (each clamped to `0..=1`; non-finite
    /// values are treated as 0). Voices read the fields they support.
    ///
    /// Pitch, decay, tone and snappy take effect on the next trigger.
    /// `level` applies immediately: an idle voice starts its next hit at the
    /// new level exactly, a sounding one glides to it within a few
    /// milliseconds so a fader move never clicks.
    fn apply_params(&mut self, params: &VoiceParams);

    /// Starts a hit. `velocity` is `0..=1` (larger values count as `1.0`, a
    /// full accent). A velocity of 0 (or NaN) is ignored: it neither starts
    /// a hit nor cuts one that is sounding. How a retrigger lands on a
    /// sounding voice is voice-specific (see each module).
    fn trigger(&mut self, velocity: f32);

    /// Renders one sample.
    fn process(&mut self) -> f32;

    /// Whether the voice is still producing sound. Idle voices must return
    /// exactly `0.0` from [`Voice::process`].
    fn is_active(&self) -> bool;

    /// Silences the voice quickly without a click (a few milliseconds of
    /// fade). Used for the closed hat choking the open hat. Default: no-op.
    fn choke(&mut self) {}
}
