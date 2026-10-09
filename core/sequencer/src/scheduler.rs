use sync::ClockSource;

use crate::queue::Producer;
use crate::{Event, Pattern, VoiceId};

/// Queue slots the scheduler always leaves free, so a flush (sent when the
/// timeline jumps) still fits however full the scheduler and parameter
/// changes have made the queue. Every producer of non-flush events should
/// honour it (see `engine::Control`).
pub const FLUSH_RESERVE: usize = 2;

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
    /// Steps per run, as configured.
    stop_after: Option<u64>,
    /// Absolute step at which the current run ends.
    stop_at: Option<u64>,
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
            stop_at: None,
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
        self.start_at(0);
    }

    /// Starts a run at absolute step `step` (pattern position `step % 16`),
    /// for joining a running external timeline in phase. A stop-after
    /// count ([`Scheduler::set_stop_after`]) counts from `step`.
    pub fn start_at(&mut self, step: u64) {
        self.next_step = step;
        self.playing = true;
        self.stop_at = self.stop_after.map(|n| step.saturating_add(n));
    }

    /// Moves the cursor of the current run to `step` after the timeline
    /// jumped, without starting a new run: a stop-after count keeps the
    /// number of steps still to play. `unplayed` is the first step that
    /// was discarded by the accompanying flush (or [`Scheduler::next_step`]
    /// if none was), so the discarded steps are played again rather than
    /// counted as heard.
    pub fn seek(&mut self, step: u64, unplayed: u64) {
        if let Some(end) = self.stop_at {
            let remaining = end.saturating_sub(unplayed.min(self.next_step));
            self.stop_at = Some(step.saturating_add(remaining));
        }
        self.next_step = step;
    }

    /// The first absolute step whose (shuffled) position on `clock` falls at
    /// or after `sample`. Steps before beat 0 do not exist, so the result is
    /// never below 0.
    #[must_use]
    pub fn first_step_at_or_after<C: ClockSource>(&self, clock: &C, sample: u64) -> u64 {
        let beat = clock.beat_at_sample(sample as f64);
        let mut step = if beat <= 0.0 {
            0
        } else {
            (beat / crate::BEATS_PER_STEP).floor() as u64
        };
        // Back off one in case shuffle delayed the previous step past
        // `sample`, then walk forward to the first step at or after it.
        step = step.saturating_sub(1);
        while self.step_sample(clock, step) < sample {
            step += 1;
        }
        step
    }

    /// Moves the cursor to the first step at or after `sample` on `clock`,
    /// after the timeline jumped (see [`Scheduler::seek`] for how a
    /// stop-after count is kept). Use together with an [`Event::flush`] so
    /// already-queued steps from the old timeline are discarded.
    pub fn resync<C: ClockSource>(&mut self, clock: &C, sample: u64) {
        let step = self.first_step_at_or_after(clock, sample);
        self.seek(step, self.next_step);
    }

    /// The sample `step`'s grid hit is queued at on `clock`.
    #[must_use]
    pub fn step_sample<C: ClockSource>(&self, clock: &C, step: u64) -> u64 {
        let beat = self.pattern.step_beat(step);
        clock.sample_at_beat(beat).round().max(0.0) as u64
    }

    /// The samples `step`'s earliest event and its grid hit are queued at on
    /// `clock`: the earliest is a flam's grace note when the step has one,
    /// otherwise the grid hit itself.
    #[must_use]
    pub fn step_span<C: ClockSource>(&self, clock: &C, step: u64) -> (u64, u64) {
        let sample = self.step_sample(clock, step);
        let index = (step % crate::STEP_COUNT as u64) as usize;
        let flam = VoiceId::ALL.into_iter().any(|voice| {
            let track = self.pattern.track(voice);
            let s = track.steps[index];
            !track.mute && s.on && s.flam
        });
        if flam {
            (sample.saturating_sub(self.flam_samples(clock)), sample)
        } else {
            (sample, sample)
        }
    }

    fn flam_samples<C: ClockSource>(&self, clock: &C) -> u64 {
        (self.pattern.flam_seconds() * clock.sample_rate()).round() as u64
    }

    /// Stops scheduling. Already-queued events still play.
    pub fn stop(&mut self) {
        self.playing = false;
    }

    /// Ends each run automatically once `steps` steps have been scheduled
    /// from the step it started at ([`Scheduler::start`] or
    /// [`Scheduler::start_at`]); `None` loops forever. Set while playing,
    /// it counts from the next unscheduled step. Used for one-shot
    /// playback and offline renders of a fixed number of bars.
    pub fn set_stop_after(&mut self, steps: Option<u64>) {
        self.stop_after = steps;
        self.stop_at = steps.map(|n| self.next_step.saturating_add(n));
    }

    /// The configured stop-after count, if any.
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
    /// events pushed. Leaves at least [`FLUSH_RESERVE`] slots free.
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
            if self.stop_at.is_some_and(|end| self.next_step >= end) {
                self.playing = false;
                break;
            }
            let sample = self.step_sample(clock, self.next_step);
            if sample >= horizon {
                break;
            }
            // A step is emitted atomically: either all its voices (and their
            // flam grace notes) fit in the queue or none are pushed and we
            // retry the step next tick.
            if out.vacant() < 2 * VoiceId::COUNT + FLUSH_RESERVE {
                break;
            }
            let index = self.next_pattern_step();
            let flam_samples = self.flam_samples(clock);
            for voice in VoiceId::ALL {
                let track = self.pattern.track(voice);
                if track.mute {
                    continue;
                }
                let step = track.steps[index];
                if step.on {
                    let velocity = self.pattern.velocity(step);
                    // Cannot fail: vacancy was checked above.
                    if step.flam {
                        let grace_at = sample.saturating_sub(flam_samples);
                        let grace = velocity * crate::FLAM_GRACE_RATIO;
                        let _ = out.push(Event::trigger(grace_at, voice, grace));
                        pushed += 1;
                    }
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
                EventKind::Param { .. } | EventKind::Flush => unreachable!(),
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
    fn flam_adds_a_quieter_grace_note_before_the_grid() {
        let clock = InternalClock::new(48_000.0, 120.0);
        let (mut p, mut c) = event_queue(64);
        let mut pattern = Pattern::empty();
        *pattern.track_mut(VoiceId::Snare) = Track::parse("---- F--- ---- ----").unwrap();
        pattern.flam = 0.0; // 8 ms = 384 samples
        pattern.accent = 1.0;
        let mut s = Scheduler::new(pattern);
        s.start();
        s.schedule(&clock, 48_000, &mut p);
        let events = drain(&mut c);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].sample, 24_000 - 384);
        assert_eq!(events[1].sample, 24_000);
        match (events[0].kind, events[1].kind) {
            (EventKind::Trigger { velocity: g, .. }, EventKind::Trigger { velocity: m, .. }) => {
                assert_eq!(m, 1.0);
                assert!((g - 0.6).abs() < 1e-6);
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn muted_tracks_schedule_nothing() {
        let clock = InternalClock::new(48_000.0, 120.0);
        let (mut p, mut c) = event_queue(64);
        let mut pattern = four_on_the_floor();
        *pattern.track_mut(VoiceId::ClosedHat) = Track::parse("x-x- x-x- x-x- x-x-").unwrap();
        pattern.track_mut(VoiceId::Kick).mute = true;
        let mut s = Scheduler::new(pattern);
        s.start();
        s.schedule(&clock, 96_000, &mut p);
        let events = drain(&mut c);
        assert_eq!(events.len(), 8);
        assert!(events.iter().all(|e| matches!(
            e.kind,
            EventKind::Trigger {
                voice: VoiceId::ClosedHat,
                ..
            }
        )));
    }

    #[test]
    fn resync_finds_the_next_step_on_a_moved_timeline() {
        let mut clock = InternalClock::new(48_000.0, 120.0);
        let mut s = Scheduler::new(four_on_the_floor());
        s.start();
        // Timeline jumps: beat 0 is now at sample 1 000.
        clock.reset(1_000.0);
        s.resync(&clock, 30_000);
        // Step 5 is at beat 1.25 = 1 000 + 30 000 = 31 000 ≥ 30 000; step 4
        // is at 25 000.
        assert_eq!(s.next_step(), 5);
        // With shuffle, an odd step may sit later than its straight slot.
        let mut pattern = four_on_the_floor();
        pattern.shuffle = 1.0;
        s.set_pattern(pattern);
        s.resync(&clock, 31_500);
        // Step 5 (odd) is delayed by 2 000 → 33 000 ≥ 31 500.
        assert_eq!(s.next_step(), 5);
        assert_eq!(s.first_step_at_or_after(&clock, 0), 0);
    }

    #[test]
    fn start_at_joins_mid_pattern() {
        let clock = InternalClock::new(48_000.0, 120.0);
        let (mut p, mut c) = event_queue(64);
        let mut s = Scheduler::new(four_on_the_floor());
        s.start_at(20); // bar 2, step 4
        assert_eq!(s.next_pattern_step(), 4);
        s.schedule(&clock, 20 * 6_000 + 1, &mut p);
        let events = drain(&mut c);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].sample, 120_000);
    }

    #[test]
    fn full_queue_pauses_without_losing_steps() {
        let clock = InternalClock::new(48_000.0, 120.0);
        // 32 slots; a step needs room for every voice plus flams (20) and
        // the flush reserve (2), so the scheduler stops once fewer than 22
        // slots are free.
        let (mut p, mut c) = event_queue(32);
        let mut pattern = Pattern::empty();
        pattern.track_mut(VoiceId::Kick).steps = [Step::ON; 16];
        let mut s = Scheduler::new(pattern);
        s.start();
        assert_eq!(s.schedule(&clock, 1_000_000, &mut p), 11);
        assert_eq!(s.next_step(), 11);
        assert_eq!(s.schedule(&clock, 1_000_000, &mut p), 0);
        let first = drain(&mut c);
        assert_eq!(s.schedule(&clock, 1_000_000, &mut p), 11);
        let second = drain(&mut c);
        let samples: Vec<u64> = first.iter().chain(&second).map(|e| e.sample).collect();
        let expected: Vec<u64> = (0..22).map(|k| k * 6_000).collect();
        assert_eq!(samples, expected);
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

    /// Joining a running timeline at step 53 with a one-bar limit plays
    /// one bar (53..69), not nothing (the limit used to be absolute).
    #[test]
    fn stop_after_counts_from_the_start_step() {
        let clock = InternalClock::new(48_000.0, 120.0);
        let (mut p, mut c) = event_queue(64);
        let mut pattern = Pattern::empty();
        pattern.track_mut(VoiceId::Kick).steps = [Step::ON; 16];
        let mut s = Scheduler::new(pattern);
        s.set_stop_after(Some(16));
        s.start_at(53);
        assert!(s.is_playing());
        let mut samples = Vec::new();
        for _ in 0..4 {
            s.schedule(&clock, 10_000_000, &mut p);
            samples.extend(drain(&mut c).iter().map(|e| e.sample));
        }
        let expected: Vec<u64> = (53..69).map(|k| k * 6_000).collect();
        assert_eq!(samples, expected);
        assert!(!s.is_playing());
        // A second run counts afresh from its own start.
        s.start_at(3);
        s.schedule(&clock, 10_000_000, &mut p);
        assert_eq!(drain(&mut c).len(), 16);
    }

    #[test]
    fn seek_keeps_the_steps_left_to_play() {
        let clock = InternalClock::new(48_000.0, 120.0);
        let (mut p, mut c) = event_queue(256);
        let mut pattern = Pattern::empty();
        pattern.track_mut(VoiceId::Kick).steps = [Step::ON; 16];
        let mut s = Scheduler::new(pattern);
        s.set_stop_after(Some(16));
        s.start_at(100);
        // Steps 100..105 queued; 103 and 104 are then flushed by a jump
        // and the timeline continues at step 40.
        s.schedule(&clock, 105 * 6_000, &mut p);
        assert_eq!(s.next_step(), 105);
        drain(&mut c);
        s.seek(40, 103);
        // 3 were heard, 13 remain.
        s.schedule(&clock, 10_000_000, &mut p);
        assert_eq!(drain(&mut c).len(), 13);
        assert!(!s.is_playing());
    }

    #[test]
    fn step_span_starts_at_the_grace_note() {
        let clock = InternalClock::new(48_000.0, 120.0);
        let mut pattern = Pattern::empty();
        *pattern.track_mut(VoiceId::Snare) = Track::parse("---- f--- ---- ----").unwrap();
        pattern.flam = 0.0; // 384 samples
        let mut s = Scheduler::new(pattern);
        assert_eq!(s.step_span(&clock, 4), (24_000 - 384, 24_000));
        assert_eq!(s.step_span(&clock, 5), (30_000, 30_000));
        let mut muted = *s.pattern();
        muted.track_mut(VoiceId::Snare).mute = true;
        s.set_pattern(muted);
        assert_eq!(s.step_span(&clock, 4), (24_000, 24_000));
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
