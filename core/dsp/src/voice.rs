//! The interface every synthesized voice implements.

use crate::VoiceParams;

/// A monophonic drum voice.
///
/// All methods run on the render thread and must be real-time safe.
pub trait Voice {
    /// (Re)configures internal coefficients for a new sample rate. Called
    /// from the render thread before any audio is processed at that rate.
    fn set_sample_rate(&mut self, sample_rate: f32);

    /// Applies the TR-style controls. Voices read the fields they support.
    /// Changes to pitch and envelope controls may take effect on the next
    /// trigger; `level` should apply immediately.
    fn apply_params(&mut self, params: &VoiceParams);

    /// Starts a hit. `velocity` is `0..=1`; `1.0` is a full accent.
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
