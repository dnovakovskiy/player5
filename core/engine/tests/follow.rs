//! The engine in follow mode against simulated sources: a minute of audio
//! per scenario, every trigger the control half emits recorded and checked
//! against the source's true beat grid. Deterministic (seeded xorshift
//! jitter, no wall clock).
//!
//! The rig is `Engine::render` with a tap: `Control::tick` at the render
//! position, then `Renderer::process`, with every event recorded on its way
//! through the queue. Flushes are applied to the record exactly as the
//! renderer applies them, so the record is what was heard.

use std::sync::Arc;

use engine::{ClockMode, Control, Engine, PatternSpec, Renderer, SharedTiming};
use sequencer::queue::{event_queue, Consumer, Producer};
use sequencer::{EventKind, Pattern, Track, VoiceId};
use sync::{MidiMessage, Observation, Phase, Precision};

const SR: f64 = 48_000.0;
/// Render block (the web worklet's quantum).
const BLOCK: usize = 128;

/// Deterministic xorshift64 noise.
struct Rng(u64);

impl Rng {
    fn unit(&mut self) -> f64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform in `[-a, a)`.
    fn sym(&mut self, a: f64) -> f64 {
        (2.0 * self.unit() - 1.0) * a
    }
}

/// How the simulated source reports.
#[derive(Clone, Copy, PartialEq)]
enum Feed {
    /// One bar-phase report per beat (Pro DJ Link beat packets, Opus Quad).
    Beats,
    /// MIDI clock: 24 pulses per beat after a Start.
    Midi,
    /// Bar-phase reports several times per beat, unrelated to the steps
    /// (precise position packets, Link).
    Position,
}

#[derive(Clone, Copy)]
struct Source {
    feed: Feed,
    precision: Precision,
    bpm: f64,
    /// True tempo on our sample clock = reported × (1 + skew).
    skew: f64,
    /// Uniform ± error on each report's sample position (seconds).
    jitter_s: f64,
    /// `(start_s, end_s, to_bpm)`: a pitch-fader move.
    ramp: Option<(f64, f64, f64)>,
    /// `(at_s, beats)`: the DJ cues away.
    jump: Option<(f64, f64)>,
    seconds: f64,
    /// When playback starts (the follower must be locked by then).
    start_s: f64,
    /// Press re-sync every this many seconds after the start (each one
    /// snaps on the next report: a stream of small snaps).
    resync_every: Option<f64>,
    /// `(period_s, length_s)`: delivery stalls for `length_s` at the start
    /// of every `period_s`; what was due arrives in one burst.
    stall: Option<(f64, f64)>,
    seed: u64,
}

impl Source {
    fn new(feed: Feed, precision: Precision) -> Self {
        Self {
            feed,
            precision,
            bpm: 124.0,
            skew: 50e-6,
            jitter_s: 0.003,
            ramp: None,
            jump: None,
            seconds: 60.0,
            start_s: 1.0,
            resync_every: None,
            stall: None,
            seed: 0x2545_F491_4F6C_DD1D,
        }
    }

    fn reported_bpm(&self, t: f64) -> f64 {
        match self.ramp {
            Some((a, b, to)) if t > a => self.bpm + (to - self.bpm) * ((t - a) / (b - a)).min(1.0),
            _ => self.bpm,
        }
    }
}

/// Control and renderer with a recording tap on the queue between them.
struct Rig {
    control: Control,
    renderer: Renderer,
    tap: Consumer,
    to_renderer: Producer,
    /// Triggers that were (or will be) heard: `(sample, voice)`.
    hits: Vec<(u64, VoiceId)>,
    flushes: Vec<u64>,
    peak: f32,
}

impl Rig {
    fn new() -> Self {
        let timing = Arc::new(SharedTiming::default());
        let (producer, tap) = event_queue(engine::EVENT_QUEUE_CAPACITY);
        let (to_renderer, consumer) = event_queue(engine::EVENT_QUEUE_CAPACITY);
        Self {
            control: Control::new(SR as f32, producer, Arc::clone(&timing)),
            renderer: Renderer::new(SR as f32, consumer, timing),
            tap,
            to_renderer,
            hits: Vec::new(),
            flushes: Vec::new(),
            peak: 0.0,
        }
    }

    fn now(&self) -> u64 {
        self.renderer.position()
    }

    fn forward(&mut self) {
        while let Some(e) = self.tap.pop() {
            match e.kind {
                EventKind::Trigger { voice, .. } => self.hits.push((e.sample, voice)),
                EventKind::Flush => {
                    self.flushes.push(e.sample);
                    self.hits.retain(|h| h.0 < e.sample);
                }
                EventKind::Param { .. } => {}
            }
            assert!(self.to_renderer.push(e).is_ok(), "renderer queue full");
        }
    }

    /// One `Engine::render`-style block.
    fn block(&mut self) {
        let now = self.now();
        self.control.tick(now);
        self.forward();
        let mut out = [0.0f32; BLOCK];
        self.renderer.process(&mut out);
        self.peak = out.iter().fold(self.peak, |p, s| p.max(s.abs()));
    }
}

/// Every step on the closed hat (so every step is one trigger), kick on the
/// beat (so bar alignment is visible).
fn pattern() -> Pattern {
    let mut p = Pattern::empty();
    *p.track_mut(VoiceId::ClosedHat) = Track::parse("xxxx xxxx xxxx xxxx").unwrap();
    *p.track_mut(VoiceId::Kick) = Track::parse("x--- ---- ---- ----").unwrap();
    p
}

/// The source's true beat at each block boundary, for checking hits.
struct Truth {
    beats: Vec<f64>,
}

impl Truth {
    fn beat_at(&self, sample: u64) -> f64 {
        let i = (sample as usize / BLOCK).min(self.beats.len() - 2);
        let frac = (sample as f64 - (i * BLOCK) as f64) / BLOCK as f64;
        self.beats[i] + (self.beats[i + 1] - self.beats[i]) * frac
    }
}

struct Outcome {
    hits: Vec<(u64, VoiceId)>,
    flushes: Vec<u64>,
    truth: Truth,
    start: u64,
    peak: f32,
    bpm: f64,
    /// The source's starting tempo (for converting errors to ms).
    source_bpm: f64,
    /// Re-sync presses during the run.
    resyncs: usize,
}

/// Runs a source for `src.seconds`; playback starts at `src.start_s`.
fn run(src: Source) -> Outcome {
    let mut rng = Rng(src.seed);
    let mut rig = Rig::new();
    rig.control.set_pattern(pattern());
    rig.control
        .set_clock_mode(ClockMode::Follow(src.precision), 0);
    // The source has been playing for a while: arbitrary phase at sample 0.
    let mut beat: f64 = 13.37;
    let mut truth = vec![beat];
    let mut pending: Vec<(f64, Report)> = Vec::new();
    // Reports are counted in integers (beats, or MIDI pulses) so the song
    // start falls exactly on a pulse.
    let per_beat = match src.feed {
        Feed::Beats => 1.0,
        Feed::Midi => 24.0,
        Feed::Position => 7.0,
    };
    let mut next_index = (beat * per_beat).ceil() as i64;
    // MIDI: the source's song starts (Start) about 0.8 s in, before our
    // own start; its pulse count, and so its bar phase, counts from there.
    let midi_origin = beat.ceil() + 1.0;
    let origin_index = (midi_origin * per_beat) as i64;
    let mut jumped = false;
    let mut start = None;
    let mut next_resync: Option<f64> = None;
    let mut resyncs = 0usize;
    while (rig.now() as f64) < src.seconds * SR {
        let now = rig.now() as f64;
        let t = now / SR;
        // Advance the source over this block (trapezoid; exact for ramps).
        let b0 = src.reported_bpm(t) * (1.0 + src.skew);
        let b1 = src.reported_bpm(t + BLOCK as f64 / SR) * (1.0 + src.skew);
        let step = 0.5 * (b0 + b1) / 60.0 * BLOCK as f64 / SR;
        let end = beat + step;
        while (next_index as f64 / per_beat) < end {
            let next_report = next_index as f64 / per_beat;
            let at = now + (next_report - beat) / step * BLOCK as f64;
            let sample = at + rng.sym(src.jitter_s) * SR;
            let latency = (0.002 + 0.004 * rng.unit()) * SR;
            let event = match src.feed {
                Feed::Beats | Feed::Position => Report::Obs(Observation {
                    sample,
                    phase: Phase::Bar(next_report.rem_euclid(4.0)),
                    bpm: Some(src.reported_bpm(at / SR)),
                }),
                Feed::Midi => {
                    if next_index == origin_index {
                        let start = at - 0.001 * SR;
                        pending.push((start, Report::Midi(MidiMessage::Start, start)));
                    }
                    Report::Midi(MidiMessage::Clock, sample)
                }
            };
            let mut due = at.max(sample) + latency;
            if let Some((period, length)) = src.stall {
                let phase = (due / SR).rem_euclid(period);
                if phase < length {
                    due += (length - phase) * SR;
                }
            }
            pending.push((due, event));
            next_index += 1;
        }
        beat = end;
        if let Some((at, by)) = src.jump {
            if !jumped && t >= at {
                jumped = true;
                beat += by;
                next_index = (beat * per_beat).ceil() as i64;
            }
        }
        truth.push(beat);
        // Deliver what has arrived, then render the block.
        let now = rig.now();
        pending.sort_by(|a, b| a.0.total_cmp(&b.0));
        while pending.first().is_some_and(|(due, _)| *due <= now as f64) {
            match pending.remove(0).1 {
                Report::Obs(obs) => rig.control.observe(&obs, now),
                Report::Midi(m, s) => rig.control.midi(m, s, now),
            }
        }
        if start.is_none() && t >= src.start_s {
            assert!(rig.control.is_locked(), "locked by {} s", src.start_s);
            rig.control.start(now);
            start = Some(now);
            next_resync = src.resync_every.map(|every| t + every);
        }
        if let Some(at) = next_resync {
            if t >= at {
                rig.control.resync(now);
                resyncs += 1;
                next_resync = src.resync_every.map(|every| at + every);
            }
        }
        rig.block();
    }
    // MIDI phase is relative to the song start, not the deck's beat count.
    if src.feed == Feed::Midi {
        for b in &mut truth {
            *b -= midi_origin;
        }
    }
    Outcome {
        hits: rig.hits,
        flushes: rig.flushes,
        truth: Truth { beats: truth },
        start: start.unwrap(),
        peak: rig.peak,
        bpm: rig.control.tempo(),
        source_bpm: src.bpm,
        resyncs,
    }
}

/// Something the simulated source delivers.
enum Report {
    Obs(Observation),
    Midi(MidiMessage, f64),
}

impl Outcome {
    /// `(time s, source step, error ms)` for every hat hit.
    fn hat_hits(&self) -> Vec<(f64, i64, f64)> {
        self.hits
            .iter()
            .filter(|(_, v)| *v == VoiceId::ClosedHat)
            .map(|&(s, _)| {
                let steps = self.truth.beat_at(s) * 4.0;
                let k = steps.round();
                let bpm = self.source_bpm;
                (s as f64 / SR, k as i64, (steps - k) / 4.0 * 60_000.0 / bpm)
            })
            .collect()
    }

    fn max_error_after(&self, t: f64) -> f64 {
        self.hat_hits()
            .iter()
            .filter(|h| h.0 >= t)
            .map(|h| h.2.abs())
            .fold(0.0, f64::max)
    }

    fn flushes_after_start(&self) -> usize {
        self.flushes.iter().filter(|f| **f > self.start).count()
    }

    /// Asserts every source step from the first hit on was heard exactly
    /// once (apart from `allowed_gaps` deliberate skips at jumps).
    fn assert_every_step_once(&self, allowed_gaps: usize) {
        let hits = self.hat_hits();
        assert!(hits.len() > 100, "{} hits", hits.len());
        let mut gaps = 0;
        for w in hits.windows(2) {
            let d = w[1].1 - w[0].1;
            assert!(d >= 1, "step {} doubled at {:.3} s", w[1].1, w[1].0);
            if d > 1 {
                gaps += 1;
            }
        }
        assert!(gaps <= allowed_gaps, "{gaps} skips");
    }

    /// No two hat hits closer than half a step in time: nothing doubled or
    /// flammed by a realign, whatever the source did.
    fn assert_no_double_in_time(&self) {
        let half_step = 0.5 * 0.25 * 60.0 * SR / self.source_bpm;
        let hats: Vec<u64> = self
            .hits
            .iter()
            .filter(|(_, v)| *v == VoiceId::ClosedHat)
            .map(|h| h.0)
            .collect();
        for w in hats.windows(2) {
            assert!(
                (w[1] - w[0]) as f64 > 0.8 * half_step,
                "hits {} samples apart at {:.3} s",
                w[1] - w[0],
                w[1] as f64 / SR
            );
        }
    }

    /// Kicks fall on source downbeats (not a beat away): our bar is the
    /// source's bar. Timing accuracy is checked separately.
    fn assert_bars_aligned(&self) {
        self.assert_bars_aligned_except(0, 0);
    }

    /// [`Outcome::assert_bars_aligned`], ignoring hits in `from..to`.
    fn assert_bars_aligned_except(&self, from: u64, to: u64) {
        for &(s, v) in &self.hits {
            if v == VoiceId::Kick && !(from..to).contains(&s) {
                let beat = self.truth.beat_at(s);
                let bar_pos = (beat - 4.0 * (beat / 4.0).round()).abs();
                assert!(bar_pos < 0.4, "kick at source beat {beat:.3}");
            }
        }
    }
}

#[test]
fn beat_packets_every_step_once_on_the_grid() {
    let mut src = Source::new(Feed::Beats, Precision::Fine);
    src.ramp = Some((25.0, 29.0, 127.0));
    let out = run(src);
    assert_eq!(out.flushes_after_start(), 0, "jitter never realigns");
    out.assert_every_step_once(0);
    out.assert_bars_aligned();
    let worst = out.max_error_after(2.0);
    assert!(worst < 4.0, "{worst:.3} ms");
    assert!((out.bpm - 127.0).abs() < 1e-9);
    assert!(out.peak > 0.01);
}

#[test]
fn midi_clock_every_step_once_on_the_grid() {
    let mut src = Source::new(Feed::Midi, Precision::Jittery);
    src.jitter_s = 0.001;
    src.ramp = Some((25.0, 29.0, 127.0));
    let out = run(src);
    // Start snapped before our own start; nothing realigns after it.
    assert_eq!(out.flushes_after_start(), 0);
    out.assert_every_step_once(0);
    out.assert_bars_aligned();
    let worst = out.max_error_after(4.0);
    assert!(worst < 2.0, "{worst:.3} ms");
    assert!((out.bpm - 127.0).abs() < 0.1, "{}", out.bpm);
}

#[test]
fn coarse_source_never_jumps() {
    let mut src = Source::new(Feed::Beats, Precision::Coarse);
    src.jitter_s = 0.2;
    // Coarse averages eight reports before its first lock.
    src.start_s = 6.0;
    src.seconds = 90.0;
    let mut src2 = src;
    src2.seed ^= 0xDEAD_BEEF;
    for src in [src, src2] {
        let out = run(src);
        assert_eq!(out.flushes_after_start(), 0, "the timeline never jumps");
        out.assert_every_step_once(0);
        out.assert_bars_aligned();
        let first = out.max_error_after(0.0);
        assert!(first < 120.0, "{first:.1} ms");
        let later = out.max_error_after(45.0);
        assert!(later < 50.0, "{later:.1} ms");
    }
}

#[test]
fn a_cue_jump_realigns_once() {
    let mut src = Source::new(Feed::Beats, Precision::Fine);
    src.jump = Some((30.0, 1.5));
    let out = run(src);
    assert_eq!(out.flushes_after_start(), 1, "one snap for one jump");
    // The jump skips the source's steps in between; nothing plays twice.
    out.assert_every_step_once(1);
    let snap = *out.flushes.last().unwrap() as f64 / SR;
    assert!(snap > 30.0 && snap < 32.0, "{snap}");
    let worst = out.max_error_after(snap + 0.1);
    assert!(worst < 4.0, "{worst:.3} ms");
    // Between the cue and the snap we are still on the old bar, by design.
    out.assert_bars_aligned_except((30.0 * SR) as u64, *out.flushes.last().unwrap());
}

/// Through the public `Engine` API: lock, playhead, lock loss.
#[test]
fn engine_api_follows_and_free_runs_after_loss() {
    let mut engine = Engine::new(SR as f32);
    let spec =
        PatternSpec::from_json(r#"{ "voices": { "kick": { "steps": "x---x---x---x---" } } }"#)
            .unwrap();
    engine.load_spec(&spec).unwrap();
    engine.set_clock_mode(ClockMode::Follow(Precision::Fine));
    assert!(!engine.is_locked());
    let mut rng = Rng(7);
    let bpm = 126.0;
    let spb = SR * 60.0 / bpm;
    let offset = 2.25; // source beat at sample 0
    let source_beat = |s: f64| offset + s / spb;
    let mut next = offset.ceil();
    let mut out = vec![0.0f32; BLOCK];
    let mut worst: f64 = 0.0;
    let mut started = false;
    while (engine.position() as f64) < 20.0 * SR {
        let now = engine.position() as f64;
        // Reports stop at 12 s.
        while now < 12.0 * SR && (next - offset) * spb + 0.004 * SR <= now {
            let at = (next - offset) * spb;
            engine.observe(&Observation {
                sample: at + rng.sym(0.002) * SR,
                phase: Phase::Bar(next.rem_euclid(4.0)),
                bpm: Some(bpm),
            });
            next += 1.0;
        }
        if !started && now >= SR {
            assert!(engine.is_locked());
            engine.start();
            started = true;
        }
        if started && now < 12.0 * SR {
            let ours = engine.beat();
            let mut err = (ours - source_beat(now)).rem_euclid(4.0);
            if err > 2.0 {
                err -= 4.0;
            }
            worst = worst.max(err.abs() * 60_000.0 / bpm);
            let step = (source_beat(now) * 4.0).floor() as usize % 16;
            if let Some(playing) = engine.playing_step() {
                let d = (playing + 16 - step) % 16;
                assert!(d == 0 || d == 1 || d == 15, "playhead {playing} vs {step}");
            }
        }
        engine.render(&mut out);
    }
    assert!(worst < 3.0, "{worst:.3} ms");
    // Eight seconds without reports: unlocked, still playing at the last
    // tempo.
    assert!(!engine.is_locked());
    assert!(engine.is_playing());
    assert!((engine.tempo() - bpm).abs() < 1e-9);
}

/// A DJ who keeps pressing re-sync: every press is a (tiny) snap, a flush
/// and a realign, and none of them may drop or double a step.
#[test]
fn a_resync_storm_never_drops_or_doubles_a_step() {
    for (feed, precision, bpm, jitter) in [
        (Feed::Beats, Precision::Fine, 124.0, 0.003),
        (Feed::Position, Precision::Exact, 200.0, 0.000_2),
        // Noisier than `Exact` promises: it slews at its full 5 % between
        // presses, so queued steps move by milliseconds before each snap,
        // and the snaps fall anywhere between steps.
        (Feed::Position, Precision::Exact, 174.0, 0.004),
        (Feed::Beats, Precision::Fine, 60.0, 0.003),
        (Feed::Midi, Precision::Jittery, 128.0, 0.001),
    ] {
        let mut src = Source::new(feed, precision);
        src.bpm = bpm;
        src.jitter_s = jitter;
        src.ramp = Some((20.0, 28.0, bpm * 1.0667));
        src.resync_every = Some(0.77);
        src.seconds = 40.0;
        let out = run(src);
        assert!(out.resyncs > 40);
        // Each press snaps once on the next report (two presses between
        // reports, at 60 BPM, snap together).
        let flushes = out.flushes_after_start();
        assert!(
            flushes <= out.resyncs && 4 * flushes >= 3 * out.resyncs,
            "{bpm}: {flushes} flushes for {} presses",
            out.resyncs
        );
        out.assert_every_step_once(0);
        out.assert_no_double_in_time();
        out.assert_bars_aligned();
    }
}

/// Congested network: delivery stalls for 300 ms every 2 s and the reports
/// arrive in bursts. Late is not wrong: nothing realigns.
#[test]
fn network_stalls_never_realign() {
    for precision in [Precision::Exact, Precision::Fine, Precision::Coarse] {
        let mut src = Source::new(Feed::Beats, precision);
        src.stall = Some((2.0, 0.3));
        if precision == Precision::Coarse {
            src.jitter_s = 0.2;
            src.start_s = 6.0;
        }
        let out = run(src);
        assert_eq!(out.flushes_after_start(), 0, "{precision:?}");
        out.assert_every_step_once(0);
        out.assert_bars_aligned();
    }
}

/// The DJ jumps back (a hot cue behind the playhead). One realign; nothing
/// is doubled or flammed, and after it every hit is back on the source's
/// grid and bar.
#[test]
fn a_backward_cue_realigns_once_without_doubling() {
    for by in [-1.5, -0.5, -2.0] {
        let mut src = Source::new(Feed::Beats, Precision::Fine);
        src.jump = Some((30.0, by));
        let out = run(src);
        assert_eq!(out.flushes_after_start(), 1, "{by}: one snap for one jump");
        out.assert_no_double_in_time();
        let snap = *out.flushes.last().unwrap();
        let worst = out.max_error_after(snap as f64 / SR + 0.1);
        assert!(worst < 4.0, "{by}: {worst:.3} ms");
        out.assert_bars_aligned_except((30.0 * SR) as u64, snap);
    }
}
