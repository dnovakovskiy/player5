//! The engine: one [`Control`] half for the control thread, one [`Renderer`]
//! half for the audio thread, connected by the lock-free event queue and a
//! [`SharedTiming`] the renderer publishes.
//!
//! Platform shells with a real audio thread (macOS, iOS) call [`split`] and
//! move the [`Renderer`] into their audio callback. [`Engine`] keeps both
//! halves together and steps them in lockstep, for offline rendering, tests,
//! the CLI and the browser's single-threaded AudioWorklet.
//!
//! [`spec`] holds the JSON pattern format shared by the CLI, the FFI and the
//! web app's URL-hash encoding.

#![forbid(unsafe_code)]

mod control;
mod renderer;
pub mod spec;
pub mod timing;

use std::sync::Arc;

pub use control::{dsp_param, ClockMode, Control, DEFAULT_LOOKAHEAD_SAMPLES};
pub use renderer::Renderer;
pub use spec::PatternSpec;
pub use timing::{SharedTiming, TimingSnapshot};

use sequencer::queue::event_queue;
use sequencer::{Pattern, VoiceId, VoiceParam};
use sync::{MidiMessage, Observation};

/// Event-queue capacity used by [`split`] and [`Engine::new`].
pub const EVENT_QUEUE_CAPACITY: usize = 1_024;

/// Creates a connected control/render pair for `sample_rate`. Allocates;
/// call at setup time.
#[must_use]
pub fn split(sample_rate: f32) -> (Control, Renderer) {
    let (producer, consumer) = event_queue(EVENT_QUEUE_CAPACITY);
    let timing = Arc::new(SharedTiming::default());
    (
        Control::new(sample_rate, producer, Arc::clone(&timing)),
        Renderer::new(sample_rate, consumer, timing),
    )
}

/// Both halves in one place, stepped in lockstep.
pub struct Engine {
    control: Control,
    renderer: Renderer,
}

impl Engine {
    /// A stopped engine at `sample_rate` with an empty pattern.
    #[must_use]
    pub fn new(sample_rate: f32) -> Self {
        let (control, renderer) = split(sample_rate);
        Self { control, renderer }
    }

    /// Control half.
    pub fn control(&mut self) -> &mut Control {
        &mut self.control
    }

    /// Control half, read-only.
    #[must_use]
    pub fn control_ref(&self) -> &Control {
        &self.control
    }

    /// Render half.
    pub fn renderer(&mut self) -> &mut Renderer {
        &mut self.renderer
    }

    /// Current render position in samples.
    #[must_use]
    pub fn position(&self) -> u64 {
        self.renderer.position()
    }

    /// Whether the scheduler is running.
    #[must_use]
    pub fn is_playing(&self) -> bool {
        self.control.is_playing()
    }

    /// Replaces the pattern.
    pub fn set_pattern(&mut self, pattern: Pattern) {
        self.control.set_pattern(pattern);
    }

    /// Sets the internal clock's tempo at the current position.
    pub fn set_tempo(&mut self, bpm: f64) {
        let now = self.renderer.position();
        self.control.set_tempo(bpm, now);
    }

    /// Tempo of the active clock.
    #[must_use]
    pub fn tempo(&self) -> f64 {
        self.control.tempo()
    }

    /// Sends a voice parameter change, applied at the current position.
    pub fn set_voice_param(&mut self, voice: VoiceId, param: VoiceParam, value: f32) {
        let now = self.renderer.position();
        self.control.set_voice_param(voice, param, value, now);
    }

    /// Sends a kick parameter change, applied at the current position.
    pub fn set_kick_param(&mut self, param: VoiceParam, value: f32) {
        self.set_voice_param(VoiceId::Kick, param, value);
    }

    /// Sets master output gain, applied at the current position.
    pub fn set_output_gain(&mut self, gain: f32) {
        let now = self.renderer.position();
        self.control.set_output_gain(gain, now);
    }

    /// Enables the soft safety limiter, applied at the current position.
    pub fn set_limiter(&mut self, enabled: bool) {
        let now = self.renderer.position();
        self.control.set_limiter(enabled, now);
    }

    /// Applies everything in a pattern file except render settings that
    /// only matter offline: tempo, pattern, every voice's controls, master.
    pub fn load_spec(&mut self, spec: &PatternSpec) -> Result<(), spec::SpecError> {
        let pattern = spec.pattern()?;
        let now = self.renderer.position();
        self.control.set_tempo(spec.bpm, now);
        self.control.set_pattern(pattern);
        for voice in VoiceId::ALL {
            let params = spec.voice_params(voice);
            self.control.set_voice_params(voice, &params, now);
        }
        self.control.set_output_gain(spec.render.output_gain, now);
        self.control.set_limiter(spec.render.limiter, now);
        Ok(())
    }

    /// Switches clock mode at the current position.
    pub fn set_clock_mode(&mut self, mode: ClockMode) {
        let now = self.renderer.position();
        self.control.set_clock_mode(mode, now);
    }

    /// Feeds an external observation.
    pub fn observe(&mut self, observation: &Observation) {
        let now = self.renderer.position();
        self.control.observe(observation, now);
    }

    /// Feeds a MIDI clock message received at `sample`.
    pub fn midi(&mut self, message: MidiMessage, sample: f64) {
        let now = self.renderer.position();
        self.control.midi(message, sample, now);
    }

    /// Registers a tap at `sample`.
    pub fn tap(&mut self, sample: f64) {
        let now = self.renderer.position();
        self.control.tap(sample, now);
    }

    /// Quantized re-sync at the current position.
    pub fn resync(&mut self) {
        let now = self.renderer.position();
        self.control.resync(now);
    }

    /// Phase nudge in milliseconds.
    pub fn set_nudge_ms(&mut self, ms: f64) {
        self.control.set_nudge_ms(ms);
    }

    /// Output latency compensation in milliseconds.
    pub fn set_latency_ms(&mut self, ms: f64) {
        self.control.set_latency_ms(ms);
    }

    /// Beat (global controls applied) at the current position.
    #[must_use]
    pub fn beat(&self) -> f64 {
        self.control.beat_at(self.renderer.position())
    }

    /// Whether the active clock is tracking its source.
    #[must_use]
    pub fn is_locked(&self) -> bool {
        self.control.is_locked()
    }

    /// Pattern step (`0..16`) audible at the current render position, or
    /// `None` when stopped. For playhead displays.
    #[must_use]
    pub fn playing_step(&self) -> Option<usize> {
        if !self.control.is_playing() {
            return None;
        }
        let beat = self.control.beat_at(self.renderer.position());
        if beat < 0.0 {
            return None;
        }
        let step = (beat / sequencer::BEATS_PER_STEP).floor() as u64;
        Some((step % sequencer::STEP_COUNT as u64) as usize)
    }

    /// Stops automatically after `steps` steps, counted from the first step
    /// of the next start (set while playing: from the next step); `None`
    /// loops forever. See [`Control::set_stop_after`].
    pub fn set_stop_after(&mut self, steps: Option<u64>) {
        self.control.set_stop_after(steps);
    }

    /// Starts playback at the current position.
    pub fn start(&mut self) {
        let now = self.renderer.position();
        self.control.start(now);
    }

    /// Stops scheduling new steps.
    pub fn stop(&mut self) {
        self.control.stop();
    }

    /// Renders one block of mono audio, ticking the control half first
    /// exactly as the control thread would.
    pub fn render(&mut self, out: &mut [f32]) {
        let now = self.renderer.position();
        self.control.tick(now);
        self.renderer.process(out);
    }

    /// Renders `frames` samples in blocks of `block_size`. Offline helper;
    /// allocates the output buffer.
    #[must_use]
    pub fn render_frames(&mut self, frames: usize, block_size: usize) -> Vec<f32> {
        let block_size = block_size.max(1);
        let mut out = vec![0.0f32; frames];
        for chunk in out.chunks_mut(block_size) {
            self.render(chunk);
        }
        out
    }
}
