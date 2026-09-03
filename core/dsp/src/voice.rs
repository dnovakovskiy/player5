//! The interface every synthesized voice implements.

/// A monophonic drum voice.
///
/// All methods run on the render thread and must be real-time safe.
pub trait Voice {
    /// (Re)configures internal coefficients for a new sample rate. Called
    /// from the render thread before any audio is processed at that rate.
    fn set_sample_rate(&mut self, sample_rate: f32);

    /// Starts a hit. `velocity` is `0..=1`; `1.0` is a full accent.
    fn trigger(&mut self, velocity: f32);

    /// Renders one sample.
    fn process(&mut self) -> f32;

    /// Whether the voice is still producing sound. Idle voices are skipped
    /// by the mixer and must return exactly `0.0` from [`Voice::process`].
    fn is_active(&self) -> bool;
}
