use std::sync::Arc;

use dsp::{Param, VoiceParams, VOICE_COUNT};
use sequencer::queue::Producer;
use sequencer::{
    Event, MasterParam, ParamTarget, Pattern, Scheduler, VoiceId, VoiceParam, FLUSH_RESERVE,
};
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

/// Render blocks a realign keeps clear of when the control half runs on
/// its own thread: the one the renderer is playing and, since the flush
/// only reaches it at its next pull, the one it may start meanwhile.
const COMMITTED_BLOCKS: u64 = 2;

/// Steps remembered with the sample each was queued at. A 100 ms lookahead
/// holds at most three steps even at the top tempo; the rest is margin for
/// longer lookaheads.
const QUEUED_MEMORY: usize = 32;

/// One scheduled step and where it was queued.
#[derive(Clone, Copy, Debug, Default)]
struct Queued {
    step: u64,
    /// Its earliest event: the grace note of a flam, else the grid hit.
    first: u64,
    /// Its grid hit.
    sample: u64,
}

/// The most recently scheduled steps in a fixed ring, oldest first, with
/// the samples they were actually queued at. A following timeline is
/// re-planned at every report, so after a snap only this record (not the
/// timeline) can tell which queued steps were already heard.
#[derive(Clone, Debug)]
struct QueuedLog {
    entries: [Queued; QUEUED_MEMORY],
    /// Next slot to write.
    head: usize,
    len: usize,
}

impl QueuedLog {
    const fn new() -> Self {
        Self {
            entries: [Queued {
                step: 0,
                first: 0,
                sample: 0,
            }; QUEUED_MEMORY],
            head: 0,
            len: 0,
        }
    }

    fn push(&mut self, queued: Queued) {
        self.entries[self.head] = queued;
        self.head = (self.head + 1) % QUEUED_MEMORY;
        self.len = (self.len + 1).min(QUEUED_MEMORY);
    }

    /// The `i`-th newest entry (`0` = newest); `i < len`.
    fn newest(&self, i: usize) -> Queued {
        self.entries[(self.head + 2 * QUEUED_MEMORY - 1 - i) % QUEUED_MEMORY]
    }

    /// Forgets the steps whose events a flush at `sample` dropped (all of
    /// them: see [`QueuedLog::cut`]). Queued samples only grow, so they are
    /// the newest entries.
    fn flush_from(&mut self, sample: u64) {
        while self.len > 0 && self.newest(0).first >= sample {
            self.head = (self.head + QUEUED_MEMORY - 1) % QUEUED_MEMORY;
            self.len -= 1;
        }
    }

    /// Splits the queued steps at `commit`, the first sample a flush can
    /// still take back, into steps that are heard whole and steps that are
    /// dropped whole. A step whose grace note falls before `commit` but
    /// whose grid hit does not is heard whole: the flush moves past its hit
    /// rather than leave a grace note that the replayed step would sound a
    /// second time.
    fn cut(&self, commit: u64) -> Cut {
        let mut cut = Cut {
            flush: commit,
            heard: None,
            dropped: None,
        };
        for i in (0..self.len).rev() {
            let q = self.newest(i);
            if q.first < cut.flush {
                cut.flush = cut.flush.max(q.sample + 1);
                cut.heard = Some(q);
            } else {
                cut.dropped = Some(q.step);
                break;
            }
        }
        cut
    }
}

/// Where a realign cuts the queued steps ([`QueuedLog::cut`]).
#[derive(Clone, Copy, Debug)]
struct Cut {
    /// The sample to flush from.
    flush: u64,
    /// The newest step heard whole.
    heard: Option<Queued>,
    /// The first step the flush drops, if it drops any.
    dropped: Option<u64>,
}

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
    queued: QueuedLog,
    /// Newest render position seen in [`SharedTiming`].
    published: u64,
    /// Smallest step between two published render positions: the render
    /// block (or more, if the control half looks less often). `0` until
    /// two have been seen.
    render_block: u64,
    /// A flush that did not fit in the queue. It is retried before
    /// anything else is scheduled, so no step of the new timeline can be
    /// queued in front of it (and then dropped by it).
    pending_flush: Option<u64>,
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
            queued: QueuedLog::new(),
            published: 0,
            render_block: 0,
            pending_flush: None,
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
    /// the commit point ([`Control::commit_point`]: `now` in lockstep, the
    /// first sample the renderer has not committed to when it runs on its
    /// own thread), after any latency compensation, so the downbeat sounds
    /// on time. Following: joins the external timeline in phase at the
    /// next step, so the pattern position matches the source's bar
    /// position.
    ///
    /// A start shortly after a stop (or while playing) must not let what is
    /// still queued from before play on top of the new run: those steps are
    /// flushed. Following, the new run also begins at least half a step
    /// after the last step heard, so no step sounds twice. A stop-after
    /// count ([`Control::set_stop_after`]) counts from the first step of
    /// the new run.
    pub fn start(&mut self, now: u64) {
        let commit = self.commit_point(now);
        match self.mode {
            ClockMode::Internal => {
                self.drop_queued_from(commit);
                self.restart_internal_at(commit);
                self.scheduler.start();
            }
            ClockMode::Follow(_) => {
                let cut = self.queued.cut(commit);
                if cut.dropped.is_some() {
                    self.send_flush(cut.flush);
                }
                self.queued.flush_from(cut.flush);
                let mut from = commit;
                if let Some(heard) = cut.heard {
                    from = from.max(self.half_step_after(heard.sample as f64));
                }
                let active = self.active();
                let clock = AdjustedClock::new(&active, self.controls);
                let step = self.scheduler.first_step_at_or_after(&clock, from);
                self.scheduler.start_at(step);
            }
        }
    }

    /// Stops scheduling. Queued events still play.
    pub fn stop(&mut self) {
        self.scheduler.stop();
    }

    /// Stops automatically after `steps` steps (`None` loops forever),
    /// counted from the first step of the next start, or from the next
    /// unscheduled step if set while playing. Realigns keep the count of
    /// steps still to play.
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
    ///
    /// Song Position Pointer is not supported (`MidiMessage` has no variant
    /// for it; see `docs/protocols/midi-clock.md`): after an SPP and a
    /// Continue the bar phase continues from our own pulse count, so a
    /// source that relocates mid-bar is followed a fraction of a bar off
    /// until its next Start. The realign that Continue triggers still never
    /// doubles or drops a step.
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
    /// beat grid onto the tap; a running pattern continues on the new grid
    /// without repeating the step just heard (as after a follower snap).
    /// Following: treated as an observation.
    pub fn tap(&mut self, sample: f64, now: u64) {
        let Some(obs) = self.tap.tap(sample) else {
            return;
        };
        match self.mode {
            ClockMode::Internal => {
                let before = self.internal;
                if let Some(bpm) = obs.bpm {
                    self.internal.set_tempo(bpm, sample);
                }
                let beat = self.internal.beat_at_sample(sample);
                self.internal.align(sample, beat.round());
                if self.scheduler.is_playing() {
                    let commit = self.commit_point(now);
                    self.realign_continuous(commit, &before);
                }
            }
            ClockMode::Follow(_) => self.observe(&obs, now),
        }
    }

    /// Quantized re-sync. Following: the next observation snaps phase
    /// instead of slewing. Internal: the bar restarts at the commit point
    /// (`now` in lockstep; see [`Control::start`]).
    pub fn resync(&mut self, now: u64) {
        match self.mode {
            ClockMode::Internal => {
                let commit = self.commit_point(now);
                if !self.scheduler.is_playing() {
                    self.restart_internal_at(commit);
                    return;
                }
                let unplayed = self.drop_queued_from(commit);
                self.restart_internal_at(commit);
                let active = self.active();
                let clock = AdjustedClock::new(&active, self.controls);
                let step = self.scheduler.first_step_at_or_after(&clock, commit);
                self.scheduler.seek(step, unplayed);
            }
            ClockMode::Follow(_) => self.follower.request_resync(),
        }
    }

    /// Puts the internal clock's beat 0 where step 0 will sound at
    /// `sample`: latency compensation and nudge shift the steps against the
    /// beat grid, and a restart is about when the downbeat is heard.
    fn restart_internal_at(&mut self, sample: u64) {
        let spb = self.internal.samples_per_beat();
        let latency = self.controls.latency_ms * f64::from(self.sample_rate) / 1_000.0;
        let anchor = sample as f64 + latency - self.controls.nudge_beats * spb;
        self.internal.align(anchor, 0.0);
        // Sub-sample rounding must not push step 0 before `sample`.
        let active = self.active();
        let clock = AdjustedClock::new(&active, self.controls);
        let first = self.scheduler.step_sample(&clock, 0);
        if first < sample {
            self.internal.align(anchor + (sample - first) as f64, 0.0);
        }
    }

    /// For a restart: flushes every step queued from `commit` on (a step
    /// whose grace note was already heard loses its hit, which leaves the
    /// grace note as a pickup into the new downbeat rather than a second
    /// hit just after it). Returns the first step dropped, or the next
    /// unscheduled one.
    fn drop_queued_from(&mut self, commit: u64) -> u64 {
        let mut dropped = None;
        for i in 0..self.queued.len {
            let q = self.queued.newest(i);
            if q.first < commit {
                break;
            }
            dropped = Some(q.step);
        }
        if dropped.is_some() {
            self.send_flush(commit);
        }
        self.queued.flush_from(commit);
        dropped.unwrap_or(self.scheduler.next_step())
    }

    /// [`Control::realign_continuous`] after a follower snap.
    fn realign_after_snap(&mut self, now: u64, before: &FollowerClock) {
        if !self.scheduler.is_playing() {
            return;
        }
        let commit = self.commit_point(now);
        self.realign_continuous(commit, before);
    }

    /// After the timeline moved under a running pattern (a follower snap,
    /// a tap): flush what is queued from the commit point on and continue
    /// on the new timeline. Nothing is heard twice: the steps already heard
    /// (queued before `commit`; with a flam, if its grace note was) stay as
    /// they were, and nothing new lands within half a step after the last
    /// of them (a small jump either way, or a forward jump of a whole
    /// number of steps, would otherwise repeat that same musical moment).
    /// Within that limit a step the jump left just behind `commit` (by up
    /// to [`SNAP_GRACE_S`]) is played late instead of skipped, and after a
    /// jump back the new timeline's steps play at once: the source replays
    /// what it jumped back over, and so do we, instead of falling silent
    /// until our old step number comes round again.
    ///
    /// What was heard comes from [`QueuedLog`], the samples steps were
    /// actually queued at: the follower re-plans its timeline at every
    /// report, so by now even the pre-snap timeline (`before`) can put a
    /// queued step on the other side of `commit`. `before` is only the
    /// fallback when the log does not reach back to `commit`.
    ///
    /// "Already heard" means queued before the commit point
    /// ([`Control::commit_point`]): `now` when control and render run in
    /// lockstep, a little later when the renderer runs on its own thread
    /// and may already be playing past `now`.
    fn realign_continuous<C: ClockSource>(&mut self, commit: u64, before: &C) {
        let cut = self.queued.cut(commit);
        self.send_flush(cut.flush);
        let unplayed = cut.dropped.unwrap_or(self.scheduler.next_step());
        let mut heard = cut.heard.map(|q| q.sample as f64);
        if heard.is_none() && self.queued.len == QUEUED_MEMORY {
            // Everything remembered is still ahead: estimate the last step
            // heard from the old timeline.
            let old = AdjustedClock::new(before, self.controls);
            let first_unplayed = unplayed.min(self.scheduler.first_step_at_or_after(&old, commit));
            if let Some(last) = first_unplayed.checked_sub(1) {
                heard = Some(old.sample_at_beat(self.scheduler.pattern().step_beat(last)));
            }
        }
        self.queued.flush_from(cut.flush);
        let grace = (SNAP_GRACE_S * f64::from(self.sample_rate)).round() as u64;
        let mut from = commit.saturating_sub(grace);
        if let Some(heard) = heard {
            from = from.max(self.half_step_after(heard));
        }
        let active = self.active();
        let new = AdjustedClock::new(&active, self.controls);
        let step = self.scheduler.first_step_at_or_after(&new, from);
        self.scheduler.seek(step, unplayed);
    }

    /// The first sample at least half a step (on the active clock) after
    /// `heard`.
    fn half_step_after(&self, heard: f64) -> u64 {
        let half_step = 0.5 * sequencer::BEATS_PER_STEP * self.active().samples_per_beat();
        (heard + half_step).ceil().max(0.0) as u64
    }

    /// Queues a flush from `at`, or keeps it for the next tick if the queue
    /// is full (it cannot be while every other producer honours
    /// [`FLUSH_RESERVE`], unless several realigns pile up before the
    /// renderer pulls). Two pending flushes merge into the earlier one.
    fn send_flush(&mut self, at: u64) {
        let at = self.pending_flush.take().map_or(at, |p| p.min(at));
        if self.producer.push(Event::flush(at)).is_err() {
            self.pending_flush = Some(at);
        }
    }

    /// Pushes a non-flush event unless that would eat into the slots kept
    /// free for a flush.
    fn push_reserved(&mut self, event: Event) -> bool {
        self.producer.vacant() > FLUSH_RESERVE && self.producer.push(event).is_ok()
    }

    /// Learns the render block from the positions the renderer publishes.
    fn note_render_position(&mut self) -> u64 {
        let position = self.timing.read().position;
        if position > self.published {
            let block = position - self.published;
            if self.published > 0 || self.render_block > 0 {
                self.render_block = if self.render_block == 0 {
                    block
                } else {
                    self.render_block.min(block)
                };
            }
            self.published = position;
        } else if position < self.published {
            // The renderer was recreated or rewound.
            self.published = position;
        }
        position
    }

    /// The first sample a flush issued at `now` can still take back. In
    /// lockstep (`Engine::render`: tick, then render) the renderer has not
    /// pulled anything past `now`, so that is `now`. With control and
    /// render on separate threads, `now` is the renderer's published block
    /// start: it has already pulled and is playing that block, and the
    /// flush reaches it only at its next pull, so the steps up to
    /// [`COMMITTED_BLOCKS`] blocks on are as good as heard.
    fn commit_point(&mut self, now: u64) -> u64 {
        let published = self.note_render_position();
        if now > published {
            return now;
        }
        let ahead = (COMMITTED_BLOCKS * self.render_block).min(self.lookahead / 2);
        (published + ahead).max(now)
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
        let ok = self.push_reserved(Event::param(at, ParamTarget::Voice(voice, param), value));
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
        let ok = self.push_reserved(Event::param(
            at,
            ParamTarget::Master(MasterParam::OutputGain),
            gain,
        ));
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
        let ok = self.push_reserved(Event::param(
            at,
            ParamTarget::Master(MasterParam::Limiter),
            value,
        ));
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
        self.note_render_position();
        if matches!(self.mode, ClockMode::Follow(_)) {
            let before = self.follower.clone();
            self.follower.advance(now as f64);
            if self.follower.take_discontinuity() {
                // Not expected (advance only slews), but if the follower
                // ever jumps here it gets the same no-replay realign.
                self.realign_after_snap(now, &before);
            }
        }
        if let Some(at) = self.pending_flush.take() {
            self.send_flush(at);
            if self.pending_flush.is_some() {
                return 0;
            }
        }
        let active = active_of(self.mode, &self.internal, &self.follower);
        let clock = AdjustedClock::new(&active, self.controls);
        let first = self.scheduler.next_step();
        let pushed = self
            .scheduler
            .schedule(&clock, now + self.lookahead, &mut self.producer);
        // Remember where the new steps went (the scheduler's own rounding),
        // for realigning after a snap.
        let end = self.scheduler.next_step();
        for step in first.max(end.saturating_sub(QUEUED_MEMORY as u64))..end {
            let (first, sample) = self.scheduler.step_span(&clock, step);
            self.queued.push(Queued {
                step,
                first,
                sample,
            });
        }
        pushed
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
        // Queues the kick on beat 1 (24 000).
        ctl.tick(20_000);
        drain(&mut c);
        ctl.resync(22_000);
        ctl.tick(22_000);
        let events = drain(&mut c);
        assert!(matches!(events[0].kind, EventKind::Flush));
        assert_eq!(events[0].sample, 22_000);
        assert_eq!(events[1].sample, 22_000);
        // Stopped, a re-sync only moves the bar; nothing is queued.
        ctl.stop();
        ctl.resync(30_000);
        ctl.tick(30_000);
        assert!(drain(&mut c).is_empty());
        assert!(ctl.beat_at(30_000).abs() < 1e-12);
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
        assert!(
            samples.windows(2).all(|w| w[1] > w[0]),
            "strictly increasing"
        );
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

    /// What the renderer would play from `events`: triggers in order, with
    /// every flush dropping the triggers at or after its sample.
    fn heard(events: &[Event]) -> Vec<u64> {
        let mut out: Vec<u64> = Vec::new();
        for e in events {
            match e.kind {
                EventKind::Trigger { .. } => out.push(e.sample),
                EventKind::Flush => out.retain(|&s| s < e.sample),
                EventKind::Param { .. } => {}
            }
        }
        out.sort_unstable();
        out
    }

    /// A slew re-plans the timeline after a step was queued, then a resync
    /// snaps: the realign must judge "already played" by where the step was
    /// actually queued, not by where the re-planned timeline puts it now.
    fn slew_then_snap(ahead_beats: f64, snap_at: u64) -> Vec<u64> {
        let (mut ctl, mut c) = control();
        ctl.set_pattern(sixteenths());
        ctl.set_clock_mode(ClockMode::Follow(Precision::Fine), 0);
        ctl.observe(&bar_obs(0.0, 0.0, 120.0), 0);
        ctl.start(0);
        let mut events = Vec::new();
        ctl.tick(0);
        // Queues step 1 at 6 000.
        ctl.tick(1_300);
        events.extend(drain(&mut c));
        // The source is a little off: a slew, no snap.
        let ours = ctl.follower_clock().beat_at_sample(1_300.0);
        ctl.observe(&bar_obs(1_300.0, ours + ahead_beats, 120.0), 1_300);
        events.extend(drain(&mut c));
        assert!(events.iter().all(|e| !matches!(e.kind, EventKind::Flush)));
        // A resync with a source right on our timeline: a (tiny) snap.
        ctl.resync(snap_at);
        let ours = ctl.follower_clock().beat_at_sample(snap_at as f64);
        ctl.observe(&bar_obs(snap_at as f64, ours + 0.000_1, 120.0), snap_at);
        let mut now = snap_at;
        while now < 40_000 {
            ctl.tick(now);
            now += 128;
        }
        events.extend(drain(&mut c));
        heard(&events)
    }

    fn assert_steady(hits: &[u64]) {
        for w in hits.windows(2) {
            let d = w[1] - w[0];
            assert!((4_500..=7_500).contains(&d), "hits {hits:?}");
        }
    }

    #[test]
    fn a_snap_does_not_drop_a_step_a_slew_moved_earlier() {
        // Step 1 was queued at 6 000; the slew moved it to about 5 830 on
        // the current plan; the snap at 5 900 flushes the queued one.
        let hits = slew_then_snap(0.04, 5_900);
        assert_eq!(hits[0], 0);
        assert_steady(&hits);
    }

    #[test]
    fn a_snap_does_not_double_a_step_a_slew_moved_later() {
        // Step 1 was queued (and played) at 6 000; the slew moved it to
        // about 6 190 on the current plan; the snap comes at 6 100.
        let hits = slew_then_snap(-0.04, 6_100);
        assert_eq!(hits[0], 0);
        assert_steady(&hits);
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

    /// Following, stop and start again within the lookahead (or press
    /// start while playing): the steps still queued from before must not
    /// play on top of the new run's.
    #[test]
    fn a_quick_restart_while_following_never_doubles_a_step() {
        for (stop_at, start_at) in [
            (Some(10_000), 10_500),
            (Some(10_000), 13_000),
            (None, 10_000),
        ] {
            let (mut ctl, mut c) = control();
            ctl.set_pattern(sixteenths());
            ctl.set_clock_mode(ClockMode::Follow(Precision::Fine), 0);
            ctl.observe(&bar_obs(0.0, 0.0, 120.0), 0);
            ctl.start(0);
            let mut events = Vec::new();
            let mut now = 0;
            while now < 40_000 {
                if Some(now) == stop_at {
                    ctl.stop();
                }
                if now == start_at {
                    ctl.start(now);
                }
                ctl.tick(now);
                events.extend(drain(&mut c));
                now += 500;
            }
            let hits = heard(&events);
            assert_eq!(hits.first(), Some(&0));
            for w in hits.windows(2) {
                assert_eq!(w[1] - w[0], 6_000, "{stop_at:?} {start_at}: {hits:?}");
            }
        }
    }

    /// What the renderer would play: `(sample, voice, velocity)` triggers
    /// in time order, every flush dropping the triggers at or after its
    /// sample that were queued before it.
    fn heard_triggers(events: &[Event]) -> Vec<(u64, VoiceId, f32)> {
        let mut out = Vec::new();
        for e in events {
            match e.kind {
                EventKind::Trigger { voice, velocity } => out.push((e.sample, voice, velocity)),
                EventKind::Flush => out.retain(|t: &(u64, VoiceId, f32)| t.0 < e.sample),
                EventKind::Param { .. } => {}
            }
        }
        out.sort_by_key(|t| t.0);
        out
    }

    /// Following with a one-bar limit, joining the source mid-song (at an
    /// absolute step far from 0), plays exactly one bar.
    #[test]
    fn following_with_stop_after_plays_one_bar_from_the_join() {
        let (mut ctl, mut c) = control();
        ctl.set_pattern(sixteenths());
        ctl.set_clock_mode(ClockMode::Follow(Precision::Exact), 0);
        // The source is at beat 13.3 at sample 0: we join at step 54.
        ctl.observe(&bar_obs(0.0, 13.3, 120.0), 0);
        ctl.set_stop_after(Some(16));
        ctl.start(0);
        let mut events = Vec::new();
        let mut now = 0;
        while now < 200_000 {
            ctl.tick(now);
            events.extend(drain(&mut c));
            now += 512;
        }
        assert_eq!(heard(&events).len(), 16);
        assert!(!ctl.is_playing());
    }

    fn flams() -> Pattern {
        let mut p = Pattern::empty();
        *p.track_mut(VoiceId::Snare) = Track::parse("ffff ffff ffff ffff").unwrap();
        p.flam = 1.0; // 40 ms = 1 920 samples
        p
    }

    /// Every grace note is followed by exactly one main hit of its voice,
    /// at most `flam` samples later, and every main hit has exactly one
    /// grace.
    fn assert_flams_whole(hits: &[(u64, VoiceId, f32)], flam: u64) {
        let graces: Vec<u64> = hits.iter().filter(|h| h.2 < 0.65).map(|h| h.0).collect();
        let mains: Vec<u64> = hits.iter().filter(|h| h.2 >= 0.65).map(|h| h.0).collect();
        assert_eq!(
            graces.len(),
            mains.len(),
            "graces {graces:?} mains {mains:?}"
        );
        for (g, m) in graces.iter().zip(&mains) {
            assert!(m >= g && m - g <= flam, "grace {g} main {m}: {hits:?}");
        }
    }

    /// A snap whose commit point falls between a step's grace note (heard)
    /// and its main hit (not yet): the step is kept whole, never replayed
    /// with a second grace note.
    #[test]
    fn a_snap_between_grace_and_hit_never_repeats_the_grace() {
        for ahead in [0.000_1, 0.02, -0.02] {
            let (mut ctl, mut c) = control();
            ctl.set_pattern(flams());
            ctl.set_clock_mode(ClockMode::Follow(Precision::Fine), 0);
            ctl.observe(&bar_obs(0.0, 0.0, 120.0), 0);
            ctl.start(0);
            let mut events = Vec::new();
            ctl.tick(0);
            // Queues step 1: grace at 4 080, hit at 6 000.
            ctl.tick(1_300);
            events.extend(drain(&mut c));
            ctl.resync(5_000);
            let ours = ctl.follower_clock().beat_at_sample(5_000.0);
            ctl.observe(&bar_obs(5_000.0, ours + ahead, 120.0), 5_000);
            let mut now = 5_000;
            while now < 60_000 {
                ctl.tick(now);
                now += 128;
            }
            events.extend(drain(&mut c));
            let hits = heard_triggers(&events);
            assert_flams_whole(&hits, 1_920);
            let mains: Vec<u64> = hits.iter().filter(|h| h.2 >= 0.65).map(|h| h.0).collect();
            assert_eq!(&mains[..2], &[0, 6_000], "{ahead}");
            for w in mains.windows(2) {
                assert!(w[1] - w[0] >= 3_000, "{ahead}: {mains:?}");
            }
        }
    }

    /// Parameter changes fill the queue: a re-sync's flush must still get
    /// through, or the old timeline's steps play on top of the new one's.
    #[test]
    fn a_flush_gets_through_a_queue_full_of_param_changes() {
        let (p, mut c) = event_queue(64);
        let mut ctl = Control::new(48_000.0, p, Arc::new(SharedTiming::default()));
        ctl.set_pattern(sixteenths());
        ctl.start(0);
        ctl.tick(0);
        let mut events = drain(&mut c);
        ctl.tick(2_000); // queues step 1 at 6 000
        let mut value = 0.0;
        while ctl.set_voice_param(VoiceId::Kick, VoiceParam::Tune, value, 2_000) {
            value += 0.001;
        }
        ctl.resync(3_000);
        events.extend(drain(&mut c));
        let mut now = 3_000;
        while now < 30_000 {
            ctl.tick(now);
            events.extend(drain(&mut c));
            now += 128;
        }
        let hits = heard(&events);
        assert_eq!(&hits[..3], &[0, 3_000, 9_000], "{hits:?}");
    }

    /// Re-sync pressed again and again while the renderer is stalled (no
    /// pulls): flushes pile up past the reserve. A flush that does not fit
    /// waits, and nothing is scheduled in front of it.
    #[test]
    fn flushes_that_do_not_fit_are_retried_before_anything_else() {
        let (p, mut c) = event_queue(64);
        let mut ctl = Control::new(48_000.0, p, Arc::new(SharedTiming::default()));
        ctl.set_pattern(sixteenths());
        ctl.start(0);
        ctl.tick(0);
        for _ in 0..200 {
            ctl.resync(1_000);
            ctl.tick(1_000);
        }
        let mut events = drain(&mut c);
        let mut now = 1_000;
        while now < 30_000 {
            ctl.tick(now);
            events.extend(drain(&mut c));
            now += 128;
        }
        let hits = heard(&events);
        assert_eq!(&hits[..4], &[0, 1_000, 7_000, 13_000], "{hits:?}");
    }

    /// The split API: control reads the renderer's published block start,
    /// and the renderer has already pulled (and is playing) that block. An
    /// internal re-sync restarts the bar at the commit point, not at the
    /// stale block start, so the new downbeat is neither late nor laid on
    /// top of a step that was already heard.
    #[test]
    fn split_internal_resync_restarts_at_the_commit_point() {
        let timing = Arc::new(SharedTiming::default());
        let (p, mut c) = event_queue(1_024);
        let mut ctl = Control::new(48_000.0, p, Arc::clone(&timing));
        ctl.set_pattern(sixteenths());
        ctl.start(0);
        let mut events = Vec::new();
        for k in 0..=20u64 {
            timing.publish(k * 512, 0);
            ctl.tick_shared();
        }
        events.extend(drain(&mut c));
        // Published 10 240; the renderer may play up to 11 264 before it
        // sees a flush.
        ctl.resync(timing.read().position);
        ctl.tick_shared();
        let after = drain(&mut c);
        assert!(matches!(after[0].kind, EventKind::Flush));
        assert_eq!(after[0].sample, 11_264);
        assert_eq!(trigger_samples(&after)[0], 11_264);
        events.extend(after);
        for k in 21..=40u64 {
            timing.publish(k * 512, 0);
            ctl.tick_shared();
        }
        events.extend(drain(&mut c));
        assert_eq!(&heard(&events)[..5], &[0, 6_000, 11_264, 17_264, 23_264]);
    }

    /// Internal clock, playing: a tap pulls the grid onto a beat that lies
    /// after a step that was already heard. That step is not played again.
    #[test]
    fn an_internal_tap_never_doubles_a_step() {
        let (mut ctl, mut c) = control();
        ctl.set_pattern(sixteenths());
        ctl.start(0);
        let mut events = Vec::new();
        let mut now = 0;
        // Taps 30 ms behind the beat, each processed as it happens.
        let taps = [49_440u64, 73_440, 97_440, 121_440];
        while now < 160_000 {
            for &t in &taps {
                if (now..now + 128).contains(&t) {
                    ctl.tap(t as f64, t);
                }
            }
            ctl.tick(now);
            events.extend(drain(&mut c));
            now += 128;
        }
        let hits = heard(&events);
        for w in hits.windows(2) {
            assert!(w[1] - w[0] >= 3_000, "{hits:?}");
            assert!(w[1] - w[0] <= 9_000, "{hits:?}");
        }
        // And the grid did move onto the taps.
        assert!(hits.contains(&(121_440 + 6_000)), "{hits:?}");
    }

    /// Internal clock: pressing start while playing, or stop and start
    /// within the lookahead, restarts the bar without the old run's queued
    /// steps playing on top of the new run's.
    #[test]
    fn an_internal_restart_drops_the_old_runs_queued_steps() {
        for stop in [false, true] {
            let (mut ctl, mut c) = control();
            ctl.set_pattern(sixteenths());
            ctl.start(0);
            let mut events = Vec::new();
            let mut now = 0;
            while now < 40_000 {
                if now == 9_984 {
                    if stop {
                        ctl.stop();
                    }
                    ctl.start(now);
                }
                ctl.tick(now);
                events.extend(drain(&mut c));
                now += 128;
            }
            let hits = heard(&events);
            assert_eq!(&hits[..5], &[0, 6_000, 9_984, 15_984, 21_984], "{hits:?}");
        }
    }
}
