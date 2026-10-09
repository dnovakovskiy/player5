//! End-to-end timing under abuse: minutes of audio through the control
//! half, the event queue and the renderer, in lockstep (`Engine::render`,
//! the browser) and split (control on its own thread at the renderer's
//! published position, the Apple shells through `core/ffi`'s split API),
//! while a source jitters, ramps, stalls and cues, and the DJ storms the
//! transport, re-syncs, taps and switches clocks. Every trigger the
//! renderer plays is recorded and checked: no step doubled or dropped,
//! flams whole, every hit on the grid of the clock it follows.
//!
//! Deterministic: seeded xorshift noise, simulated time, no wall clock.

use std::collections::HashMap;
use std::sync::Arc;

use engine::{ClockMode, Control, Renderer, SharedTiming};
use sequencer::queue::{event_queue, Consumer, Producer};
use sequencer::{EventKind, Pattern, Track, VoiceId};
use sync::{MidiMessage, Observation, Phase, Precision};

const SR: f64 = 48_000.0;
/// Control tick and lockstep render block (the web worklet's quantum).
const BLOCK: usize = 128;
/// Render block of the split rig (a typical Core Audio buffer).
const SPLIT_BLOCK: usize = 512;
/// Grace-note velocity is 0.6 of the main hit's; anything below this is a
/// grace note in [`pattern`].
const GRACE_BELOW: f32 = 0.5;
/// The flam spacing of [`pattern`] (`flam = 1`, 40 ms).
const FLAM: u64 = 1_920;

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

/// Every step on the closed hat (one trigger per step, the identity
/// track), the kick on the downbeat (bar alignment), and a flam on every
/// step of the snare (so realigns keep landing between a grace note and
/// its hit).
fn pattern() -> Pattern {
    let mut p = Pattern::empty();
    *p.track_mut(VoiceId::ClosedHat) = Track::parse("xxxx xxxx xxxx xxxx").unwrap();
    *p.track_mut(VoiceId::Kick) = Track::parse("x--- ---- ---- ----").unwrap();
    *p.track_mut(VoiceId::Snare) = Track::parse("ffff ffff ffff ffff").unwrap();
    p.flam = 1.0;
    p
}

/// One trigger the renderer played.
#[derive(Clone, Copy, Debug)]
struct Hit {
    /// The sample it was stamped with.
    stamp: u64,
    /// The sample it actually played at (later than `stamp` if it arrived
    /// late).
    at: u64,
    voice: VoiceId,
    velocity: f32,
}

/// Control and renderer with a recording tap on the queue between them.
struct Rig {
    control: Control,
    renderer: Renderer,
    tap: Consumer,
    to_renderer: Producer,
    split: bool,
    /// Triggers the renderer holds but has not played, mirroring its
    /// pending list.
    pending: Vec<Hit>,
    hits: Vec<Hit>,
    flushes: Vec<u64>,
}

impl Rig {
    fn new(split: bool) -> Self {
        let timing = Arc::new(SharedTiming::default());
        let (producer, tap) = event_queue(engine::EVENT_QUEUE_CAPACITY);
        let (to_renderer, consumer) = event_queue(engine::EVENT_QUEUE_CAPACITY);
        let mut control = Control::new(SR as f32, producer, Arc::clone(&timing));
        control.set_pattern(pattern());
        Self {
            control,
            renderer: Renderer::new(SR as f32, consumer, timing),
            tap,
            to_renderer,
            split,
            pending: Vec::new(),
            hits: Vec::new(),
            flushes: Vec::new(),
        }
    }

    /// The `now` the control half uses: the render position in lockstep,
    /// the renderer's published block start when split.
    fn now(&self) -> u64 {
        if self.split {
            self.control.render_timing().position
        } else {
            self.renderer.position()
        }
    }

    /// Where a restart pressed now puts its downbeat: `now` in lockstep;
    /// split, the first sample after the block the renderer is playing and
    /// the one it may pull before the flush arrives.
    fn commit(&self) -> u64 {
        let now = self.now();
        if self.split && now >= 2 * SPLIT_BLOCK as u64 {
            now + 2 * SPLIT_BLOCK as u64
        } else {
            now
        }
    }

    /// The renderer pulls: everything the control half queued so far.
    fn forward(&mut self) {
        while let Some(e) = self.tap.pop() {
            match e.kind {
                EventKind::Trigger { voice, velocity } => self.pending.push(Hit {
                    stamp: e.sample,
                    at: e.sample,
                    voice,
                    velocity,
                }),
                EventKind::Flush => {
                    self.flushes.push(e.sample);
                    self.pending.retain(|h| h.stamp < e.sample);
                }
                EventKind::Param { .. } => {}
            }
            assert!(self.to_renderer.push(e).is_ok(), "renderer queue full");
        }
    }

    fn render(&mut self, frames: usize) {
        let start = self.renderer.position();
        let end = start + frames as u64;
        let mut i = 0;
        while i < self.pending.len() {
            if self.pending[i].stamp < end {
                let mut h = self.pending.remove(i);
                h.at = h.stamp.max(start);
                self.hits.push(h);
            } else {
                i += 1;
            }
        }
        let mut out = vec![0.0f32; frames];
        self.renderer.process(&mut out);
        assert!(out.iter().all(|s| s.is_finite()));
    }

    /// [`BLOCK`] samples of simulated time ending at `clock + BLOCK`.
    /// Lockstep: tick at the render position, then render. Split: the
    /// audio thread renders a [`SPLIT_BLOCK`] when its time comes; the
    /// control thread ticks every [`BLOCK`] at the published position.
    fn block(&mut self, clock: u64) {
        if self.split {
            if self.renderer.position() <= clock {
                self.forward();
                self.render(SPLIT_BLOCK);
            }
            let now = self.now();
            self.control.tick(now);
        } else {
            let now = self.now();
            self.control.tick(now);
            self.forward();
            self.render(BLOCK);
        }
    }

    /// Everything still pending will be heard; hits in playing order.
    fn finish(mut self) -> (Vec<Hit>, Vec<u64>) {
        self.forward();
        let mut hits = std::mem::take(&mut self.hits);
        hits.append(&mut self.pending);
        hits.sort_by_key(|h| (h.at, h.stamp));
        (hits, self.flushes)
    }
}

fn hats(hits: &[Hit]) -> Vec<Hit> {
    hits.iter()
        .copied()
        .filter(|h| h.voice == VoiceId::ClosedHat)
        .collect()
}

/// Every flam is whole and single: each snare grace note is stamped
/// exactly a flam before one main hit, each main hit has exactly one grace
/// note a flam before it, and the grace plays no later than its hit. A
/// grace note heard twice (a realign replaying a step whose grace was
/// already heard), orphaned, or missing all break this. `lone_grace_ok`
/// may excuse a grace note (by stamp) whose hit a restart dropped.
fn assert_flams_whole(
    hits: &[Hit],
    min_pairs: usize,
    lone_grace_ok: impl Fn(u64) -> bool,
    what: &str,
) {
    let mut graces: HashMap<u64, Vec<u64>> = HashMap::new();
    let mut mains: HashMap<u64, Vec<u64>> = HashMap::new();
    for h in hits.iter().filter(|h| h.voice == VoiceId::Snare) {
        let set = if h.velocity < GRACE_BELOW {
            &mut graces
        } else {
            &mut mains
        };
        set.entry(h.stamp).or_default().push(h.at);
    }
    let s = |at: u64| at as f64 / SR;
    for (&stamp, at) in &graces {
        assert_eq!(
            at.len(),
            1,
            "{what}: grace note doubled at {:.4} s",
            s(at[0])
        );
        match mains.get(&(stamp + FLAM)).map(Vec::as_slice) {
            Some([m]) => assert!(*m >= at[0], "{what}: grace after its hit at {:.4} s", s(*m)),
            None => assert!(
                lone_grace_ok(stamp),
                "{what}: grace note without its hit at {:.4} s",
                s(at[0])
            ),
            Some(m) => panic!("{what}: hit doubled at {:.4} s", s(m[0])),
        }
    }
    for (&stamp, at) in &mains {
        assert_eq!(at.len(), 1, "{what}: hit doubled at {:.4} s", s(at[0]));
        if stamp >= FLAM {
            assert!(
                graces.contains_key(&(stamp - FLAM)),
                "{what}: hit without its grace note at {:.4} s",
                s(at[0])
            );
        }
    }
    assert!(mains.len() >= min_pairs, "{what}: {} flams", mains.len());
}

// ---------------------------------------------------------------------
// Following a source.
// ---------------------------------------------------------------------

/// How the simulated source reports.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Feed {
    /// One bar-phase report per beat (Pro DJ Link beat packets).
    Beats,
    /// Bar-phase reports several times a beat (precise position, Link).
    Position,
    /// MIDI clock: 24 pulses per beat after a Start.
    Midi,
}

/// What the DJ does, at a time in seconds.
#[derive(Clone, Copy, Debug)]
enum Act {
    Start,
    Stop,
    Resync,
    /// Switch to the internal clock (continuous).
    Internal,
    /// Switch back to following.
    Follow,
}

struct FollowCase {
    feed: Feed,
    precision: Precision,
    bpm: f64,
    jitter_s: f64,
    /// Max grid error (ms) away from disturbances.
    tolerance_ms: f64,
    split: bool,
    seed: u64,
}

/// The source's true beat (with cue jumps) and its elapsed beat (without:
/// strictly increasing, so it names each source step once) per block.
struct Truth {
    beat: Vec<f64>,
    elapsed: Vec<f64>,
}

impl Truth {
    fn interp(v: &[f64], sample: u64) -> f64 {
        let i = (sample as usize / BLOCK).min(v.len() - 2);
        let frac = (sample as f64 - (i * BLOCK) as f64) / BLOCK as f64;
        v[i] + (v[i + 1] - v[i]) * frac
    }
}

/// A three-minute set: the source ramps three times, stalls every few
/// seconds, cues forward and back; the DJ presses re-sync, storms the
/// transport and borrows the internal clock twice.
fn follow_script() -> Vec<(f64, Act)> {
    let mut acts = vec![(1.0, Act::Start)];
    // Re-sync presses, irregular.
    let mut t: f64 = 3.0;
    while t < 170.0 {
        acts.push((t, Act::Resync));
        t += 0.61 + 0.37 * (t * 7.3).rem_euclid(1.0);
    }
    // Start/stop storms: stop and start again inside the lookahead, start
    // while playing, stop long enough for the queue to run dry.
    for (k, base) in [20.0, 47.0, 88.0, 133.0].into_iter().enumerate() {
        let k = k as f64;
        acts.push((base, Act::Stop));
        acts.push((base + 0.013 + 0.01 * k, Act::Start));
        acts.push((base + 0.4, Act::Start));
        acts.push((base + 0.9, Act::Start));
        acts.push((base + 1.5, Act::Stop));
        acts.push((base + 1.5 + 0.06, Act::Start));
        acts.push((base + 3.0, Act::Stop));
        acts.push((base + 5.0 + 0.1 * k, Act::Start));
    }
    // Borrow the internal clock while playing, then follow again.
    for base in [60.0, 110.0] {
        acts.push((base, Act::Internal));
        acts.push((base + 3.0, Act::Follow));
    }
    acts.sort_by(|a, b| a.0.total_cmp(&b.0));
    acts
}

/// Pitch-fader moves `(from_s, to_s, to_bpm_factor)`, kept clear of the
/// internal-clock windows (the internal clock does not ramp with them).
const RAMPS: [(f64, f64, f64); 3] = [(25.0, 31.0, 1.04), (70.0, 74.0, 0.97), (140.0, 150.0, 1.05)];
/// Cue jumps `(at_s, beats)`.
const JUMPS: [(f64, f64); 2] = [(80.0, 1.5), (125.0, -0.75)];
const SECONDS: f64 = 180.0;

fn reported_bpm(base: f64, t: f64) -> f64 {
    let mut bpm = base;
    for (a, b, f) in RAMPS {
        if t <= a {
            break;
        }
        let from = bpm;
        bpm = from + (base * f - from) * ((t - a) / (b - a)).min(1.0);
    }
    bpm
}

enum Report {
    Obs(Observation),
    Midi(MidiMessage, f64),
}

struct FollowRun {
    hits: Vec<Hit>,
    flushes: Vec<u64>,
    truth: Truth,
    /// Spans `(from, to)` in samples where steps may legitimately be
    /// missing: stopped, a cue jump, the hand-back to the source.
    gap_ok: Vec<(u64, u64)>,
    /// Spans where the grid error is not checked (settling).
    settle: Vec<(u64, u64)>,
}

fn run_follow(case: &FollowCase) -> FollowRun {
    let mut rng = Rng(case.seed);
    let mut rig = Rig::new(case.split);
    let follow = ClockMode::Follow(case.precision);
    rig.control.set_clock_mode(follow, 0);
    let per_beat = match case.feed {
        Feed::Beats => 1.0,
        Feed::Position => 7.0,
        Feed::Midi => 24.0,
    };
    let skew = 60e-6;
    let mut beat: f64 = 13.37;
    let mut elapsed = beat;
    let mut truth = Truth {
        beat: vec![beat],
        elapsed: vec![elapsed],
    };
    let mut next_index = (beat * per_beat).ceil() as i64;
    // MIDI: the song starts about 0.8 s in; phase counts from there.
    let midi_origin = beat.ceil() + 1.0;
    let origin_index = (midi_origin * per_beat) as i64;
    let mut pending: Vec<(f64, Report)> = Vec::new();
    let script = follow_script();
    let mut next_act = 0;
    let mut jumped = [false; 2];
    let mut gap_ok = Vec::new();
    let mut settle = vec![(0, (2.0 * SR) as u64)];
    let mut stopped_at: Option<u64> = Some(0);
    let mut internal = false;
    let mut clock = 0u64;
    while (clock as f64) < SECONDS * SR {
        let now_f = clock as f64;
        let t = now_f / SR;
        let b0 = reported_bpm(case.bpm, t) * (1.0 + skew);
        let b1 = reported_bpm(case.bpm, t + BLOCK as f64 / SR) * (1.0 + skew);
        let step = 0.5 * (b0 + b1) / 60.0 * BLOCK as f64 / SR;
        let end = beat + step;
        while (next_index as f64 / per_beat) < end {
            let report = next_index as f64 / per_beat;
            let at = now_f + (report - beat) / step * BLOCK as f64;
            let sample = at + rng.sym(case.jitter_s) * SR;
            let latency = (0.002 + 0.004 * rng.unit()) * SR;
            let event = match case.feed {
                Feed::Beats | Feed::Position => Report::Obs(Observation {
                    sample,
                    phase: Phase::Bar(report.rem_euclid(4.0)),
                    bpm: Some(reported_bpm(case.bpm, at / SR)),
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
            // Network stall: 250 ms every 4.1 s; the backlog bursts out
            // at one instant, in order (MIDI is a byte stream; a datagram
            // source may reorder, which the follower handles anyway).
            let window = ((due / SR) / 4.1).floor();
            if due / SR - window * 4.1 < 0.25 {
                due = (window * 4.1 + 0.25) * SR;
            }
            pending.push((due, event));
            next_index += 1;
        }
        beat = end;
        elapsed += step;
        if case.feed != Feed::Midi {
            for (k, (at, by)) in JUMPS.into_iter().enumerate() {
                if !jumped[k] && t >= at {
                    jumped[k] = true;
                    beat += by;
                    next_index = (beat * per_beat).ceil() as i64;
                    // Until the follower is sure the source jumped (a beat
                    // or two) we play the old timeline, by design.
                    let from = (at * SR) as u64;
                    gap_ok.push((from, from + (3.0 * SR) as u64));
                    settle.push((from, from + (4.0 * SR) as u64));
                }
            }
        }
        truth.beat.push(beat);
        truth.elapsed.push(elapsed);

        let now = rig.now();
        pending.sort_by(|a, b| a.0.total_cmp(&b.0));
        while pending.first().is_some_and(|(due, _)| *due <= now as f64) {
            match pending.remove(0).1 {
                Report::Obs(obs) => rig.control.observe(&obs, now),
                Report::Midi(m, s) => rig.control.midi(m, s, now),
            }
        }
        while next_act < script.len() && script[next_act].0 <= t {
            let act = script[next_act].1;
            next_act += 1;
            match act {
                Act::Start => {
                    assert!(rig.control.is_locked() || t < 2.0);
                    if let Some(s) = stopped_at.take() {
                        gap_ok.push((s, now + (0.2 * SR) as u64));
                        settle.push((s, now + (0.2 * SR) as u64));
                    }
                    rig.control.start(now);
                }
                Act::Stop => {
                    rig.control.stop();
                    stopped_at.get_or_insert(now);
                }
                // On the internal clock re-sync restarts the bar: not
                // something to do while borrowing it mid-set.
                Act::Resync if internal => {}
                Act::Resync => rig.control.resync(now),
                Act::Internal => {
                    rig.control.set_clock_mode(ClockMode::Internal, now);
                    internal = true;
                }
                Act::Follow => {
                    rig.control.set_clock_mode(follow, now);
                    internal = false;
                    // The fresh follower snaps on its first report: a
                    // forward snap may skip a step to keep half a step
                    // clear of the last one heard.
                    gap_ok.push((now, now + (1.5 * SR) as u64));
                    settle.push((now, now + (2.0 * SR) as u64));
                }
            }
        }
        rig.block(clock);
        clock += BLOCK as u64;
    }
    if case.feed == Feed::Midi {
        for b in truth.beat.iter_mut().chain(truth.elapsed.iter_mut()) {
            *b -= midi_origin;
        }
    }
    let (hits, flushes) = rig.finish();
    FollowRun {
        hits,
        flushes,
        truth,
        gap_ok,
        settle,
    }
}

fn overlaps(spans: &[(u64, u64)], a: u64, b: u64) -> bool {
    spans.iter().any(|&(s, e)| a <= e && b >= s)
}

fn check_follow(case: &FollowCase, run: &FollowRun) {
    let what = format!(
        "{:?}/{:?} {} BPM {}",
        case.feed,
        case.precision,
        case.bpm,
        if case.split { "split" } else { "lockstep" }
    );
    let hats = hats(&run.hits);
    assert!(hats.len() > 1_000, "{what}: {} hats", hats.len());
    let step = 0.25 * 60.0 * SR / case.bpm;
    let min_step = step / 1.05;
    let max_step = step / 0.97;
    // Nothing doubled or flammed in time, whatever happened.
    for w in hats.windows(2) {
        let d = (w[1].at - w[0].at) as f64;
        if d <= 0.4 * min_step {
            let a = w[0].at;
            eprintln!(
                "hats {:?}",
                hats.iter()
                    .filter(|h| h.at + 30_000 > a && h.at < a + 30_000)
                    .map(|h| (
                        h.stamp,
                        h.at,
                        (Truth::interp(&run.truth.beat, h.stamp) * 4.0)
                    ))
                    .collect::<Vec<_>>()
            );
            eprintln!(
                "flushes {:?}",
                run.flushes
                    .iter()
                    .filter(|&&f| f + 30_000 > a && f < a + 30_000)
                    .collect::<Vec<_>>()
            );
        }
        assert!(
            d > 0.4 * min_step,
            "{what}: hats {d} samples apart at {:.3} s",
            w[1].at as f64 / SR
        );
        if !overlaps(&run.gap_ok, w[0].at, w[1].at) {
            assert!(
                d < 1.6 * max_step,
                "{what}: silent for {:.0} ms at {:.3} s",
                d * 1_000.0 / SR,
                w[0].at as f64 / SR
            );
        }
    }
    // Every source step heard once: the elapsed step index (which does not
    // repeat when the source cues back) advances by exactly one.
    let index = |h: &Hit| (Truth::interp(&run.truth.elapsed, h.stamp) * 4.0).round() as i64;
    for w in hats.windows(2) {
        let (a, b) = (index(&w[0]), index(&w[1]));
        if overlaps(&run.gap_ok, w[0].stamp, w[1].stamp) {
            assert!(
                b > a,
                "{what}: step {b} after {a} at {:.3} s",
                w[1].at as f64 / SR
            );
        } else {
            assert_eq!(
                b - a,
                1,
                "{what}: step {a} then {b} at {:.3} s",
                w[1].at as f64 / SR
            );
        }
    }
    // On the source's grid. A step a realign left just behind the commit
    // point may play up to the snap grace (20 ms) late.
    let mut worst: f64 = 0.0;
    for h in &hats {
        if overlaps(&run.settle, h.stamp, h.stamp) {
            continue;
        }
        let steps = Truth::interp(&run.truth.beat, h.stamp) * 4.0;
        let err_ms = (steps - steps.round()) / 4.0 * 60_000.0 / case.bpm;
        let after_flush = run
            .flushes
            .iter()
            .any(|&f| f <= h.at && h.at < f + 2 * step as u64);
        let late_ms = (h.at - h.stamp) as f64 * 1_000.0 / SR;
        let allowed_late = if after_flush { 20.5 } else { 0.0 };
        assert!(
            err_ms.abs() < case.tolerance_ms && late_ms <= allowed_late,
            "{what}: {err_ms:.2} ms off, {late_ms:.2} ms late at {:.3} s",
            h.at as f64 / SR
        );
        worst = worst.max(err_ms.abs());
    }
    assert!(worst > 0.0);
    // Our bar is the source's bar.
    for h in run.hits.iter().filter(|h| h.voice == VoiceId::Kick) {
        if overlaps(&run.settle, h.stamp, h.stamp) {
            continue;
        }
        let b = Truth::interp(&run.truth.beat, h.stamp);
        let bar_pos = (b - 4.0 * (b / 4.0).round()).abs();
        assert!(bar_pos < 0.1, "{what}: kick at source beat {b:.3}");
    }
    assert_flams_whole(&run.hits, 1_000, |_| false, &what);
}

fn follow_cases(split: bool) -> Vec<FollowCase> {
    vec![
        FollowCase {
            feed: Feed::Beats,
            precision: Precision::Fine,
            bpm: 124.0,
            jitter_s: 0.003,
            tolerance_ms: 6.0,
            split,
            seed: 0x2545_F491_4F6C_DD1D,
        },
        FollowCase {
            feed: Feed::Position,
            precision: Precision::Exact,
            bpm: 174.0,
            jitter_s: 0.000_3,
            tolerance_ms: 3.0,
            split,
            seed: 0x9E37_79B9_7F4A_7C15,
        },
        FollowCase {
            feed: Feed::Midi,
            precision: Precision::Jittery,
            bpm: 128.0,
            jitter_s: 0.001,
            tolerance_ms: 4.0,
            split,
            seed: 0xD1B5_4A32_D192_ED03,
        },
    ]
}

/// Three minutes per source, lockstep: everything a set can throw at the
/// follower and the transport at once.
#[test]
fn following_through_a_rough_set_lockstep() {
    for case in follow_cases(false) {
        let run = run_follow(&case);
        check_follow(&case, &run);
    }
}

/// The same set with control and render on separate threads.
#[test]
fn following_through_a_rough_set_split() {
    for case in follow_cases(true) {
        let run = run_follow(&case);
        check_follow(&case, &run);
    }
}

// ---------------------------------------------------------------------
// The internal clock.
// ---------------------------------------------------------------------

/// Two minutes on the internal clock. Each restart (start, or re-sync
/// while playing) begins a run whose steps must land exactly on
/// `restart + k * step` with nothing of the previous run after it; taps
/// and tempo changes in between must never double or drop a step.
fn run_internal(split: bool, seed: u64) {
    let what = if split { "split" } else { "lockstep" };
    let mut rng = Rng(seed);
    let mut rig = Rig::new(split);
    // Restarts, in samples, with the tempo of their run, and stops.
    let mut restarts: Vec<(u64, f64)> = Vec::new();
    let mut stops: Vec<u64> = Vec::new();
    let mut playing = false;
    let mut bpm = 120.0;
    let mut clock = 0u64;
    let seconds = 120.0;
    // Phase A (0-60 s): restart storms at fixed tempi, sometimes with
    // latency compensation and nudge set. Phase B (60-120 s): playing
    // through tempo changes and tap sequences.
    let mut next_event = 0.5;
    let mut taps_left = 0;
    let mut tap_at = 0.0;
    let mut tap_period = 0.0;
    let mut phase_b_started = false;
    while (clock as f64) < seconds * SR {
        let t = clock as f64 / SR;
        if t >= next_event && t < 60.0 {
            let now = rig.now();
            match (rng.unit() * 6.0) as u32 {
                0 | 1 => {
                    // Start (while playing or stopped).
                    restarts.push((rig.commit(), bpm));
                    rig.control.start(now);
                    playing = true;
                }
                2 => {
                    if playing {
                        restarts.push((rig.commit(), bpm));
                    }
                    rig.control.resync(now);
                }
                3 => {
                    rig.control.stop();
                    if playing {
                        stops.push(now);
                    }
                    playing = false;
                    // A new tempo for the next run.
                    bpm = 90.0 + 60.0 * rng.unit();
                    rig.control.set_tempo(bpm, now);
                }
                4 => {
                    // Stop and start again within the lookahead.
                    rig.control.stop();
                    rig.control.start(now);
                    restarts.push((rig.commit(), bpm));
                    playing = true;
                }
                _ => {
                    let latency = rng.sym(12.0);
                    let nudge = rng.sym(8.0);
                    rig.control.set_latency_ms(latency);
                    rig.control.set_nudge_ms(nudge);
                    restarts.push((rig.commit(), bpm));
                    rig.control.start(now);
                    playing = true;
                }
            }
            next_event = t + 0.05 + 1.2 * rng.unit();
        }
        if t >= 60.0 && !phase_b_started {
            phase_b_started = true;
            // Phase B starts from a clean run at 120 BPM.
            let now = rig.now();
            rig.control.set_latency_ms(0.0);
            rig.control.set_nudge_ms(0.0);
            bpm = 120.0;
            rig.control.set_tempo(bpm, now);
            restarts.push((rig.commit(), bpm));
            rig.control.start(now);
            playing = true;
            next_event = t + 0.3;
        }
        if t >= next_event && t >= 60.5 {
            let now = rig.now();
            if taps_left == 0 && rng.unit() < 0.3 {
                // A tap sequence, off our grid, at a nearby tempo.
                taps_left = 3 + (rng.unit() * 5.0) as u32;
                tap_period = 60.0 / (100.0 + 40.0 * rng.unit());
                tap_at = t + 0.05;
            } else if taps_left == 0 {
                bpm = 90.0 + 60.0 * rng.unit();
                rig.control.set_tempo(bpm, now);
            }
            next_event = t + 0.3 + rng.unit();
        }
        if taps_left > 0 && t >= tap_at {
            // The tap lands where the simulated finger is: the current
            // time, which on the split rig is ahead of the published
            // position. Human error up to 25 ms.
            let sample = clock as f64 + rng.sym(0.025) * SR;
            let now = rig.now();
            rig.control.tap(sample.max(0.0), now);
            taps_left -= 1;
            tap_at += tap_period;
        }
        rig.block(clock);
        clock += BLOCK as u64;
    }
    let (hits, _) = rig.finish();
    let hats = hats(&hits);
    let phase_b = restarts.last().unwrap().0;
    let restart_at = |s: u64| restarts.iter().any(|r| r.0 == s);

    // Phase A: every run is exactly its own grid, from its restart up to
    // the next restart or a little after its stop.
    for (i, &(r, run_bpm)) in restarts.iter().enumerate() {
        if r >= phase_b {
            break;
        }
        let next = restarts[i + 1].0;
        let stop = stops.iter().copied().find(|&s| s >= r && s < next);
        let step = 0.25 * 60.0 * SR / run_bpm;
        let run: Vec<&Hit> = hats
            .iter()
            .filter(|h| h.stamp >= r && h.stamp < next)
            .collect();
        assert!(!run.is_empty(), "{what}: run at {r} played nothing");
        for (k, h) in run.iter().enumerate() {
            let expected = r as f64 + k as f64 * step;
            if (h.stamp as f64 - expected).abs() > 1.0 {
                eprintln!("restarts {:?}", &restarts[i.saturating_sub(3)..i + 2]);
                eprintln!(
                    "stops {:?}",
                    stops
                        .iter()
                        .filter(|&&s| s + 100_000 > r && s < r + 100_000)
                        .collect::<Vec<_>>()
                );
                eprintln!(
                    "hats {:?}",
                    hats.iter()
                        .filter(|h| h.stamp + 30_000 > r && h.stamp < r + 30_000)
                        .map(|h| (h.stamp, h.at))
                        .collect::<Vec<_>>()
                );
            }
            assert!(
                (h.stamp as f64 - expected).abs() <= 1.0,
                "{what}: run at {r} ({run_bpm:.2} BPM): hit {k} at {} not {expected:.1}",
                h.stamp
            );
            assert_eq!(h.at, h.stamp, "{what}: late hit at {}", h.stamp);
        }
        // Nothing missing: every grid step before the next restart, or
        // before the stop (and at most a lookahead's worth after it).
        let steps_before = |end: u64| {
            (0..)
                .take_while(|&k| (r as f64 + f64::from(k) * step).round() < end as f64)
                .count()
        };
        match stop {
            None => assert_eq!(run.len(), steps_before(next), "{what}: run at {r}"),
            Some(s) => {
                let lookahead = rig_lookahead();
                assert!(
                    run.len() >= steps_before(s) && run.len() <= steps_before(s + lookahead),
                    "{what}: run at {r} stopped at {s}: {} steps",
                    run.len()
                );
            }
        }
    }
    // Phase B: one run; tempo changes and taps move the grid but never
    // double or drop a step.
    let b: Vec<&Hit> = hats.iter().filter(|h| h.stamp >= phase_b).collect();
    assert!(b.len() > 300, "{what}: {} hats in phase B", b.len());
    let (min_step, max_step) = (0.25 * 60.0 * SR / 170.0, 0.25 * 60.0 * SR / 80.0);
    for w in b.windows(2) {
        let d = (w[1].at - w[0].at) as f64;
        assert!(
            d >= 0.5 * min_step && d <= 1.5 * max_step,
            "{what}: hats {d} samples apart at {:.3} s",
            w[1].at as f64 / SR
        );
    }
    // Everywhere: no two hats within half a step except a restart's
    // downbeat (a restart puts the downbeat where it was pressed).
    for w in hats.windows(2) {
        let d = (w[1].at - w[0].at) as f64;
        if !restart_at(w[1].stamp) {
            assert!(
                d >= 0.5 * min_step,
                "{what}: hats {d} samples apart at {:.3} s",
                w[1].at as f64 / SR
            );
        }
    }
    // A restart drops a step's hit if its grace note was already heard;
    // that grace note is then a pickup into the new downbeat.
    assert_flams_whole(
        &hits,
        500,
        |stamp| restarts.iter().any(|r| r.0 > stamp && r.0 - stamp <= FLAM),
        what,
    );
}

/// The default lookahead plus a split render block.
fn rig_lookahead() -> u64 {
    engine::DEFAULT_LOOKAHEAD_SAMPLES + SPLIT_BLOCK as u64
}

#[test]
fn internal_clock_through_transport_storms_lockstep() {
    run_internal(false, 0x1234_5678_9ABC_DEF1);
    run_internal(false, 0x0FED_CBA9_8765_4321);
}

#[test]
fn internal_clock_through_transport_storms_split() {
    run_internal(true, 0x1234_5678_9ABC_DEF1);
    run_internal(true, 0x0FED_CBA9_8765_4321);
}

// ---------------------------------------------------------------------
// Stop-after.
// ---------------------------------------------------------------------

/// A four-bar (64-step) one-shot plays exactly 64 steps in every mode, on
/// both rigs, through re-sync presses and a cue jump, joining a source
/// whose absolute step is far from 0.
#[test]
fn stop_after_plays_exactly_its_steps_everywhere() {
    for split in [false, true] {
        for follow in [false, true] {
            for resyncs in [false, true] {
                let mut rig = Rig::new(split);
                let mut rng = Rng(99);
                if follow {
                    rig.control
                        .set_clock_mode(ClockMode::Follow(Precision::Fine), 0);
                }
                rig.control.set_stop_after(Some(64));
                let bpm = 131.0;
                let spb = 60.0 * SR / bpm;
                let offset: f64 = 57.3; // source beat at sample 0
                let mut next_beat = offset.ceil();
                let mut jumped = 0.0;
                let mut clock = 0u64;
                let mut presses = 0;
                while (clock as f64) < 12.0 * SR {
                    let t = clock as f64 / SR;
                    let now = rig.now();
                    // A cue jump of 1.25 beats at 3 s.
                    if t >= 3.0 && jumped == 0.0 {
                        jumped = 1.25;
                        next_beat = (offset + jumped + clock as f64 / spb).ceil();
                    }
                    let source_sample = (next_beat - offset - jumped) * spb;
                    if source_sample + 0.004 * SR <= now as f64 {
                        rig.control.observe(
                            &Observation {
                                sample: source_sample + rng.sym(0.002) * SR,
                                phase: Phase::Bar(next_beat.rem_euclid(4.0)),
                                bpm: Some(bpm),
                            },
                            now,
                        );
                        next_beat += 1.0;
                    }
                    if (t - 1.0).abs() < 0.5 * BLOCK as f64 / SR {
                        rig.control.start(now);
                    }
                    if resyncs && t > 1.2 && t < 7.0 && rng.unit() < 0.004 {
                        rig.control.resync(now);
                        presses += 1;
                    }
                    rig.block(clock);
                    clock += BLOCK as u64;
                }
                let what = format!("split {split} follow {follow} resyncs {resyncs}");
                if resyncs {
                    assert!(presses > 3, "{what}: {presses} presses");
                }
                assert!(!rig.control.is_playing(), "{what}");
                let (hits, _) = rig.finish();
                assert_eq!(hats(&hits).len(), 64, "{what}");
                // An internal re-sync is a restart: it may leave a grace
                // note as a pickup (see `run_internal`).
                assert_flams_whole(&hits, 64, |_| !follow && resyncs, &what);
            }
        }
    }
}
