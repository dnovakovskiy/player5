use std::sync::Arc;

use dsp::{Kit, Master};
use sequencer::queue::Consumer;
use sequencer::{Event, EventKind, MasterParam, ParamTarget, VoiceId};

use crate::control::dsp_param;
use crate::timing::SharedTiming;

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
    kit: Kit,
    master: Master,
    timing: Arc<SharedTiming>,
}

impl Renderer {
    /// Creates the render half over `consumer`, publishing its position to
    /// `timing`.
    #[must_use]
    pub fn new(sample_rate: f32, consumer: Consumer, timing: Arc<SharedTiming>) -> Self {
        Self {
            sample_rate,
            position: 0,
            consumer,
            pending: [Event::trigger(0, VoiceId::Kick, 0.0); PENDING_CAPACITY],
            pending_len: 0,
            kit: Kit::new(sample_rate),
            master: Master::default(),
            timing,
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

    /// The voices.
    #[must_use]
    pub fn kit(&self) -> &Kit {
        &self.kit
    }

    /// The master section.
    #[must_use]
    pub fn master(&self) -> &Master {
        &self.master
    }

    /// Renders one block of mono audio. Real-time safe.
    pub fn process(&mut self, out: &mut [f32]) {
        self.process_at(out, 0);
    }

    /// Renders one block and publishes `host_ticks` (the platform host time
    /// at which the block's first sample is output) for host-time mapping.
    /// Real-time safe.
    pub fn process_at(&mut self, out: &mut [f32], host_ticks: u64) {
        self.timing.publish(self.position, host_ticks);
        self.pull_events();
        let block_start = self.position;
        for (i, sample) in out.iter_mut().enumerate() {
            let now = block_start + i as u64;
            self.apply_due_events(now);
            let mix = self.kit.process();
            *sample = self.master.process(mix);
        }
        self.position = block_start + out.len() as u64;
    }

    /// Moves queued events into the pending list, keeping it sorted by
    /// sample so out-of-order arrivals (a parameter change queued behind a
    /// trigger scheduled 100 ms out) are still applied at the right time.
    /// A flush drops held triggers at or after its sample immediately.
    fn pull_events(&mut self) {
        while self.pending_len < PENDING_CAPACITY {
            let Some(event) = self.consumer.pop() else {
                break;
            };
            if let EventKind::Flush = event.kind {
                self.flush_from(event.sample);
                continue;
            }
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

    fn flush_from(&mut self, sample: u64) {
        let mut kept = 0;
        for i in 0..self.pending_len {
            let e = self.pending[i];
            let drop = matches!(e.kind, EventKind::Trigger { .. }) && e.sample >= sample;
            if !drop {
                self.pending[kept] = e;
                kept += 1;
            }
        }
        self.pending_len = kept;
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
            EventKind::Trigger { voice, velocity } => self.kit.trigger(voice.index(), velocity),
            EventKind::Param { target, value } => match target {
                ParamTarget::Voice(voice, param) => {
                    self.kit.set_param(voice.index(), dsp_param(param), value);
                }
                ParamTarget::Master(MasterParam::OutputGain) => {
                    self.master.set_output_gain(value);
                }
                ParamTarget::Master(MasterParam::Limiter) => {
                    self.master.set_limiter_enabled(value > 0.5);
                }
            },
            EventKind::Flush => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sequencer::queue::event_queue;
    use sequencer::VoiceParam;

    fn renderer() -> (sequencer::queue::Producer, Renderer) {
        let (p, c) = event_queue(64);
        (
            p,
            Renderer::new(48_000.0, c, Arc::new(SharedTiming::default())),
        )
    }

    #[test]
    fn renderer_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<Renderer>();
    }

    #[test]
    fn voice_ids_match_kit_slots() {
        use dsp::slot;
        assert_eq!(VoiceId::COUNT, dsp::VOICE_COUNT);
        let pairs = [
            (VoiceId::Kick, slot::KICK),
            (VoiceId::Snare, slot::SNARE),
            (VoiceId::LowTom, slot::LOW_TOM),
            (VoiceId::MidTom, slot::MID_TOM),
            (VoiceId::HighTom, slot::HIGH_TOM),
            (VoiceId::Rim, slot::RIM),
            (VoiceId::Clap, slot::CLAP),
            (VoiceId::ClosedHat, slot::CLOSED_HAT),
            (VoiceId::OpenHat, slot::OPEN_HAT),
            (VoiceId::Cowbell, slot::COWBELL),
        ];
        for (v, s) in pairs {
            assert_eq!(v.index(), s, "{v:?}");
        }
    }

    #[test]
    fn trigger_fires_on_the_exact_sample() {
        let (mut p, mut r) = renderer();
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
        let (mut p, mut r) = renderer();
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
        let (mut p, mut r) = renderer();
        let mut out = vec![0.0f32; 100];
        r.process(&mut out);
        p.push(Event::trigger(10, VoiceId::Kick, 1.0)).unwrap();
        r.process(&mut out);
        assert!(out[0] != 0.0);
    }

    #[test]
    fn flush_drops_held_triggers_but_keeps_params() {
        let (mut p, mut r) = renderer();
        p.push(Event::trigger(2_000, VoiceId::Kick, 1.0)).unwrap();
        p.push(Event::param(
            1_500,
            ParamTarget::Master(MasterParam::OutputGain),
            0.5,
        ))
        .unwrap();
        p.push(Event::flush(1_000)).unwrap();
        let mut out = vec![0.0f32; 4_000];
        r.process(&mut out);
        assert!(out.iter().all(|&s| s == 0.0), "flushed trigger played");
        assert_eq!(r.master().output_gain(), 0.5);
    }

    #[test]
    fn master_params_apply() {
        let (mut p, mut r) = renderer();
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

    #[test]
    fn publishes_timing() {
        let (_p, mut r) = renderer();
        let timing = Arc::clone(&r.timing);
        r.process_at(&mut [0.0; 64], 777);
        r.process_at(&mut [0.0; 64], 888);
        let snap = timing.read();
        assert_eq!(snap.position, 64);
        assert_eq!(snap.host_ticks, 888);
    }
}
