//! Beat timing from coarse status packets.
//!
//! The Opus Quad in lighting mode only reports its beat counter in status
//! packets about every 200 ms (opus-quad.md, "Timing precision"). When the
//! counter goes from `n` to `n + 1` between two packets, beat `n + 1`
//! started somewhere in that gap: a *bracket* of about ±100 ms.
//!
//! The tracker narrows that by projecting the brackets of the last few
//! beats onto the newest one with the current tempo and intersecting them.
//! Packet times and beat times are not commensurate, so successive
//! brackets cut different slices out of the beat and, in simulation, the
//! intersection shrinks to a few tens of milliseconds within a couple of
//! bars (not measured on a unit; see the doc). When the
//! brackets stop agreeing (tempo change, nudge, loop) the oldest ones are
//! dropped. This is player5's estimator, not protocol behaviour.

/// Most beats remembered.
pub const WINDOW: usize = 16;

/// Slack added on both sides of each bracket for receive-time jitter.
pub const SLACK_NS: u64 = 4_000_000;

/// A gap between two packets longer than this drops the older brackets
/// (the deck may have paused in between).
pub const MAX_GAP_NS: u64 = 1_000_000_000;

/// A relative tempo change larger than this drops the older brackets:
/// they were laid out at the old tempo and would be projected wrongly.
pub const TEMPO_TOLERANCE: f64 = 0.0005;

/// Estimates less certain than this (half-width) are not reported. A
/// single bracket between two packets ~200 ms apart is about ±104 ms; a
/// lost packet doubles that, and a consumer that snaps its phase to each
/// observation would jump by a large part of a beat. The bracket is still
/// remembered, so the next beat's estimate can use it.
pub const MAX_UNCERTAINTY_NS: u64 = 150_000_000;

/// When a beat is estimated to have started.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BeatEstimate {
    /// Beat counter value of the beat that started.
    pub beat: u32,
    /// Estimated start, host nanoseconds.
    pub host_ns: u64,
    /// Half-width of the window the start must lie in.
    pub uncertainty_ns: u64,
    /// How many beats' brackets agreed on the estimate.
    pub beats_used: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Bracket {
    beat: u32,
    lo: u64,
    hi: u64,
}

/// Estimates beat start times from a stream of `(time, beat counter)`
/// reports. Fixed size, no allocation.
#[derive(Clone, Debug, Default)]
pub struct BeatTracker {
    last: Option<(u64, u32)>,
    bpm: Option<f64>,
    ring: [Bracket; WINDOW],
    len: usize,
    head: usize,
}

impl BeatTracker {
    /// An empty tracker.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Forgets everything (new deck, deck lost).
    pub fn reset(&mut self) {
        self.last = None;
        self.bpm = None;
        self.clear();
    }

    /// Number of beats currently remembered.
    #[must_use]
    pub fn beats_remembered(&self) -> usize {
        self.len
    }

    fn clear(&mut self) {
        self.len = 0;
    }

    fn push(&mut self, b: Bracket) {
        self.head = (self.head + 1) % WINDOW;
        self.ring[self.head] = b;
        self.len = (self.len + 1).min(WINDOW);
    }

    fn nth_newest(&self, i: usize) -> Bracket {
        self.ring[(self.head + WINDOW - i) % WINDOW]
    }

    /// Feeds one status report received at `host_ns`. Returns an estimate
    /// when this report shows the next beat starting, unless it is less
    /// certain than [`MAX_UNCERTAINTY_NS`].
    ///
    /// `beat` is the beat counter (`None` if unknown), `playing` whether
    /// the deck plays, `bpm` its effective tempo.
    pub fn update(
        &mut self,
        host_ns: u64,
        beat: Option<u32>,
        playing: bool,
        bpm: Option<f64>,
    ) -> Option<BeatEstimate> {
        let Some(beat) = beat else {
            self.reset();
            return None;
        };
        let (prev_ns, prev_beat) = self.last.replace((host_ns, beat))?;
        if host_ns < prev_ns || !playing {
            self.clear();
            return None;
        }
        if beat == prev_beat {
            return None;
        }
        // Only a step to the next beat is a beat boundary. Beat 0 is
        // "paused at the start", so 0 -> 1 is the play button, not a beat.
        if prev_beat == 0 || prev_beat.checked_add(1) != Some(beat) {
            self.clear();
            return None;
        }
        let bpm = bpm.filter(|b| b.is_finite() && *b > 0.0);
        let tempo_moved = match (self.bpm, bpm) {
            (Some(old), Some(new)) => ((new - old) / old).abs() > TEMPO_TOLERANCE,
            (None, None) => false,
            _ => true,
        };
        self.bpm = bpm;
        if host_ns - prev_ns > MAX_GAP_NS || tempo_moved {
            self.clear();
        }
        let newest = Bracket {
            beat,
            lo: prev_ns.saturating_sub(SLACK_NS),
            hi: host_ns.saturating_add(SLACK_NS),
        };
        // Brackets from before a jump never survive (`clear` above), so
        // the remembered beats are consecutive and older than `beat`.
        self.push(newest);

        let rel = |t: u64| (i128::from(t) - i128::from(host_ns)) as f64;
        let mut lo = rel(newest.lo);
        let mut hi = rel(newest.hi);
        let mut used = 1;
        match bpm.map(|b| 60.0e9 / b) {
            Some(period) => {
                while used < self.len {
                    let b = self.nth_newest(used);
                    let shift = f64::from(beat.wrapping_sub(b.beat)) * period;
                    let nlo = lo.max(rel(b.lo) + shift);
                    let nhi = hi.min(rel(b.hi) + shift);
                    if nlo > nhi {
                        break;
                    }
                    lo = nlo;
                    hi = nhi;
                    used += 1;
                }
                self.len = used;
            }
            None => self.len = 1,
        }
        let mid = ((lo + hi) / 2.0).min(0.0);
        let start = (i128::from(host_ns) + mid.round() as i128).max(0) as u64;
        let uncertainty_ns = ((hi - lo) / 2.0).max(0.0) as u64;
        (uncertainty_ns <= MAX_UNCERTAINTY_NS).then_some(BeatEstimate {
            beat,
            host_ns: start,
            uncertainty_ns,
            beats_used: used,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic xorshift for jitter.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        /// Uniform in `0..n`.
        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }
    }

    /// A simulated deck: beat `k` (k >= 1) starts at `origin + (k-1) * period`.
    struct Deck {
        origin: u64,
        period: f64,
    }
    impl Deck {
        fn beat_at(&self, t: u64) -> u32 {
            if t < self.origin {
                return 1;
            }
            ((t - self.origin) as f64 / self.period).floor() as u32 + 1
        }
        fn start_of(&self, beat: u32) -> u64 {
            self.origin + (f64::from(beat - 1) * self.period).round() as u64
        }
    }

    fn run(bpm: f64, interval_ns: u64, jitter_ns: u64, seconds: u64) -> Vec<(BeatEstimate, u64)> {
        let deck = Deck {
            origin: 1_000_000_000 + 123_456_789,
            period: 60.0e9 / bpm,
        };
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        let mut tracker = BeatTracker::new();
        let mut out = Vec::new();
        let mut t = 1_000_000_000;
        while t < 1_000_000_000 + seconds * 1_000_000_000 {
            // The packet is a snapshot at `t`, received 0.5–1.5 ms later.
            let received = t + 500_000 + rng.below(1_000_000);
            let beat = deck.beat_at(t);
            if let Some(est) = tracker.update(received, Some(beat), true, Some(bpm)) {
                out.push((est, deck.start_of(est.beat)));
            }
            t += interval_ns + rng.below(jitter_ns + 1);
        }
        out
    }

    #[test]
    fn single_bracket_is_within_half_a_packet_interval() {
        let mut tracker = BeatTracker::new();
        assert_eq!(
            tracker.update(1_000_000_000, Some(10), true, Some(120.0)),
            None
        );
        let est = tracker
            .update(1_200_000_000, Some(11), true, Some(120.0))
            .unwrap();
        assert_eq!(est.beat, 11);
        assert_eq!(est.beats_used, 1);
        // Midpoint of the bracket, which is symmetric around 1.1 s.
        assert_eq!(est.host_ns, 1_100_000_000);
        assert_eq!(est.uncertainty_ns, 100_000_000 + SLACK_NS);
    }

    #[test]
    fn brackets_converge_on_the_true_beat() {
        // Real cadence: ~200 ms with a few ms of jitter, at 128 BPM.
        let estimates = run(128.0, 200_000_000, 6_000_000, 20);
        assert!(estimates.len() > 35, "{}", estimates.len());
        for (i, (est, truth)) in estimates.iter().enumerate() {
            let err = est.host_ns.abs_diff(*truth);
            // Never worse than the single-bracket bound.
            assert!(err <= 110_000_000, "beat {i}: error {err} ns");
            // The truth is inside the claimed window (plus slack).
            assert!(
                err <= est.uncertainty_ns + SLACK_NS,
                "beat {i}: {est:?} vs {truth}"
            );
        }
        let late: Vec<u64> = estimates[8..]
            .iter()
            .map(|(e, t)| e.host_ns.abs_diff(*t))
            .collect();
        let worst = late.iter().copied().max().unwrap();
        assert!(worst < 40_000_000, "settled error {worst} ns");
    }

    #[test]
    fn commensurate_cadence_still_beats_a_single_bracket() {
        // 120 BPM against exactly 200 ms packets: only five distinct
        // packet phases per beat, so the window settles near ±50 ms.
        let estimates = run(120.0, 200_000_000, 0, 12);
        let worst = estimates[6..]
            .iter()
            .map(|(e, t)| e.host_ns.abs_diff(*t))
            .max()
            .unwrap();
        assert!(worst <= 60_000_000, "{worst}");
    }

    #[test]
    fn jumps_pauses_and_unknown_beats_reset() {
        let mut t = BeatTracker::new();
        let bpm = Some(120.0);
        assert!(t.update(0, Some(5), true, bpm).is_none());
        assert!(t.update(200_000_000, Some(6), true, bpm).is_some());
        // Loop back: no estimate, window cleared.
        assert!(t.update(400_000_000, Some(3), true, bpm).is_none());
        assert_eq!(t.beats_remembered(), 0);
        // Resumes on the next step.
        let e = t.update(600_000_000, Some(4), true, bpm).unwrap();
        assert_eq!(e.beats_used, 1);
        // Paused: nothing, and the window is cleared.
        assert!(t.update(800_000_000, Some(4), false, bpm).is_none());
        assert_eq!(t.beats_remembered(), 0);
        // Leaving the paused beat after resuming is a boundary again.
        assert!(t.update(1_000_000_000, Some(5), true, bpm).is_some());
        // Unknown beat: full reset.
        assert!(t.update(1_200_000_000, None, true, bpm).is_none());
        assert!(t.update(1_400_000_000, Some(6), true, bpm).is_none());
        // Start of track: 0 -> 1 is not a beat boundary.
        let mut s = BeatTracker::new();
        assert!(s.update(0, Some(0), false, bpm).is_none());
        assert!(s.update(200_000_000, Some(1), true, bpm).is_none());
        assert!(s.update(400_000_000, Some(2), true, bpm).is_some());
        // Time going backwards is ignored safely.
        assert!(s.update(100, Some(3), true, bpm).is_none());
    }

    #[test]
    fn tempo_change_drops_disagreeing_brackets() {
        let mut t = BeatTracker::new();
        let mut now = 1_000_000_000u64;
        let mut beat = 1u32;
        let mut next_beat_at = now + 100_000_000;
        let mut period = 500_000_000u64;
        let mut last_est = None;
        for step in 0..200 {
            if step == 100 {
                period = 400_000_000; // 120 -> 150 BPM
            }
            now += 200_000_000;
            while next_beat_at <= now {
                beat += 1;
                next_beat_at += period;
            }
            let bpm = 60.0e9 / period as f64;
            if let Some(e) = t.update(now, Some(beat), true, Some(bpm)) {
                let truth = next_beat_at - period;
                assert!(e.host_ns.abs_diff(truth) <= 110_000_000, "step {step}");
                last_est = Some(e);
            }
        }
        assert!(last_est.unwrap().beats_used > 1);
    }

    #[test]
    fn a_lost_packet_does_not_report_a_wide_bracket() {
        // 100 BPM, packets every 200 ms, but the one at 400 ms is lost:
        // the step from beat 5 to 6 is only known to within ±204 ms.
        let mut t = BeatTracker::new();
        let bpm = Some(100.0);
        assert!(t.update(0, Some(5), true, bpm).is_none());
        assert!(t.update(200_000_000, Some(5), true, bpm).is_none());
        assert!(t.update(600_000_000, Some(6), true, bpm).is_none());
        // The wide bracket is remembered and agrees with the next one.
        assert_eq!(t.beats_remembered(), 1);
        assert!(t.update(800_000_000, Some(6), true, bpm).is_none());
        assert!(t.update(1_000_000_000, Some(6), true, bpm).is_none());
        let e = t.update(1_200_000_000, Some(7), true, bpm).unwrap();
        assert_eq!(e.beats_used, 2);
        assert!(e.uncertainty_ns <= 100_000_000 + SLACK_NS);
    }

    #[test]
    fn extreme_inputs_never_panic() {
        let mut rng = Rng(0xdead_beef_cafe_f00d);
        let mut t = BeatTracker::new();
        let specials = [0, 1, u64::MAX - 1, u64::MAX, 1 << 63];
        let bpms = [
            None,
            Some(f64::NAN),
            Some(f64::INFINITY),
            Some(-1.0),
            Some(0.0),
            Some(f64::MIN_POSITIVE),
            Some(1e-8),
            Some(128.0),
            Some(1e300),
        ];
        let mut now = 0u64;
        let mut beat = 1u32;
        for i in 0..50_000u32 {
            now = match rng.below(10) {
                0 => specials[rng.below(specials.len() as u64) as usize],
                _ => now.wrapping_add(rng.below(400_000_000)),
            };
            beat = match rng.below(8) {
                0 => rng.next() as u32,
                1 => u32::MAX,
                2 => 0,
                _ => beat.wrapping_add(rng.below(2) as u32),
            };
            let b = bpms[rng.below(bpms.len() as u64) as usize];
            if let Some(e) = t.update(now, Some(beat), i % 13 != 0, b) {
                assert!(e.uncertainty_ns <= MAX_UNCERTAINTY_NS);
                assert!((1..=WINDOW).contains(&e.beats_used));
                assert!(e.host_ns <= now);
            }
        }
    }

    #[test]
    fn missing_tempo_uses_the_newest_bracket_only() {
        let mut t = BeatTracker::new();
        t.update(0, Some(1), true, None);
        t.update(200_000_000, Some(2), true, None);
        t.update(400_000_000, Some(2), true, None);
        let e = t.update(600_000_000, Some(3), true, None).unwrap();
        assert_eq!(e.beats_used, 1);
    }
}
