use std::sync::Arc;

use dsp::{Param, VoiceParams, VOICE_COUNT};
use sequencer::queue::Producer;
use sequencer::{Event, MasterParam, ParamTarget, Pattern, Scheduler, VoiceId, VoiceParam};
use sync::{
    AdjustedClock, ClockControls, ClockSource, FollowerClock, InternalClock, MidiClockFollower,
    MidiMessage, Observation, Phase, Precision, TapTempo,
};

use crate::timing::{SharedTiming, TimingSnapshot};

/// Default lookahead: 100 ms at 48 kHz. See ADR-0001.
pub const DEFAULT_LOOKAHEAD_SAMPLES: u64 = 4_800;

/// After a following snap moves the timeline forward, a step that now
/// falls at most this long before `now` is still played (late) rather than
/// skipped, e.g. the downbeat right after a MIDI Start. ADR-0006.
const SNAP_GRACE_S: f64 = 0.02;

/// Which clock drives the sequencer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockMode {
    /// The free-running internal clock (tempo from the pattern / UI / tap).
    Internal,
    /// Follow observations from an external source with this tuning.
    Follow(Precision),
}

/// Borrowed view of whichever clock is active.
#[derive(Clone, Copy, Debug)]
enum ActiveClock<'a> {
    Internal(&'a InternalClock),
    Follow(&'a FollowerClock),
}

impl ClockSource for ActiveClock<'_> {
    fn sample_rate(&self) -> f64 {
        match self {
            Self::Internal(c) => c.sample_rate(),
            Self::Follow(c) => c.sample_rate(),
        }
    }
    fn tempo_bpm(&self) -> f64 {
        match self {
            Self::Internal(c) => c.tempo_bpm(),
            Self::Follow(c) => c.tempo_bpm(),
        }
    }
    fn beat_at_sample(&self, sample: f64) -> f64 {
        match self {
            Self::Internal(c) => c.beat_at_sample(sample),
            Self::Follow(c) => c.beat_at_sample(sample),
        }
    }
    fn sample_at_beat(&self, beat: f64) -> f64 {
        match self {
            Self::Internal(c) => c.sample_at_beat(beat),
            Self::Follow(c) => c.sample_at_beat(beat),
        }
    }
}

/// Control-thread half of the engine. Owns the clocks, the scheduler and
/// the producer end of the event queue.
///
/// Every mutation that must be audible goes through the queue as an event
/// stamped with a sample position; the only memory shared with the renderer
/// is the [`SharedTiming`] the renderer publishes.
pub struct Control {
    sample_rate: f32,
    mode: ClockMode,
    internal: InternalClock,
    follower: FollowerClock,
    tap: TapTempo,
    midi: MidiClockFollower,
    controls: ClockControls,
    scheduler: Scheduler,
    producer: Producer,
    lookahead: u64,
    timing: Arc<SharedTiming>,
    sent_params: [VoiceParams; VOICE_COUNT],
    sent_gain: f32,
    sent_limiter: bool,
    last_now: u64,
}

impl Control {
    /// Default tempo.
    pub const DEFAULT_BPM: f64 = 120.0;

    /// Creates the control half over `producer`, reading render timing from
    /// `timing`.
    #[must_use]
    pub fn new(sample_rate: f32, producer: Producer, timing: Arc<SharedTiming>) -> Self {
        let sr = f64::from(sample_rate);
        Self {
            sample_rate,
            mode: ClockMode::Internal,
            internal: InternalClock::new(sr, Self::DEFAULT_BPM),
            follower: FollowerClock::new(sr, Self::DEFAULT_BPM, Precision::Fine),
            tap: TapTempo::new(sr),
            midi: MidiClockFollower::new(sr),
            controls: ClockControls::default(),
            scheduler: Scheduler::default(),
            producer,
            lookahead: (DEFAULT_LOOKAHEAD_SAMPLES as f64 * sr / 48_000.0).round() as u64,
            timing,
            sent_params: [VoiceParams::default(); VOICE_COUNT],
            sent_gain: 1.0,
            sent_limiter: false,
            last_now: 0,
        }
    }

    /// Sample rate.
    #[must_use]
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    fn active(&self) -> ActiveClock<'_> {
        active_of(self.mode, &self.internal, &self.follower)
    }

    /// The internal clock.
    #[must_use]
    pub fn internal_clock(&self) -> &InternalClock {
        &self.internal
    }

    /// The follower clock (meaningful in [`ClockMode::Follow`]).
    #[must_use]
    pub fn follower_clock(&self) -> &FollowerClock {
        &self.follower
    }

    /// Current clock mode.
    #[must_use]
    pub fn clock_mode(&self) -> ClockMode {
        self.mode
    }

    /// Switches clock mode at `now`, keeping the beat continuous so a
    /// running pattern does not jump. Switching between two following
    /// precisions keeps the follower (and its lock); entering follow mode
    /// starts a fresh follower on the current beat, which snaps to the
    /// source on its first observation like any first lock.
    pub fn set_clock_mode(&mut self, mode: ClockMode, now: u64) {
        if mode == self.mode {
            return;
        }
        let beat = self.active().beat_at_sample(now as f64);
        let bpm = self.active().tempo_bpm();
        match (self.mode, mode) {
            (_, ClockMode::Internal) => {
                self.internal.set_tempo(bpm, now as f64);
                self.internal.align(now as f64, beat);
            }
            (ClockMode::Follow(_), ClockMode::Follow(precision)) => {
                self.follower.set_precision(precision);
            }
            (ClockMode::Internal, ClockMode::Follow(precision)) => {
                self.follower = FollowerClock::new(f64::from(self.sample_rate), bpm, precision);
                self.follower.reset(now as f64, beat);
                let _ = self.follower.take_discontinuity();
            }
        }
        self.mode = mode;
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

    /// Phase nudge in milliseconds at the current tempo (positive = later).
    pub fn set_nudge_ms(&mut self, ms: f64) {
        let ms = if ms.is_finite() { ms } else { 0.0 };
        self.controls.nudge_beats = ms / 1_000.0 * self.tempo() / 60.0;
    }

    /// Phase nudge in milliseconds at the current tempo.
    #[must_use]
    pub fn nudge_ms(&self) -> f64 {
        self.controls.nudge_beats * 60_000.0 / self.tempo()
    }

    /// Output-path latency compensation in milliseconds (positive = trigger
    /// earlier).
    pub fn set_latency_ms(&mut self, ms: f64) {
        self.controls.latency_ms = if ms.is_finite() { ms } else { 0.0 };
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

    /// Sets the internal clock's tempo, keeping the beat continuous at
    /// `now`. Ignored for scheduling while following an external clock.
    pub fn set_tempo(&mut self, bpm: f64, now: u64) {
        self.internal.set_tempo(bpm, now as f64);
    }

    /// Tempo of the active clock.
    #[must_use]
    pub fn tempo(&self) -> f64 {
        self.active().tempo_bpm()
    }

    /// The beat (with the global controls applied) that falls on `sample`.
    #[must_use]
    pub fn beat_at(&self, sample: u64) -> f64 {
        AdjustedClock::new(&self.active(), self.controls).beat_at_sample(sample as f64)
    }

    /// Whether the active clock is tracking a live source (always `true`
    /// for the internal clock).
    #[must_use]
    pub fn is_locked(&self) -> bool {
        match self.mode {
            ClockMode::Internal => true,
            ClockMode::Follow(_) => self.follower.is_locked(),
        }
    }

    /// Starts playback at `now`. Internal clock: beat 0 and step 0 fall on
    /// `now`. Following: joins the external timeline in phase at the next
    /// step, so the pattern position matches the source's bar position.
    pub fn start(&mut self, now: u64) {
        match self.mode {
            ClockMode::Internal => {
                self.internal.reset(now as f64);
                self.scheduler.start();
            }
            ClockMode::Follow(_) => {
                let active = self.active();
                let clock = AdjustedClock::new(&active, self.controls);
                let step = self.scheduler.first_step_at_or_after(&clock, now);
                self.scheduler.start_at(step);
            }
        }
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

    /// Feeds an external observation (sample-domain). Ignored unless
    /// following. Small corrections leave the schedule alone (the follower
    /// slews); a snap flushes and realigns immediately.
    pub fn observe(&mut self, observation: &Observation, now: u64) {
        if !matches!(self.mode, ClockMode::Follow(_)) {
            return;
        }
        // The timeline before the observation tells which queued steps
        // have already played if it snaps.
        let before = self.follower.clone();
        self.follower.observe(observation, now as f64);
        if self.follower.take_discontinuity() {
            self.realign_after_snap(now, &before);
        }
    }

    /// Feeds a MIDI clock message received at `sample`. While following,
    /// Start and Continue request a re-sync: the source's song position
    /// jumped, so the next pulse snaps instead of slewing.
    pub fn midi(&mut self, message: MidiMessage, sample: f64, now: u64) {
        if matches!(message, MidiMessage::Start | MidiMessage::Continue)
            && matches!(self.mode, ClockMode::Follow(_))
        {
            self.follower.request_resync();
        }
        if let Some(obs) = self.midi.handle(message, sample) {
            self.observe(&obs, now);
        }
    }

    /// Registers a tap at `sample`. Internal clock: sets tempo and pulls the
    /// beat grid onto the tap. Following: treated as an observation.
    pub fn tap(&mut self, sample: f64, now: u64) {
        let Some(obs) = self.tap.tap(sample) else {
            return;
        };
        match self.mode {
            ClockMode::Internal => {
                if let Some(bpm) = obs.bpm {
                    self.internal.set_tempo(bpm, sample);
                }
                let beat = self.internal.beat_at_sample(sample);
                self.internal.align(sample, beat.round());
                self.realign(now);
            }
            ClockMode::Follow(_) => self.observe(&obs, now),
        }
    }

    /// Quantized re-sync. Following: the next observation snaps phase
    /// instead of slewing. Internal: the bar restarts at `now`.
    pub fn resync(&mut self, now: u64) {
        match self.mode {
            ClockMode::Internal => {
                self.internal.reset(now as f64);
                self.realign(now);
            }
            ClockMode::Follow(_) => self.follower.request_resync(),
        }
    }

    /// After the timeline moved: drop already-queued triggers from `now` on
    /// and continue from the first step at or after `now`.
    fn realign(&mut self, now: u64) {
        if !self.scheduler.is_playing() {
            return;
        }
        let _ = self.producer.push(Event::flush(now));
        let active = self.active();
        let clock = AdjustedClock::new(&active, self.controls);
        let step = self.scheduler.first_step_at_or_after(&clock, now);
        self.scheduler.start_at(step);
    }

    /// [`Control::realign`] after a follower snap, where both timelines are
    /// known. Nothing is heard twice: no step the old timeline already
    /// played (stamped before `now`) is scheduled again, and nothing lands
    /// within half a step after the last step already heard (a forward jump
    /// of a whole number of steps renumbers that same musical step). Within
    /// those limits a step the jump left just behind `now` (by up to
    /// [`SNAP_GRACE_S`]) is played late instead of skipped.
    fn realign_after_snap(&mut self, now: u64, before: &FollowerClock) {
        if !self.scheduler.is_playing() {
            return;
        }
        let _ = self.producer.push(Event::flush(now));
        let old = AdjustedClock::new(before, self.controls);
        let unplayed = self
            .scheduler
            .first_step_at_or_after(&old, now)
            .min(self.scheduler.next_step());
        let grace = (SNAP_GRACE_S * f64::from(self.sample_rate)).round() as u64;
        let mut from = now.saturating_sub(grace);
        if let Some(last) = unplayed.checked_sub(1) {
            let beat = self.scheduler.pattern().step_beat(last);
            let heard = old.sample_at_beat(beat);
            let half_step = 0.5 * sequencer::BEATS_PER_STEP * self.follower.samples_per_beat();
            from = from.max((heard + half_step).ceil().max(0.0) as u64);
        }
        let new = AdjustedClock::new(&self.follower, self.controls);
        let step = self
            .scheduler
            .first_step_at_or_after(&new, from)
            .max(unplayed);
        self.scheduler.start_at(step);
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
        let ok = self
            .producer
            .push(Event::param(at, ParamTarget::Voice(voice, param), value))
            .is_ok();
        if ok {
            self.sent_params[voice.index()].set(dsp_param(param), value);
        }
        ok
    }

    /// Queues only the controls of `voice` that differ from what was last
    /// sent. Returns `false` if the queue filled up.
    pub fn set_voice_params(&mut self, voice: VoiceId, params: &VoiceParams, at: u64) -> bool {
        let mut ok = true;
        for (param, vp) in [
            (Param::Tune, VoiceParam::Tune),
            (Param::Decay, VoiceParam::Decay),
            (Param::Tone, VoiceParam::Tone),
            (Param::Snappy, VoiceParam::Snappy),
            (Param::Level, VoiceParam::Level),
        ] {
            let value = params.get(param);
            if self.sent_params[voice.index()].get(param) != value {
                ok &= self.set_voice_param(voice, vp, value, at);
            }
        }
        ok
    }

    /// Queues an output-gain change for sample `at` (skipped if unchanged).
    pub fn set_output_gain(&mut self, gain: f32, at: u64) -> bool {
        if gain == self.sent_gain {
            return true;
        }
        let ok = self
            .producer
            .push(Event::param(
                at,
                ParamTarget::Master(MasterParam::OutputGain),
                gain,
            ))
            .is_ok();
        if ok {
            self.sent_gain = gain;
        }
        ok
    }

    /// Queues a limiter on/off change for sample `at` (skipped if
    /// unchanged).
    pub fn set_limiter(&mut self, enabled: bool, at: u64) -> bool {
        if enabled == self.sent_limiter {
            return true;
        }
        let value = if enabled { 1.0 } else { 0.0 };
        let ok = self
            .producer
            .push(Event::param(
                at,
                ParamTarget::Master(MasterParam::Limiter),
                value,
            ))
            .is_ok();
        if ok {
            self.sent_limiter = enabled;
        }
        ok
    }

    /// The renderer's latest published timing.
    #[must_use]
    pub fn render_timing(&self) -> TimingSnapshot {
        self.timing.read()
    }

    /// Maps a host time (ns, [`sync::host_time`] scale) onto the sample
    /// clock using the renderer's latest timestamp. `None` until the host
    /// has supplied host times.
    #[must_use]
    pub fn host_ns_to_sample(&self, host_ns: u64) -> Option<f64> {
        let snap = self.timing.read();
        if snap.host_ticks == 0 {
            return None;
        }
        let block_ns = sync::host_time::ticks_to_ns(snap.host_ticks);
        let delta_s = (host_ns as f64 - block_ns as f64) / 1e9;
        Some(snap.position as f64 + delta_s * f64::from(self.sample_rate))
    }

    /// Feeds an observation timestamped in host nanoseconds (from a network
    /// source or MIDI driver). Dropped until the renderer has published a
    /// host time.
    pub fn observe_host(&mut self, host_ns: u64, phase: Phase, bpm: Option<f64>) {
        if let Some(sample) = self.host_ns_to_sample(host_ns) {
            let now = self.timing.read().position;
            self.observe(&Observation { sample, phase, bpm }, now);
        }
    }

    /// One control tick at render position `now`: advances the follower,
    /// handles timeline jumps, and schedules up to `now + lookahead`.
    /// Returns the number of events pushed.
    pub fn tick(&mut self, now: u64) -> usize {
        self.last_now = now;
        if matches!(self.mode, ClockMode::Follow(_)) {
            self.follower.advance(now as f64);
            if self.follower.take_discontinuity() {
                self.realign(now);
            }
        }
        let active = active_of(self.mode, &self.internal, &self.follower);
        let clock = AdjustedClock::new(&active, self.controls);
        self.scheduler
            .schedule(&clock, now + self.lookahead, &mut self.producer)
    }

    /// [`Control::tick`] at the renderer's published position, for shells
    /// that run control and render on different threads.
    pub fn tick_shared(&mut self) -> usize {
        let now = self.timing.read().position;
        self.tick(now)
    }

    /// Position passed to the last [`Control::tick`].
    #[must_use]
    pub fn last_tick_position(&self) -> u64 {
        self.last_now
    }
}

fn active_of<'a>(
    mode: ClockMode,
    internal: &'a InternalClock,
    follower: &'a FollowerClock,
) -> ActiveClock<'a> {
    match mode {
        ClockMode::Internal => ActiveClock::Internal(internal),
        ClockMode::Follow(_) => ActiveClock::Follow(follower),
    }
}

/// Maps the sequencer's parameter names onto the DSP ones.
#[must_use]
pub fn dsp_param(param: VoiceParam) -> Param {
    match param {
        VoiceParam::Tune => Param::Tune,
        VoiceParam::Decay => Param::Decay,
        VoiceParam::Level => Param::Level,
        VoiceParam::Snappy => Param::Snappy,
        VoiceParam::Tone => Param::Tone,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sequencer::queue::event_queue;
    use sequencer::{EventKind, Track};

    fn control() -> (Control, sequencer::queue::Consumer) {
        let (p, c) = event_queue(1_024);
        (
            Control::new(48_000.0, p, Arc::new(SharedTiming::default())),
            c,
        )
    }

    fn drain(c: &mut sequencer::queue::Consumer) -> Vec<Event> {
        std::iter::from_fn(|| c.pop()).collect()
    }

    fn kicks() -> Pattern {
        let mut p = Pattern::empty();
        *p.track_mut(VoiceId::Kick) = Track::parse("x--- x--- x--- x---").unwrap();
        p
    }

    #[test]
    fn param_changes_are_diffed() {
        let (mut ctl, mut c) = control();
        let mut params = VoiceParams::default();
        assert!(ctl.set_voice_params(VoiceId::Snare, &params, 0));
        assert!(drain(&mut c).is_empty(), "defaults are not resent");
        params.snappy = 0.9;
        ctl.set_voice_params(VoiceId::Snare, &params, 0);
        let events = drain(&mut c);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0].kind,
            EventKind::Param {
                target: ParamTarget::Voice(VoiceId::Snare, VoiceParam::Snappy),
                ..
            }
        ));
    }

    #[test]
    fn following_joins_in_bar_phase() {
        let (mut ctl, mut c) = control();
        ctl.set_pattern(kicks());
        ctl.set_clock_mode(ClockMode::Follow(Precision::Exact), 0);
        // Source says: at sample 100 000 we are on beat 2 of the bar (2.0).
        ctl.observe(
            &Observation {
                sample: 100_000.0,
                phase: Phase::Bar(2.0),
                bpm: Some(120.0),
            },
            100_000,
        );
        ctl.tick(100_000);
        ctl.start(100_000);
        ctl.tick(100_000);
        let events = drain(&mut c);
        assert!(!events.is_empty());
        // First hit lands on the source's beat 2 (sample 100 000, a kick
        // step since kicks are on every beat).
        assert_eq!(events[0].sample, 100_000);
        assert_eq!(ctl.scheduler().next_step() % 16, 9);
    }

    #[test]
    fn internal_resync_flushes_and_restarts_the_bar() {
        let (mut ctl, mut c) = control();
        ctl.set_pattern(kicks());
        ctl.start(0);
        ctl.tick(0);
        drain(&mut c);
        ctl.resync(10_000);
        ctl.tick(10_000);
        let events = drain(&mut c);
        assert!(matches!(events[0].kind, EventKind::Flush));
        assert_eq!(events[0].sample, 10_000);
        assert_eq!(events[1].sample, 10_000);
    }

    #[test]
    fn host_time_maps_through_the_published_timing() {
        let timing = Arc::new(SharedTiming::default());
        let (p, _c) = event_queue(16);
        let ctl = Control::new(48_000.0, p, Arc::clone(&timing));
        assert_eq!(ctl.host_ns_to_sample(5), None);
        timing.publish(48_000, 1_000_000_000);
        // Off Apple platforms ticks are nanoseconds.
        #[cfg(not(target_vendor = "apple"))]
        {
            let s = ctl.host_ns_to_sample(1_500_000_000).unwrap();
            assert!((s - 72_000.0).abs() < 1e-6, "{s}");
        }
    }

    #[test]
    fn nudge_is_expressed_in_ms() {
        let (mut ctl, _c) = control();
        ctl.set_nudge_ms(10.0);
        assert!((ctl.nudge_ms() - 10.0).abs() < 1e-9);
        // 10 ms at 120 BPM = 0.02 beats.
        assert!((ctl.clock_controls().nudge_beats - 0.02).abs() < 1e-12);
    }

    fn sixteenths() -> Pattern {
        let mut p = Pattern::empty();
        *p.track_mut(VoiceId::ClosedHat) = Track::parse("xxxx xxxx xxxx xxxx").unwrap();
        p
    }

    fn bar_obs(sample: f64, beat: f64, bpm: f64) -> Observation {
        Observation {
            sample,
            phase: Phase::Bar(beat.rem_euclid(4.0)),
            bpm: Some(bpm),
        }
    }

    fn trigger_samples(events: &[Event]) -> Vec<u64> {
        events
            .iter()
            .filter(|e| matches!(e.kind, EventKind::Trigger { .. }))
            .map(|e| e.sample)
            .collect()
    }

    #[test]
    fn mode_switches_are_continuous() {
        let (mut ctl, _c) = control();
        ctl.set_tempo(126.0, 0);
        let before = ctl.beat_at(50_000);
        ctl.set_clock_mode(ClockMode::Follow(Precision::Fine), 50_000);
        assert!((ctl.beat_at(50_000) - before).abs() < 1e-9);
        assert!((ctl.tempo() - 126.0).abs() < 1e-9);
        // Lock, then switch precision: same follower, still locked.
        ctl.observe(&bar_obs(60_000.0, 3.0, 128.0), 60_000);
        assert!(ctl.is_locked());
        let before = ctl.beat_at(70_000);
        ctl.set_clock_mode(ClockMode::Follow(Precision::Exact), 70_000);
        assert!(ctl.is_locked());
        assert!((ctl.beat_at(70_000) - before).abs() < 1e-9);
        // And back to the internal clock at the followed tempo.
        let before = ctl.beat_at(80_000);
        ctl.set_clock_mode(ClockMode::Internal, 80_000);
        assert!((ctl.beat_at(80_000) - before).abs() < 1e-9);
        assert!((ctl.tempo() - 128.0).abs() < 1e-9);
    }

    #[test]
    fn small_corrections_never_flush() {
        let (mut ctl, mut c) = control();
        ctl.set_pattern(sixteenths());
        ctl.set_clock_mode(ClockMode::Follow(Precision::Fine), 0);
        ctl.observe(&bar_obs(0.0, 0.0, 120.0), 0);
        ctl.start(0);
        let mut now = 0u64;
        // A source 0.3 % fast with 1 ms of alternating jitter.
        for k in 1..40u32 {
            let jitter = if k % 2 == 0 { 48.0 } else { -48.0 };
            let at = f64::from(k) * 24_000.0 / 1.003 + jitter;
            while (now as f64) < at + 100.0 {
                ctl.tick(now);
                now += 256;
            }
            ctl.observe(&bar_obs(at, f64::from(k), 120.0 * 1.003), now);
        }
        let events = drain(&mut c);
        assert!(events.iter().all(|e| !matches!(e.kind, EventKind::Flush)));
        let samples = trigger_samples(&events);
        assert!(samples.windows(2).all(|w| w[1] > w[0]), "strictly increasing");
        assert_eq!(samples.len() as u64, ctl.scheduler().next_step());
    }

    #[test]
    fn a_snap_flushes_and_never_replays_a_step() {
        let (mut ctl, mut c) = control();
        ctl.set_pattern(sixteenths());
        ctl.set_clock_mode(ClockMode::Follow(Precision::Fine), 0);
        ctl.observe(&bar_obs(0.0, 0.0, 120.0), 0);
        ctl.start(0);
        ctl.tick(0);
        let first = trigger_samples(&drain(&mut c));
        assert_eq!(first, vec![0]);
        // Resync to a source 40 ms behind us (our step at 6 000 has played
        // when the snap arrives at 7 000; on the new grid it would be at
        // 7 920).
        ctl.tick(6_000);
        drain(&mut c);
        ctl.resync(7_000);
        ctl.observe(&bar_obs(1_920.0, 0.0, 120.0), 7_000);
        ctl.tick(7_000);
        ctl.tick(10_000);
        let events = drain(&mut c);
        assert!(matches!(events[0].kind, EventKind::Flush));
        assert_eq!(events[0].sample, 7_000);
        let samples = trigger_samples(&events);
        // Step 1 is not played again; step 2 lands on the new grid.
        assert_eq!(samples, vec![1_920 + 12_000]);
    }

    #[test]
    fn a_forward_snap_plays_a_just_missed_step_late() {
        let (mut ctl, mut c) = control();
        ctl.set_pattern(sixteenths());
        ctl.set_clock_mode(ClockMode::Follow(Precision::Fine), 0);
        ctl.observe(&bar_obs(0.0, 0.0, 120.0), 0);
        ctl.start(0);
        ctl.tick(0);
        drain(&mut c);
        // At 5 000 the source turns out to be 31 ms ahead: step 1, at 6 000
        // on the old grid, is at 4 480 on the new one, already behind us.
        ctl.resync(5_000);
        ctl.observe(&bar_obs(5_000.0, 0.25 + 520.0 / 24_000.0, 120.0), 5_000);
        ctl.tick(5_000);
        let samples = trigger_samples(&drain(&mut c));
        assert_eq!(samples, vec![4_480], "played 520 samples late, not skipped");
        ctl.tick(6_000);
        assert_eq!(trigger_samples(&drain(&mut c)), vec![10_480]);
    }

    #[test]
    fn midi_start_puts_the_downbeat_on_the_first_pulse() {
        let (mut ctl, _c) = control();
        ctl.set_clock_mode(ClockMode::Follow(Precision::Jittery), 0);
        // Clock running while the source is stopped: tempo only.
        let mut t = 0.0;
        for _ in 0..100 {
            ctl.midi(MidiMessage::Clock, t, t as u64);
            ctl.tick(t as u64);
            t += 1_000.0; // 120 BPM
        }
        assert!(ctl.is_locked());
        assert!((ctl.tempo() - 120.0).abs() < 1e-6);
        // Start, then the first pulse at a sample that is not on our grid.
        let first = t + 333.0;
        ctl.midi(MidiMessage::Start, first - 500.0, (first - 500.0) as u64);
        for k in 0..48 {
            let at = first + f64::from(k) * 1_000.0;
            ctl.midi(MidiMessage::Clock, at, at as u64);
            ctl.tick(at as u64);
        }
        let beat = ctl.beat_at(first as u64);
        assert!(
            (beat - beat.round()).abs() < 1e-3 && beat.round().rem_euclid(4.0) == 0.0,
            "{beat}"
        );
    }
}
