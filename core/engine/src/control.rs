use sequencer::queue::Producer;
use sequencer::{Event, MasterParam, ParamTarget, Pattern, Scheduler, VoiceId, VoiceParam};
use sync::{AdjustedClock, ClockControls, ClockSource, InternalClock};

/// Default lookahead: 100 ms at 48 kHz. See ADR-0001.
pub const DEFAULT_LOOKAHEAD_SAMPLES: u64 = 4_800;

/// Control-thread half of the engine. Owns the clock, the scheduler and the
/// producer end of the event queue.
///
/// Every mutation that must be audible goes through the queue as an event
/// stamped with a sample position; nothing here shares memory with the
/// renderer.
pub struct Control {
    sample_rate: f32,
    clock: InternalClock,
    controls: ClockControls,
    scheduler: Scheduler,
    producer: Producer,
    lookahead: u64,
}

impl Control {
    /// Default tempo.
    pub const DEFAULT_BPM: f64 = 120.0;

    /// Creates the control half over `producer`.
    #[must_use]
    pub fn new(sample_rate: f32, producer: Producer) -> Self {
        Self {
            sample_rate,
            clock: InternalClock::new(f64::from(sample_rate), Self::DEFAULT_BPM),
            controls: ClockControls::default(),
            scheduler: Scheduler::default(),
            producer,
            lookahead: (DEFAULT_LOOKAHEAD_SAMPLES as f64 * f64::from(sample_rate) / 48_000.0)
                .round() as u64,
        }
    }

    /// Sample rate.
    #[must_use]
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// The internal clock.
    #[must_use]
    pub fn clock(&self) -> &InternalClock {
        &self.clock
    }

    /// Global timing controls (nudge, latency offset).
    #[must_use]
    pub fn clock_controls(&self) -> ClockControls {
        self.controls
    }

    /// Replaces the global timing controls.
    pub fn set_clock_controls(&mut self, controls: ClockControls) {
        self.controls = controls;
    }

    /// Lookahead in samples.
    #[must_use]
    pub fn lookahead(&self) -> u64 {
        self.lookahead
    }

    /// Sets the lookahead in samples.
    pub fn set_lookahead(&mut self, samples: u64) {
        self.lookahead = samples;
    }

    /// The scheduler (read-only; mutate through this type's methods).
    #[must_use]
    pub fn scheduler(&self) -> &Scheduler {
        &self.scheduler
    }

    /// Replaces the pattern; applies from the next unscheduled step.
    pub fn set_pattern(&mut self, pattern: Pattern) {
        self.scheduler.set_pattern(pattern);
    }

    /// Current pattern.
    #[must_use]
    pub fn pattern(&self) -> &Pattern {
        self.scheduler.pattern()
    }

    /// Changes tempo, keeping the beat continuous at `now`.
    pub fn set_tempo(&mut self, bpm: f64, now: u64) {
        self.clock.set_tempo(bpm, now as f64);
    }

    /// Current tempo.
    #[must_use]
    pub fn tempo(&self) -> f64 {
        self.clock.tempo_bpm()
    }

    /// The beat (from beat 0 at start) that falls on `sample`, with the
    /// global controls applied. Negative before playback started.
    #[must_use]
    pub fn beat_at(&self, sample: u64) -> f64 {
        AdjustedClock::new(&self.clock, self.controls).beat_at_sample(sample as f64)
    }

    /// Starts playback: beat 0 and step 0 fall on `now`.
    pub fn start(&mut self, now: u64) {
        self.clock.reset(now as f64);
        self.scheduler.start();
    }

    /// Stops scheduling. Queued events still play.
    pub fn stop(&mut self) {
        self.scheduler.stop();
    }

    /// Stops automatically after `steps` steps (`None` loops forever).
    pub fn set_stop_after(&mut self, steps: Option<u64>) {
        self.scheduler.set_stop_after(steps);
    }

    /// Whether playback is running.
    #[must_use]
    pub fn is_playing(&self) -> bool {
        self.scheduler.is_playing()
    }

    /// Queues a voice parameter change for sample `at`. Returns `false` if
    /// the queue was full (the change is dropped; retry later).
    pub fn set_voice_param(
        &mut self,
        voice: VoiceId,
        param: VoiceParam,
        value: f32,
        at: u64,
    ) -> bool {
        self.producer
            .push(Event::param(at, ParamTarget::Voice(voice, param), value))
            .is_ok()
    }

    /// Queues an output-gain change for sample `at`.
    pub fn set_output_gain(&mut self, gain: f32, at: u64) -> bool {
        self.producer
            .push(Event::param(
                at,
                ParamTarget::Master(MasterParam::OutputGain),
                gain,
            ))
            .is_ok()
    }

    /// Queues a limiter on/off change for sample `at`.
    pub fn set_limiter(&mut self, enabled: bool, at: u64) -> bool {
        let value = if enabled { 1.0 } else { 0.0 };
        self.producer
            .push(Event::param(
                at,
                ParamTarget::Master(MasterParam::Limiter),
                value,
            ))
            .is_ok()
    }

    /// Schedules every step up to `now + lookahead`. Call once per control
    /// tick (or before every block when driving offline). Returns the number
    /// of events pushed.
    pub fn schedule_ahead(&mut self, now: u64) -> usize {
        let clock = AdjustedClock::new(&self.clock, self.controls);
        self.scheduler
            .schedule(&clock, now + self.lookahead, &mut self.producer)
    }
}
