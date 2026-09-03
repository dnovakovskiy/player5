use sync::ClockSource;

use crate::queue::Producer;
use crate::{Event, Pattern, VoiceId};

/// Walks the pattern ahead of the audio clock and pushes trigger events into
/// the queue. Control-thread only.
///
/// Usage per control tick: `schedule(clock, horizon, producer)`, where
/// `horizon` is the current render position plus the lookahead. Every step
/// whose sample position falls before the horizon is emitted exactly once,
/// in time order. If the queue fills up the scheduler stops and resumes from
/// the same step next tick, so nothing is dropped or duplicated.
#[derive(Clone, Debug)]
pub struct Scheduler {
    pattern: Pattern,
    next_step: u64,
    playing: bool,
    stop_after: Option<u64>,
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new(Pattern::empty())
    }
}

impl Scheduler {
    /// A stopped scheduler holding `pattern`.
    #[must_use]
    pub fn new(pattern: Pattern) -> Self {
        Self {
            pattern,
            next_step: 0,
            playing: false,
            stop_after: None,
        }
    }

    /// The pattern currently being played. Takes effect from the next step.
    #[must_use]
    pub fn pattern(&self) -> &Pattern {
        &self.pattern
    }

    /// Replaces the pattern. Takes effect from the next unscheduled step.
    pub fn set_pattern(&mut self, pattern: Pattern) {
        self.pattern = pattern;
    }

    /// Starts from step 0. The caller anchors the clock so beat 0 is where
    /// playback should begin.
    pub fn start(&mut self) {
        self.next_step = 0;
        self.playing = true;
    }

    /// Stops scheduling. Already-queued events still play.
    pub fn stop(&mut self) {
        self.playing = false;
    }

    /// Stops automatically once `steps` steps have been scheduled since
    /// [`Scheduler::start`] (`None` loops forever). Used for one-shot
    /// playback and offline renders of a fixed number of bars.
    pub fn set_stop_after(&mut self, steps: Option<u64>) {
        self.stop_after = steps;
    }

    /// The automatic stop point, if any.
    #[must_use]
    pub fn stop_after(&self) -> Option<u64> {
        self.stop_after
    }

    /// Whether the scheduler is running.
    #[must_use]
    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// Next absolute step index that will be scheduled.
    #[must_use]
    pub fn next_step(&self) -> u64 {
        self.next_step
    }

    /// Pattern-relative index (`0..16`) of the next step.
    #[must_use]
    pub fn next_pattern_step(&self) -> usize {
        (self.next_step % crate::STEP_COUNT as u64) as usize
    }

    /// Emits every step that falls before `horizon`. Returns the number of
    /// events pushed.
    pub fn schedule<C: ClockSource>(
        &mut self,
        clock: &C,
        horizon: u64,
        out: &mut Producer,
    ) -> usize {
        if !self.playing {
            return 0;
        }
        let mut pushed = 0;
        loop {
            if self.stop_after.is_some_and(|end| self.next_step >= end) {
                self.playing = false;
                break;
            }
            let beat = self.pattern.step_beat(self.next_step);
            let sample = clock.sample_at_beat(beat).round().max(0.0) as u64;
            if sample >= horizon {
                break;
            }
            // A step is emitted atomically: either all its voices fit in the
            // queue or none are pushed and we retry the step next tick.
            if out.vacant() < VoiceId::COUNT {
                break;
            }
            let index = self.next_pattern_step();
            for voice in VoiceId::ALL {
                let step = self.pattern.track(voice).steps[index];
                if step.on {
                    let velocity = self.pattern.velocity(step);
                    // Cannot fail: vacancy was checked above.
                    let _ = out.push(Event::trigger(sample, voice, velocity));
                    pushed += 1;
                }
            }
            self.next_step += 1;
        }
        pushed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue::event_queue;
    use crate::{EventKind, Step, Track};
    use sync::InternalClock;

    fn drain(c: &mut crate::queue::Consumer) -> Vec<Event> {
        std::iter::from_fn(|| c.pop()).collect()
    }

    fn four_on_the_floor() -> Pattern {
        let mut p = Pattern::empty();
        *p.track_mut(VoiceId::Kick) = Track::parse("X--- x--- X--- x---").unwrap();
        p.accent = 1.0;
        p
    }

    #[test]
    fn stopped_scheduler_emits_nothing() {
        let clock = InternalClock::new(48_000.0, 120.0);
        let (mut p, mut c) = event_queue(16);
        let mut s = Scheduler::new(four_on_the_floor());
        assert_eq!(s.schedule(&clock, 1_000_000, &mut p), 0);
        assert!(drain(&mut c).is_empty());
    }

    #[test]
    fn steps_land_on_exact_samples() {
        // 120 BPM at 48 kHz: a beat is 24 000 samples, a step 6 000.
        let clock = InternalClock::new(48_000.0, 120.0);
        let (mut p, mut c) = event_queue(64);
        let mut s = Scheduler::new(four_on_the_floor());
        s.start();
        let n = s.schedule(&clock, 96_000, &mut p);
        assert_eq!(n, 4);
        let events = drain(&mut c);
        let samples: Vec<u64> = events.iter().map(|e| e.sample).collect();
        assert_eq!(samples, vec![0, 24_000, 48_000, 72_000]);
        // Accents follow the pattern.
        let velocities: Vec<f32> = events
            .iter()
            .map(|e| match e.kind {
                EventKind::Trigger { velocity, .. } => velocity,
                EventKind::Param { .. } => unreachable!(),
            })
            .collect();
        assert_eq!(velocities, vec![1.0, 0.7, 1.0, 0.7]);
        // Next call continues from where it left off (bar 2, step 0 = beat 4).
        assert_eq!(s.next_step(), 16);
        assert_eq!(s.schedule(&clock, 96_001, &mut p), 1);
        assert_eq!(drain(&mut c)[0].sample, 96_000);
    }

    #[test]
    fn horizon_is_exclusive_and_incremental() {
        let clock = InternalClock::new(48_000.0, 120.0);
        let (mut p, mut c) = event_queue(64);
        let mut s = Scheduler::new(four_on_the_floor());
        s.start();
        assert_eq!(s.schedule(&clock, 24_000, &mut p), 1); // only step 0
        assert_eq!(s.schedule(&clock, 24_000, &mut p), 0); // nothing new
        assert_eq!(s.schedule(&clock, 24_001, &mut p), 1); // beat 1
        assert_eq!(drain(&mut c).len(), 2);
    }

    #[test]
    fn shuffle_shifts_off_beat_sixteenths() {
        let clock = InternalClock::new(48_000.0, 120.0);
        let (mut p, mut c) = event_queue(64);
        let mut pattern = Pattern::empty();
        *pattern.track_mut(VoiceId::Kick) = Track::parse("xxxx xxxx xxxx xxxx").unwrap();
        pattern.shuffle = 1.0;
        let mut s = Scheduler::new(pattern);
        s.start();
        s.schedule(&clock, 24_000, &mut p);
        let samples: Vec<u64> = drain(&mut c).iter().map(|e| e.sample).collect();
        // Step = 6 000 samples; full shuffle delays odd steps by 2 000.
        assert_eq!(samples, vec![0, 8_000, 12_000, 20_000]);
    }

    #[test]
    fn full_queue_pauses_without_losing_steps() {
        let clock = InternalClock::new(48_000.0, 120.0);
        let (mut p, mut c) = event_queue(2);
        let mut pattern = Pattern::empty();
        pattern.track_mut(VoiceId::Kick).steps = [Step::ON; 16];
        let mut s = Scheduler::new(pattern);
        s.start();
        assert_eq!(s.schedule(&clock, 1_000_000, &mut p), 2);
        assert_eq!(s.next_step(), 2);
        let first = drain(&mut c);
        assert_eq!(s.schedule(&clock, 1_000_000, &mut p), 2);
        let second = drain(&mut c);
        let samples: Vec<u64> = first.iter().chain(&second).map(|e| e.sample).collect();
        assert_eq!(samples, vec![0, 6_000, 12_000, 18_000]);
    }

    #[test]
    fn stop_after_ends_playback_exactly() {
        let clock = InternalClock::new(48_000.0, 120.0);
        let (mut p, mut c) = event_queue(64);
        let mut pattern = Pattern::empty();
        pattern.track_mut(VoiceId::Kick).steps = [Step::ON; 16];
        let mut s = Scheduler::new(pattern);
        s.set_stop_after(Some(20));
        s.start();
        assert_eq!(s.schedule(&clock, 10_000_000, &mut p), 20);
        assert!(!s.is_playing());
        assert_eq!(drain(&mut c).last().unwrap().sample, 19 * 6_000);
        assert_eq!(s.schedule(&clock, 10_000_000, &mut p), 0);
    }

    #[test]
    fn pattern_change_applies_from_next_step() {
        let clock = InternalClock::new(48_000.0, 120.0);
        let (mut p, mut c) = event_queue(64);
        let mut s = Scheduler::new(four_on_the_floor());
        s.start();
        s.schedule(&clock, 24_000, &mut p);
        s.set_pattern(Pattern::empty());
        s.schedule(&clock, 96_000, &mut p);
        assert_eq!(drain(&mut c).len(), 1);
    }
}
