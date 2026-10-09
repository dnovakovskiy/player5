//! MIDI clock input (24 pulses per quarter note), from CoreMIDI on Apple
//! platforms or Web MIDI in Chromium. See `docs/protocols/midi-clock.md`.
//!
//! Pulses arrive individually jittery (driver and USB scheduling), so the
//! tempo is a least-squares fit of pulse time against pulse index over a
//! sliding window, which averages the jitter of every pulse in it instead
//! of trusting any single interval. Phase comes from the pulse count since
//! Start: beat 0 is the first pulse after Start, Continue keeps counting,
//! Stop turns the reports tempo-only. Timestamps the fit cannot explain
//! (a stalled driver delivering a burst) are left out of the fit; a gap
//! longer than any accepted tempo restarts it.

use crate::follower::{FollowerClock, Observation, Phase};

/// Pulses per quarter note.
pub const PPQN: u32 = 24;

/// Pulses in the tempo fit: four beats. The slope error of a least-squares
/// fit falls as `N^-1.5`: with ±1 ms of jitter it is about 0.035 BPM (1σ)
/// at 120 BPM after two beats and 0.012 BPM once the window is full. A
/// pitch-fader move shows up about half a window (two beats) late; the
/// follower's phase loop absorbs that.
const WINDOW: usize = 4 * PPQN as usize;

/// How far (in standard deviations of the expected difference) the newest
/// half-window's tempo must stray from the full window's before it counts
/// as a tempo change in progress.
const RAMP_SIGMAS: f64 = 4.0;

/// Pulses needed before the first tempo report.
const MIN_FIT: usize = 6;

/// A pulse further than this fraction of a pulse period from where the fit
/// expects it is not used for tempo.
const OUTLIER_FRACTION: f64 = 0.5;

/// Consecutive rejected pulses after which the fit restarts (the tempo
/// really changed abruptly).
const MAX_REJECTED: u32 = 4;

/// The MIDI System Real-Time messages that matter for clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MidiMessage {
    /// Timing Clock (0xF8).
    Clock,
    /// Start (0xFA): the next Clock is the first pulse of beat 0.
    Start,
    /// Continue (0xFB): resume counting from the stopped position.
    Continue,
    /// Stop (0xFC).
    Stop,
}

impl MidiMessage {
    /// Parses a status byte; `None` for anything that is not clock-related.
    #[must_use]
    pub const fn from_status(status: u8) -> Option<Self> {
        match status {
            0xF8 => Some(Self::Clock),
            0xFA => Some(Self::Start),
            0xFB => Some(Self::Continue),
            0xFC => Some(Self::Stop),
            _ => None,
        }
    }
}

/// Turns timestamped MIDI clock messages into [`Observation`]s.
#[derive(Clone, Debug)]
pub struct MidiClockFollower {
    sample_rate: f64,
    running: bool,
    /// Pulses since Start (song position, for phase).
    pulses: u64,
    /// Pulses ever received (x axis of the tempo fit; never reset).
    index: u64,
    last_pulse: Option<f64>,
    /// Ring of pulses used for the tempo fit.
    fit: [Point; WINDOW],
    len: usize,
    head: usize,
    rejected: u32,
    /// Run number for new points: Start and Continue begin a new run,
    /// because a source may restart its clock's phase there.
    run: u32,
}

/// One pulse in the tempo fit.
#[derive(Clone, Copy, Debug, Default)]
struct Point {
    index: u64,
    sample: f64,
    run: u32,
}

impl MidiClockFollower {
    /// A stopped follower.
    #[must_use]
    pub fn new(sample_rate: f64) -> Self {
        Self {
            sample_rate,
            running: false,
            pulses: 0,
            index: 0,
            last_pulse: None,
            fit: [Point::default(); WINDOW],
            len: 0,
            head: 0,
            rejected: 0,
            run: 0,
        }
    }

    /// Whether Start/Continue was seen without a later Stop.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Current tempo estimate, once enough pulses arrived.
    #[must_use]
    pub fn tempo_bpm(&self) -> Option<f64> {
        let period = self.period()?;
        let bpm = 60.0 * self.sample_rate / (period * f64::from(PPQN));
        (FollowerClock::MIN_BPM..=FollowerClock::MAX_BPM)
            .contains(&bpm)
            .then_some(bpm)
    }

    /// Handles one message received at `sample` (consumer's sample clock).
    /// Returns an observation when there is something to report: one per
    /// Timing Clock once the tempo is known, with the bar phase while
    /// running and tempo only while stopped.
    pub fn handle(&mut self, message: MidiMessage, sample: f64) -> Option<Observation> {
        match message {
            MidiMessage::Start => {
                self.running = true;
                self.pulses = 0;
                self.run = self.run.wrapping_add(1);
                None
            }
            MidiMessage::Continue => {
                self.running = true;
                self.run = self.run.wrapping_add(1);
                None
            }
            MidiMessage::Stop => {
                self.running = false;
                None
            }
            MidiMessage::Clock => self.clock(sample),
        }
    }

    fn clock(&mut self, sample: f64) -> Option<Observation> {
        if !sample.is_finite() {
            return None;
        }
        // The pulse counts for phase whatever its timestamp looks like.
        let pulse = self.pulses;
        if self.running {
            self.pulses += 1;
        }
        let index = self.index;
        self.index += 1;

        let usable = self.accept_timing(index, sample);
        self.last_pulse = Some(sample);
        if !usable {
            return None;
        }
        let bpm = self.tempo_bpm()?;
        let phase = if self.running {
            Phase::Bar((pulse as f64 / f64::from(PPQN)).rem_euclid(4.0))
        } else {
            Phase::TempoOnly
        };
        Some(Observation {
            sample,
            phase,
            bpm: Some(bpm),
        })
    }

    /// Decides whether this pulse's timestamp is trustworthy and, if so,
    /// adds it to the fit.
    fn accept_timing(&mut self, index: u64, sample: f64) -> bool {
        // Slower than the slowest accepted tempo: the clock paused.
        let max_gap = 60.0 * self.sample_rate / (FollowerClock::MIN_BPM * f64::from(PPQN));
        if let Some(prev) = self.last_pulse {
            if sample - prev > max_gap {
                self.clear_fit();
            } else if sample <= prev {
                // Out-of-order or duplicate timestamp: no timing information.
                return false;
            }
        }
        if self.len >= MIN_FIT {
            if let (Some(period), Some(predicted)) = (self.period(), self.predict(index, self.run))
            {
                if (sample - predicted).abs() > OUTLIER_FRACTION * period {
                    self.rejected += 1;
                    if self.rejected < MAX_REJECTED {
                        return false;
                    }
                    // Persistently off: the tempo jumped. Start a new fit.
                    self.clear_fit();
                }
            }
        }
        self.rejected = 0;
        self.fit[self.head] = Point {
            index,
            sample,
            run: self.run,
        };
        self.head = (self.head + 1) % WINDOW;
        self.len = (self.len + 1).min(WINDOW);
        true
    }

    fn clear_fit(&mut self) {
        self.len = 0;
        self.rejected = 0;
    }

    /// The newest `count` points of the fit window, oldest first.
    fn points(&self, count: usize) -> impl Iterator<Item = Point> + '_ {
        let count = count.min(self.len);
        let start = (self.head + WINDOW - count) % WINDOW;
        (0..count).map(move |i| self.fit[(start + i) % WINDOW])
    }

    /// Least-squares fit `sample = a_run + b * index` over the newest
    /// `count` points: one slope (the period) shared by every run, one
    /// intercept per run, so a phase restart at Start does not bend the
    /// tempo. Coordinates are relative to the newest point for precision.
    fn line(&self, count: usize) -> Option<Line> {
        let count = count.min(self.len);
        if count < MIN_FIT {
            return None;
        }
        let newest = self.fit[(self.head + WINDOW - 1) % WINDOW];
        // Pooled within-run sums; runs are contiguous in time.
        let mut pooled = Pooled::default();
        let mut run = RunSums::default();
        let mut current = None;
        for p in self.points(count) {
            if current != Some(p.run) {
                pooled.add(&run);
                run = RunSums::default();
                current = Some(p.run);
            }
            run.add(
                p.index as f64 - newest.index as f64,
                p.sample - newest.sample,
            );
        }
        pooled.add(&run);
        if pooled.sxx <= 0.0 {
            return None;
        }
        let b = pooled.sxy / pooled.sxx;
        // `run` is the newest run: its own intercept.
        let a = (run.sy - b * run.sx) / run.n;
        Some(Line {
            newest: newest.index,
            origin: newest.sample,
            run: newest.run,
            a,
            b,
            rss: (pooled.syy - b * pooled.sxy).max(0.0),
            n: count,
            runs: pooled.runs,
        })
    }

    /// Samples per pulse now. Normally the full-window slope; but that is
    /// the period half a window ago, so while the tempo is moving (the
    /// newest half of the window disagrees with the whole by more than the
    /// fit's own jitter explains) the trend is extrapolated to the newest
    /// pulse instead.
    fn period(&self) -> Option<f64> {
        let full = self.line(WINDOW)?;
        let mut period = full.b;
        if full.n == WINDOW {
            if let Some(half) = self.line(WINDOW / 2) {
                // Slope variance of an n-point fit with unit jitter.
                let var = |n: f64| 12.0 / (n * (n * n - 1.0));
                // Timestamps are whole samples at best.
                let dof = (full.n - full.runs - 1).max(1) as f64;
                let jitter = (full.rss / dof).sqrt().max(0.5);
                let spread = jitter * (var(half.n as f64) - var(full.n as f64)).sqrt();
                if (half.b - full.b).abs() > RAMP_SIGMAS * spread {
                    // Centres lag the newest pulse by n/2: extrapolate the
                    // half-window slope by its distance to the full one's.
                    period = 2.0 * half.b - full.b;
                }
            }
        }
        (period > 0.0).then_some(period)
    }

    /// Where pulse `index` of run `run` should land; `None` for a run the
    /// fit has not seen yet (its phase is unknown).
    fn predict(&self, index: u64, run: u32) -> Option<f64> {
        self.line(WINDOW)
            .filter(|l| l.run == run)
            .map(|l| l.origin + l.a + l.b * (index as f64 - l.newest as f64))
    }
}

/// Sums over one run of the pooled fit.
#[derive(Clone, Copy, Debug, Default)]
struct RunSums {
    n: f64,
    sx: f64,
    sy: f64,
    sxx: f64,
    sxy: f64,
    syy: f64,
}

impl RunSums {
    fn add(&mut self, x: f64, y: f64) {
        self.n += 1.0;
        self.sx += x;
        self.sy += y;
        self.sxx += x * x;
        self.sxy += x * y;
        self.syy += y * y;
    }
}

/// Within-run (co)variances pooled over runs.
#[derive(Clone, Copy, Debug, Default)]
struct Pooled {
    sxx: f64,
    sxy: f64,
    syy: f64,
    runs: usize,
}

impl Pooled {
    /// Adds a run's deviations from its own means.
    fn add(&mut self, run: &RunSums) {
        if run.n > 0.0 {
            self.sxx += run.sxx - run.sx * run.sx / run.n;
            self.sxy += run.sxy - run.sx * run.sy / run.n;
            self.syy += run.syy - run.sy * run.sy / run.n;
            self.runs += 1;
        }
    }
}

/// A least-squares fit of pulse time against pulse index.
#[derive(Clone, Copy, Debug)]
struct Line {
    /// Index and sample of the newest point (the coordinate origin).
    newest: u64,
    origin: f64,
    /// Run of the newest point; `a` is that run's intercept.
    run: u32,
    a: f64,
    b: f64,
    rss: f64,
    n: usize,
    runs: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 48_000.0;

    /// Deterministic xorshift64 noise, uniform in `[-a, a)`.
    struct Rng(u64);

    impl Rng {
        fn sym(&mut self, a: f64) -> f64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            ((x >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0) * a
        }
    }

    fn pulse_samples(bpm: f64) -> f64 {
        60.0 * SR / (bpm * f64::from(PPQN))
    }

    #[test]
    fn steady_clock_reports_tempo() {
        let mut m = MidiClockFollower::new(SR);
        // 120 BPM: 24 pulses per 24 000 samples = 1 000 samples per pulse.
        m.handle(MidiMessage::Start, 0.0);
        let mut last = None;
        for k in 0..96 {
            last = m.handle(MidiMessage::Clock, k as f64 * 1_000.0);
        }
        let obs = last.unwrap();
        assert!((obs.bpm.unwrap() - 120.0).abs() < 1e-6);
        assert_eq!(obs.phase, Phase::Bar(95.0 / 24.0));
    }

    #[test]
    fn status_bytes() {
        assert_eq!(MidiMessage::from_status(0xF8), Some(MidiMessage::Clock));
        assert_eq!(MidiMessage::from_status(0xFA), Some(MidiMessage::Start));
        assert_eq!(MidiMessage::from_status(0xFB), Some(MidiMessage::Continue));
        assert_eq!(MidiMessage::from_status(0xFC), Some(MidiMessage::Stop));
        assert_eq!(MidiMessage::from_status(0x90), None);
    }

    /// ±1 ms of jitter at 24 ppqn: within ±0.1 BPM after two beats (all but
    /// a handful of 3σ reports), and every report within it after four.
    #[test]
    fn jittery_clock_gives_tempo_within_a_tenth_after_two_beats() {
        for bpm in [86.5, 120.0, 124.0, 128.0, 140.0] {
            let (mut reports, mut outside) = (0u32, 0u32);
            let mut worst_after_four: f64 = 0.0;
            for seed in 1..=20u64 {
                let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
                let mut m = MidiClockFollower::new(SR);
                m.handle(MidiMessage::Start, 0.0);
                let period = pulse_samples(bpm);
                for k in 0..(24 * 16) {
                    let t = 1_000.0 + k as f64 * period + rng.sym(0.001) * SR;
                    let obs = m.handle(MidiMessage::Clock, t);
                    if k >= 2 * 24 {
                        let got = obs.and_then(|o| o.bpm).expect("tempo after two beats");
                        let err = (got - bpm).abs();
                        reports += 1;
                        if err >= 0.1 {
                            outside += 1;
                        }
                        if k >= 4 * 24 {
                            worst_after_four = worst_after_four.max(err);
                        }
                    }
                }
            }
            assert!(
                f64::from(outside) < 0.01 * f64::from(reports),
                "{bpm} BPM: {outside} of {reports} reports off by 0.1 BPM or more"
            );
            assert!(
                worst_after_four < 0.1,
                "{bpm} BPM: {worst_after_four:.4} BPM after four beats"
            );
        }
    }

    #[test]
    #[ignore = "report; run with --ignored --nocapture"]
    fn tempo_error_report() {
        for bpm in [90.0, 120.0, 128.0, 140.0, 174.0] {
            let mut worst2: f64 = 0.0;
            let mut worst4: f64 = 0.0;
            for seed in 1..=50u64 {
                let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
                let mut m = MidiClockFollower::new(SR);
                m.handle(MidiMessage::Start, 0.0);
                let period = pulse_samples(bpm);
                for k in 0..(24 * 8) {
                    let t = 1_000.0 + k as f64 * period + rng.sym(0.001) * SR;
                    let obs = m.handle(MidiMessage::Clock, t);
                    let err = obs.and_then(|o| o.bpm).map_or(0.0, |b| (b - bpm).abs());
                    if k >= 48 {
                        worst2 = worst2.max(err);
                    }
                    if k >= 96 {
                        worst4 = worst4.max(err);
                    }
                }
            }
            println!("{bpm}: worst after 2 beats {worst2:.4}, after 4 beats {worst4:.4}");
        }
    }

    #[test]
    fn start_resets_the_pulse_count_and_continue_keeps_it() {
        let mut m = MidiClockFollower::new(SR);
        // Clock runs while stopped: tempo-only reports.
        let mut t = 0.0;
        let mut last = None;
        for _ in 0..30 {
            last = m.handle(MidiMessage::Clock, t);
            t += 1_000.0;
        }
        assert_eq!(last.unwrap().phase, Phase::TempoOnly);
        assert!(!m.is_running());
        // Start: the next pulse is beat 0.
        m.handle(MidiMessage::Start, t - 500.0);
        let first = m.handle(MidiMessage::Clock, t).unwrap();
        assert_eq!(first.phase, Phase::Bar(0.0));
        for _ in 0..35 {
            t += 1_000.0;
            last = m.handle(MidiMessage::Clock, t);
        }
        assert_eq!(last.unwrap().phase, Phase::Bar(35.0 / 24.0));
        // Stop: tempo-only, count frozen.
        m.handle(MidiMessage::Stop, t + 10.0);
        t += 1_000.0;
        assert_eq!(
            m.handle(MidiMessage::Clock, t).unwrap().phase,
            Phase::TempoOnly
        );
        // Continue: counting resumes where it stopped.
        m.handle(MidiMessage::Continue, t + 10.0);
        t += 1_000.0;
        assert_eq!(
            m.handle(MidiMessage::Clock, t).unwrap().phase,
            Phase::Bar(36.0 / 24.0)
        );
    }

    #[test]
    fn absurd_intervals_are_rejected() {
        let mut m = MidiClockFollower::new(SR);
        m.handle(MidiMessage::Start, 0.0);
        let mut t = 0.0;
        for _ in 0..48 {
            m.handle(MidiMessage::Clock, t);
            t += 1_000.0;
        }
        // A burst: one pulse stamped 600 samples late, the next on time.
        assert!(m.handle(MidiMessage::Clock, t + 600.0).is_none());
        t += 1_000.0;
        let obs = m.handle(MidiMessage::Clock, t).unwrap();
        assert!((obs.bpm.unwrap() - 120.0).abs() < 1e-6);
        // Both pulses still counted for phase.
        assert_eq!(obs.phase, Phase::Bar(49.0 / 24.0));
        // A duplicate timestamp carries no timing.
        assert!(m.handle(MidiMessage::Clock, t).is_none());
        // A long gap (the clock paused) restarts the fit instead of
        // averaging the pause in.
        t += 10.0 * SR;
        assert!(m.handle(MidiMessage::Clock, t).is_none());
        for _ in 0..(MIN_FIT - 1) {
            t += 500.0;
            m.handle(MidiMessage::Clock, t);
        }
        assert!((m.tempo_bpm().unwrap() - 240.0).abs() < 1e-6);
    }

    #[test]
    fn a_clock_restarted_at_start_keeps_its_tempo() {
        let mut m = MidiClockFollower::new(SR);
        let mut t = 0.0;
        for _ in 0..96 {
            m.handle(MidiMessage::Clock, t);
            t += 1_000.0;
        }
        // The source restarts its clock phase at Start: the first pulse
        // comes 333 samples off the old grid. Not an outlier, not a ramp.
        m.handle(MidiMessage::Start, t - 500.0);
        t += 333.0;
        for k in 0..60 {
            let obs = m.handle(MidiMessage::Clock, t).expect("every pulse reports");
            assert!((obs.bpm.unwrap() - 120.0).abs() < 1e-6, "pulse {k}: {obs:?}");
            t += 1_000.0;
        }
    }

    #[test]
    fn a_pitch_fader_move_is_tracked_without_window_lag() {
        // 120 -> 126 BPM over four seconds, ±1 ms jitter.
        let mut rng = Rng(0x5EED);
        let mut m = MidiClockFollower::new(SR);
        m.handle(MidiMessage::Start, 0.0);
        let (mut t, mut worst, mut inside) = (0.0, 0.0f64, 0.0f64);
        while t < 9.0 * SR {
            let bpm = 120.0 + 6.0 * ((t / SR - 2.0) / 4.0).clamp(0.0, 1.0);
            t += pulse_samples(bpm);
            let obs = m.handle(MidiMessage::Clock, t + rng.sym(0.001) * SR);
            if t < SR {
                continue; // the first beats are the fit warming up
            }
            let err = (obs.unwrap().bpm.unwrap() - bpm).abs();
            worst = worst.max(err);
            if t > 3.0 * SR && t < 6.0 * SR {
                inside = inside.max(err);
            }
        }
        // A plain four-beat fit lags two beats, about 1.5 BPM here, for the
        // whole move. Extrapolating the trend removes that lag inside the
        // move; only its kinks (start and end) cost a brief error.
        assert!(inside < 0.3, "{inside:.3} BPM");
        assert!(worst < 1.0, "{worst:.3} BPM");
    }

    #[test]
    fn follows_a_tempo_jump() {
        let mut m = MidiClockFollower::new(SR);
        m.handle(MidiMessage::Start, 0.0);
        let mut t = 0.0;
        for _ in 0..96 {
            m.handle(MidiMessage::Clock, t);
            t += pulse_samples(120.0);
        }
        // An abrupt switch to 140 BPM.
        for _ in 0..96 {
            m.handle(MidiMessage::Clock, t);
            t += pulse_samples(140.0);
        }
        assert!((m.tempo_bpm().unwrap() - 140.0).abs() < 1e-6);
    }
}
