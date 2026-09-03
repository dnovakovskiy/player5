/// A tempo source: something that can say where beats fall on the audio
/// sample clock.
///
/// Implementations must be consistent: `beat_at_sample(sample_at_beat(b))`
/// should return `b` (within floating-point error) for any `b`.
pub trait ClockSource {
    /// Audio sample rate this clock's sample positions are expressed in.
    fn sample_rate(&self) -> f64;

    /// Current tempo estimate in beats per minute.
    fn tempo_bpm(&self) -> f64;

    /// The (fractional) beat that falls on the given sample position.
    fn beat_at_sample(&self, sample: f64) -> f64;

    /// The (fractional) sample position at which the given beat falls.
    fn sample_at_beat(&self, beat: f64) -> f64;

    /// Samples per beat at the current tempo.
    fn samples_per_beat(&self) -> f64 {
        self.sample_rate() * 60.0 / self.tempo_bpm()
    }
}

/// Global timing controls that apply regardless of the clock source.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ClockControls {
    /// Shifts this device's grid relative to the source. Positive values make
    /// our beats land later. Applied in beats so it survives tempo changes.
    pub nudge_beats: f64,
    /// Compensates the audio path: the time (ms) between the render callback
    /// producing a sample and that sample leaving the mixer's channel.
    /// Positive values make us trigger earlier so the sound lands on the
    /// source's beat.
    pub latency_ms: f64,
}

/// A [`ClockSource`] wrapped with [`ClockControls`]. Borrow-based so the
/// underlying source keeps its identity and can be updated elsewhere.
#[derive(Clone, Copy, Debug)]
pub struct AdjustedClock<'a, C: ClockSource> {
    inner: &'a C,
    controls: ClockControls,
}

impl<'a, C: ClockSource> AdjustedClock<'a, C> {
    /// Wraps `inner` with `controls`.
    pub fn new(inner: &'a C, controls: ClockControls) -> Self {
        Self { inner, controls }
    }

    fn latency_samples(&self) -> f64 {
        self.controls.latency_ms * self.inner.sample_rate() / 1_000.0
    }
}

impl<C: ClockSource> ClockSource for AdjustedClock<'_, C> {
    fn sample_rate(&self) -> f64 {
        self.inner.sample_rate()
    }

    fn tempo_bpm(&self) -> f64 {
        self.inner.tempo_bpm()
    }

    fn beat_at_sample(&self, sample: f64) -> f64 {
        self.inner.beat_at_sample(sample + self.latency_samples()) - self.controls.nudge_beats
    }

    fn sample_at_beat(&self, beat: f64) -> f64 {
        self.inner.sample_at_beat(beat + self.controls.nudge_beats) - self.latency_samples()
    }
}
