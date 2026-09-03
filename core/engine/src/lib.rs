//! The engine: one [`Control`] half for the control thread, one [`Renderer`]
//! half for the audio thread, connected by the lock-free event queue.
//!
//! Platform shells call [`split`] and move the [`Renderer`] into their audio
//! callback. [`Engine`] keeps both halves together for offline rendering,
//! tests and the CLI harness, and drives the scheduler itself before every
//! block.
//!
//! [`spec`] holds the JSON pattern format shared by the CLI, the FFI and,
//! later, the web app's URL-hash encoding.

#![forbid(unsafe_code)]

mod control;
mod renderer;
pub mod spec;

pub use control::{Control, DEFAULT_LOOKAHEAD_SAMPLES};
pub use renderer::Renderer;
pub use spec::PatternSpec;

use sequencer::queue::event_queue;
use sequencer::{Pattern, VoiceParam};

/// Event-queue capacity used by [`split`] and [`Engine::new`].
pub const EVENT_QUEUE_CAPACITY: usize = 1_024;

/// Creates a connected control/render pair for `sample_rate`. Allocates;
/// call at setup time.
#[must_use]
pub fn split(sample_rate: f32) -> (Control, Renderer) {
    let (producer, consumer) = event_queue(EVENT_QUEUE_CAPACITY);
    (
        Control::new(sample_rate, producer),
        Renderer::new(sample_rate, consumer),
    )
}

/// Both halves in one place, stepped in lockstep. For offline use.
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

    /// Render half.
    pub fn renderer(&mut self) -> &mut Renderer {
        &mut self.renderer
    }

    /// Current render position in samples.
    #[must_use]
    pub fn position(&self) -> u64 {
        self.renderer.position()
    }

    /// Replaces the pattern.
    pub fn set_pattern(&mut self, pattern: Pattern) {
        self.control.set_pattern(pattern);
    }

    /// Sets the tempo at the current position.
    pub fn set_tempo(&mut self, bpm: f64) {
        let now = self.renderer.position();
        self.control.set_tempo(bpm, now);
    }

    /// Sends a kick parameter change, applied at the current position.
    pub fn set_kick_param(&mut self, param: VoiceParam, value: f32) {
        let now = self.renderer.position();
        self.control
            .set_voice_param(sequencer::VoiceId::Kick, param, value, now);
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

    /// Starts playback from step 0 at the current position.
    pub fn start(&mut self) {
        let now = self.renderer.position();
        self.control.start(now);
    }

    /// Stops scheduling new steps.
    pub fn stop(&mut self) {
        self.control.stop();
    }

    /// Renders one block of mono audio, scheduling ahead first exactly as
    /// the control thread would.
    pub fn render(&mut self, out: &mut [f32]) {
        let now = self.renderer.position();
        self.control.schedule_ahead(now);
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
