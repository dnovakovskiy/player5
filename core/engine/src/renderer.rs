use dsp::{Kick, Master, Voice};
use sequencer::queue::Consumer;
use sequencer::{Event, EventKind, MasterParam, ParamTarget, VoiceId, VoiceParam};

/// How many events the renderer can hold back for later blocks.
const PENDING_CAPACITY: usize = 256;

/// Render-thread half of the engine. Owns the voices, the master section
/// and the consumer end of the event queue.
///
/// [`Renderer::process`] is the audio callback body and obeys the real-time
/// rules: it allocates nothing, takes no locks and does no I/O.
pub struct Renderer {
    sample_rate: f32,
    position: u64,
    consumer: Consumer,
    pending: [Event; PENDING_CAPACITY],
    pending_len: usize,
    kick: Kick,
    master: Master,
}

impl Renderer {
    /// Creates the render half over `consumer`.
    #[must_use]
    pub fn new(sample_rate: f32, consumer: Consumer) -> Self {
        Self {
            sample_rate,
            position: 0,
            consumer,
            pending: [Event::trigger(0, VoiceId::Kick, 0.0); PENDING_CAPACITY],
            pending_len: 0,
            kick: Kick::new(sample_rate),
            master: Master::default(),
        }
    }

    /// Sample rate.
    #[must_use]
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// Absolute position of the next sample to be rendered.
    #[must_use]
    pub fn position(&self) -> u64 {
        self.position
    }

    /// The kick voice.
    #[must_use]
    pub fn kick(&self) -> &Kick {
        &self.kick
    }

    /// The master section.
    #[must_use]
    pub fn master(&self) -> &Master {
        &self.master
    }

    /// Renders one block of mono audio. Real-time safe.
    pub fn process(&mut self, out: &mut [f32]) {
        self.pull_events();
        let block_start = self.position;
        for (i, sample) in out.iter_mut().enumerate() {
            let now = block_start + i as u64;
            self.apply_due_events(now);
            let mix = self.kick.process();
            *sample = self.master.process(mix);
        }
        self.position = block_start + out.len() as u64;
    }

    /// Moves queued events into the pending list, keeping it sorted by
    /// sample so out-of-order arrivals (a parameter change queued behind a
    /// trigger scheduled 100 ms out) are still applied at the right time.
    fn pull_events(&mut self) {
        while self.pending_len < PENDING_CAPACITY {
            let Some(event) = self.consumer.pop() else {
                break;
            };
            // Insertion sort from the back: the list is short and events
            // arrive almost sorted, so this is a handful of moves.
            let mut i = self.pending_len;
            while i > 0 && self.pending[i - 1].sample > event.sample {
                self.pending[i] = self.pending[i - 1];
                i -= 1;
            }
            self.pending[i] = event;
            self.pending_len += 1;
        }
    }

    /// Applies every pending event stamped at or before `now`. Late events
    /// (stamped in the past) fire immediately rather than being dropped.
    #[inline]
    fn apply_due_events(&mut self, now: u64) {
        let mut consumed = 0;
        while consumed < self.pending_len && self.pending[consumed].sample <= now {
            let event = self.pending[consumed];
            self.apply(event);
            consumed += 1;
        }
        if consumed > 0 {
            self.pending.copy_within(consumed..self.pending_len, 0);
            self.pending_len -= consumed;
        }
    }

    fn apply(&mut self, event: Event) {
        match event.kind {
            EventKind::Trigger { voice, velocity } => match voice {
                VoiceId::Kick => self.kick.trigger(velocity),
            },
            EventKind::Param { target, value } => match target {
                ParamTarget::Voice(VoiceId::Kick, param) => match param {
                    VoiceParam::Tune => self.kick.set_tune(value),
                    VoiceParam::Decay => self.kick.set_decay(value),
                    VoiceParam::Level => self.kick.set_level(value),
                    VoiceParam::Snappy | VoiceParam::Tone => {}
                },
                ParamTarget::Master(MasterParam::OutputGain) => {
                    self.master.set_output_gain(value);
                }
                ParamTarget::Master(MasterParam::Limiter) => {
                    self.master.set_limiter_enabled(value > 0.5);
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sequencer::queue::event_queue;

    #[test]
    fn renderer_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<Renderer>();
    }

    #[test]
    fn trigger_fires_on_the_exact_sample() {
        let (mut p, c) = event_queue(16);
        let mut r = Renderer::new(48_000.0, c);
        p.push(Event::trigger(1_000, VoiceId::Kick, 1.0)).unwrap();
        let mut out = vec![0.0f32; 2_048];
        r.process(&mut out[..512]);
        r.process(&mut out[512..]);
        assert!(out[..1_000].iter().all(|&s| s == 0.0));
        // The click makes the very first sample non-zero.
        assert!(out[1_000] != 0.0, "no output on trigger sample");
        assert_eq!(r.position(), 2_048);
    }

    #[test]
    fn out_of_order_events_are_applied_in_time_order() {
        let (mut p, c) = event_queue(16);
        let mut r = Renderer::new(48_000.0, c);
        // Trigger far ahead, then a level change due immediately.
        p.push(Event::trigger(3_000, VoiceId::Kick, 1.0)).unwrap();
        p.push(Event::param(
            0,
            ParamTarget::Voice(VoiceId::Kick, VoiceParam::Level),
            0.0,
        ))
        .unwrap();
        let mut out = vec![0.0f32; 4_000];
        r.process(&mut out);
        assert!(
            out.iter().all(|&s| s == 0.0),
            "level 0 should have applied before the hit"
        );
    }

    #[test]
    fn late_events_fire_immediately() {
        let (mut p, c) = event_queue(16);
        let mut r = Renderer::new(48_000.0, c);
        let mut out = vec![0.0f32; 100];
        r.process(&mut out);
        p.push(Event::trigger(10, VoiceId::Kick, 1.0)).unwrap();
        r.process(&mut out);
        assert!(out[0] != 0.0);
    }

    #[test]
    fn master_params_apply() {
        let (mut p, c) = event_queue(16);
        let mut r = Renderer::new(48_000.0, c);
        p.push(Event::param(
            0,
            ParamTarget::Master(MasterParam::OutputGain),
            0.25,
        ))
        .unwrap();
        p.push(Event::param(
            0,
            ParamTarget::Master(MasterParam::Limiter),
            1.0,
        ))
        .unwrap();
        r.process(&mut [0.0; 1]);
        assert_eq!(r.master().output_gain(), 0.25);
        assert!(r.master().limiter_enabled());
    }
}
