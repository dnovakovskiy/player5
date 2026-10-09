//! Ableton Link as a clock source (cargo feature `ableton-link`, off by
//! default).
//!
//! [`start`] joins (or starts) a Link session on a background thread, polls
//! the session state every few milliseconds and reports what it sees as
//! [`SourceEvent`]s: a bar-phase [`SourceEvent::Observation`] per poll,
//! timestamped on [`crate::host_time`], and a [`SourceEvent::Status`]
//! whenever the number of peers changes. Sessions faster than the
//! follower accepts are reported at half or quarter tempo.
//!
//! This peer only listens. It never commits a session state, so it cannot
//! change the session's tempo or beat grid; joining adopts whatever the
//! session already plays. Start/stop sync stays disabled.
//!
//! Link runs on its own clock (a microsecond reading of the host's
//! monotonic clock). Every poll reads that clock back to back with
//! [`crate::host_time::now_ns`] and stamps the observation with the host
//! time of the very instant whose beat it reports, so consumers never deal
//! with Link time. Protocol notes, sources and the time-base details:
//! `docs/protocols/ableton-link.md`. Licensing: ADR-0005 (the Link SDK is
//! GPLv2+ unless Ableton licenses it otherwise).
//!
//! Nothing here runs on an audio thread.

use std::io;
use std::time::Duration;

use rusty_link::{AblLink, SessionState};

use crate::follower::{FollowerClock, Phase, Precision};
use crate::host_time;
use crate::net::{SourceContext, SourceEvent, SourceHandle};

/// The Link quantum player5 uses: one 4-beat bar, so Link's shared phase
/// is the position in the bar ([`Phase::Bar`]).
pub const QUANTUM: f64 = 4.0;

/// Lowest session tempo Link allows (see the protocol notes, "Tempo").
pub const MIN_BPM: f64 = 20.0;

/// Highest session tempo Link allows (see the protocol notes, "Tempo").
pub const MAX_BPM: f64 = 999.0;

/// Fastest tempo this source reports: what [`FollowerClock`] accepts.
/// Faster sessions are followed at half or quarter tempo (see the protocol
/// notes, "Tempo").
pub const MAX_FOLLOW_BPM: f64 = FollowerClock::MAX_BPM;

/// Shortest accepted poll interval.
const MIN_POLL: Duration = Duration::from_millis(1);

/// Longest accepted poll interval. The source thread must check its stop
/// flag at least every 100 ms (see [`SourceContext`]).
const MAX_POLL: Duration = Duration::from_millis(50);

/// How many times to try for a tight host/Link clock reading per poll.
const CLOCK_SAMPLE_TRIES: usize = 3;

/// A host/Link clock reading whose bracketing host reads are at most this
/// far apart is taken as is; a wider one means the thread was preempted
/// mid-reading, so try again.
const TIGHT_BRACKET_NS: u64 = 20_000;

/// Settings for [`start`].
#[derive(Clone, Debug, PartialEq)]
pub struct LinkConfig {
    /// Tempo of the session this peer starts if it finds no session to
    /// join. Joining an existing session adopts that session's tempo; this
    /// value never overrides it. Clamped to [`MIN_BPM`]..=[`MAX_BPM`].
    pub initial_bpm: f64,
    /// How often to sample the session state and report an observation.
    /// Clamped to 1..=50 ms.
    pub poll_interval: Duration,
}

impl Default for LinkConfig {
    fn default() -> Self {
        Self {
            initial_bpm: 120.0,
            poll_interval: Duration::from_millis(5),
        }
    }
}

/// Enables a Link peer and follows its session on a background thread.
///
/// The handle's [`SourceHandle::name`] is `"link"`. Observations carry
/// [`Phase::Bar`] (the session phase for a quantum of [`QUANTUM`] beats),
/// the session tempo, [`Precision::Exact`] and no device number. A session
/// faster than [`MAX_FOLLOW_BPM`] is reported at half or quarter tempo,
/// with the bar phase of 2 or 4 session bars. Status messages report the
/// peer count when it changes, and when that tempo divisor changes. [`SourceCommand`]s
/// are accepted and ignored: a Link session has no devices to choose
/// between. Dropping or stopping the handle leaves the session.
///
/// # Errors
///
/// `InvalidInput` if `initial_bpm` is not finite; otherwise only if the
/// thread cannot be spawned.
///
/// [`SourceCommand`]: crate::net::SourceCommand
pub fn start(config: LinkConfig) -> io::Result<SourceHandle> {
    if !config.initial_bpm.is_finite() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Ableton Link: the initial tempo must be a finite number",
        ));
    }
    let bpm = config.initial_bpm.clamp(MIN_BPM, MAX_BPM);
    let interval = config.poll_interval.clamp(MIN_POLL, MAX_POLL);
    SourceHandle::spawn("link", move |ctx| run(&ctx, bpm, interval))
}

/// The source thread.
fn run(ctx: &SourceContext, initial_bpm: f64, interval: Duration) {
    let link = AblLink::new(initial_bpm);
    link.enable(true);
    let mut state = SessionState::new();
    let mut peers: Option<u64> = None;
    // Session beats per reported beat, and whether a slowed-down follow
    // was ever announced (then the return to full tempo is, too).
    let mut divisor: Option<u32> = None;
    let mut divisor_announced = false;

    let _ = ctx.send(SourceEvent::Status {
        warning: false,
        message: format!("Ableton Link enabled; {initial_bpm:.2} BPM until a session is joined"),
    });

    while !ctx.should_stop() {
        // Following a particular device means nothing in a Link session.
        while ctx.commands.try_recv().is_ok() {}

        let n = link.num_peers();
        if peers != Some(n) {
            peers = Some(n);
            let status = SourceEvent::Status {
                warning: false,
                message: peers_message(n),
            };
            if !ctx.send(status) {
                break;
            }
        }

        link.capture_app_session_state(&mut state);
        let now = sample_clocks(host_time::now_ns, || link.clock_micros());
        let session_bpm = state.tempo();
        if let Some(report) = follow(session_bpm, |q| state.beat_at_time(now.link_us, q)) {
            if divisor != Some(report.divisor) {
                divisor = Some(report.divisor);
                if report.divisor > 1 || divisor_announced {
                    divisor_announced = true;
                    let status = SourceEvent::Status {
                        warning: false,
                        message: divisor_message(session_bpm, report),
                    };
                    if !ctx.send(status) {
                        break;
                    }
                }
            }
            let observation = SourceEvent::Observation {
                host_ns: now.host_ns,
                phase: Phase::Bar(report.phase),
                bpm: Some(report.bpm),
                precision: Precision::Exact,
                device: None,
            };
            if !ctx.send(observation) {
                break;
            }
        }

        std::thread::sleep(interval);
    }

    link.enable(false);
}

/// The status line for a peer count. "Ableton Link" is written out in full
/// as Ableton's integration guidelines ask (see the protocol notes).
fn peers_message(peers: u64) -> String {
    match peers {
        0 => "Ableton Link: no peers".to_owned(),
        1 => "Ableton Link: 1 peer".to_owned(),
        n => format!("Ableton Link: {n} peers"),
    }
}

/// What one poll reports.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Report {
    /// Bar phase, `0.0..QUANTUM`, in reported beats.
    phase: f64,
    /// Reported tempo: the session tempo divided by `divisor`.
    bpm: f64,
    /// Session beats per reported beat: 1, 2 or 4.
    divisor: u32,
}

/// How many session beats make one reported beat at session tempo `bpm`:
/// the smallest power of two that brings it to at most [`MAX_FOLLOW_BPM`].
/// Link sessions reach 999 BPM, so this is at most 4.
fn tempo_divisor(bpm: f64) -> u32 {
    let mut k = 1u32;
    while bpm / f64::from(k) > MAX_FOLLOW_BPM && k < 4 {
        k *= 2;
    }
    k
}

/// Turns the session tempo and the session timeline (`beat_at(quantum)` =
/// Link's `beatAtTime(t, quantum)` at the observed instant) into what the
/// source reports. A session faster than [`MAX_FOLLOW_BPM`] is followed
/// at half or quarter tempo, as Ableton's test plan (TEMPO-4) asks: the
/// reported bar then spans 2 or 4 session bars, and its phase is taken
/// with the matching quantum so it is the one every peer shares. `None` if
/// Link returned something unusable.
fn follow(bpm: f64, beat_at: impl FnOnce(f64) -> f64) -> Option<Report> {
    if !bpm.is_finite() || bpm <= 0.0 {
        return None;
    }
    let divisor = tempo_divisor(bpm);
    let k = f64::from(divisor);
    let beat = beat_at(QUANTUM * k);
    if !beat.is_finite() {
        return None;
    }
    Some(Report {
        // k is a power of two, so the division is exact.
        phase: bar_phase(beat / k),
        bpm: bpm / k,
        divisor,
    })
}

/// The status line when the tempo divisor changes.
fn divisor_message(session_bpm: f64, report: Report) -> String {
    match report.divisor {
        1 => format!("Ableton Link: following the session at full tempo ({session_bpm:.2} BPM)"),
        2 => format!(
            "Ableton Link: session at {session_bpm:.2} BPM; following at half tempo ({:.2} BPM)",
            report.bpm
        ),
        _ => format!(
            "Ableton Link: session at {session_bpm:.2} BPM; following at quarter tempo ({:.2} BPM)",
            report.bpm
        ),
    }
}

/// Position in the bar, `0.0..QUANTUM`, of a Link beat value. Beat values
/// may be negative after joining a session; the phase still is not.
fn bar_phase(beat: f64) -> f64 {
    let phase = beat.rem_euclid(QUANTUM);
    // `rem_euclid` rounds tiny negative values up to exactly QUANTUM.
    if phase < QUANTUM {
        phase
    } else {
        0.0
    }
}

/// One instant on both clocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ClockPair {
    /// Host time, nanoseconds ([`host_time::now_ns`]).
    host_ns: u64,
    /// Link time, microseconds (`AblLink::clock_micros`).
    link_us: i64,
}

/// Reads the Link clock between two host-clock reads and pairs it with
/// their midpoint. Retries (keeping the tightest) when the two host reads
/// are far apart, which means the thread was preempted in between.
fn sample_clocks(mut host_ns: impl FnMut() -> u64, mut link_us: impl FnMut() -> i64) -> ClockPair {
    let mut read = || {
        let before = host_ns();
        let link = link_us();
        let after = host_ns();
        let width = after.saturating_sub(before);
        (
            width,
            ClockPair {
                host_ns: before + width / 2,
                link_us: link,
            },
        )
    };
    let (mut best_width, mut best) = read();
    for _ in 1..CLOCK_SAMPLE_TRIES {
        if best_width <= TIGHT_BRACKET_NS {
            break;
        }
        let (width, pair) = read();
        if width < best_width {
            best_width = width;
            best = pair;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};
    use std::time::Instant;

    /// Every Link instance in this process joins the same session, so the
    /// tests that enable one run one at a time.
    fn network_lock() -> MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// How long to wait for two peers on this machine to find each other.
    /// Discovery normally takes well under a second; it needs IPv4
    /// multicast with loopback on some running interface.
    const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(10);

    /// Allowed phase disagreement, in beats (0.5 ms at 120 BPM).
    const PHASE_TOLERANCE: f64 = 1e-3;

    fn parse_peers(message: &str) -> Option<u64> {
        let rest = message.strip_prefix("Ableton Link: ")?;
        if rest == "no peers" {
            return Some(0);
        }
        rest.split(' ').next()?.parse().ok()
    }

    /// Circular distance between two bar phases.
    fn phase_distance(a: f64, b: f64) -> f64 {
        let d = (a - b).rem_euclid(QUANTUM);
        d.min(QUANTUM - d)
    }

    #[derive(Clone, Copy, Debug)]
    struct Obs {
        host_ns: u64,
        phase: f64,
        bpm: f64,
    }

    /// What a test has seen from one source so far.
    #[derive(Default)]
    struct Seen {
        peers: Option<u64>,
        /// Highest peer count ever reported.
        max_peers: u64,
        last: Option<Obs>,
        observations: usize,
        statuses: Vec<String>,
    }

    impl Seen {
        fn absorb(&mut self, event: SourceEvent) {
            match event {
                SourceEvent::Observation {
                    host_ns,
                    phase,
                    bpm,
                    precision,
                    device,
                } => {
                    assert_eq!(precision, Precision::Exact);
                    assert_eq!(device, None);
                    let Phase::Bar(phase) = phase else {
                        panic!("Link reports bar phase, got {phase:?}");
                    };
                    assert!((0.0..QUANTUM).contains(&phase), "phase {phase}");
                    let bpm = bpm.expect("Link always reports a tempo");
                    assert!((MIN_BPM..=MAX_FOLLOW_BPM).contains(&bpm), "bpm {bpm}");
                    if let Some(prev) = self.last {
                        assert!(host_ns >= prev.host_ns, "host time went backwards");
                    }
                    self.last = Some(Obs {
                        host_ns,
                        phase,
                        bpm,
                    });
                    self.observations += 1;
                }
                SourceEvent::Status { warning, message } => {
                    assert!(!warning, "unexpected warning: {message}");
                    if let Some(n) = parse_peers(&message) {
                        self.peers = Some(n);
                        self.max_peers = self.max_peers.max(n);
                    }
                    self.statuses.push(message);
                }
                SourceEvent::Devices(_) => panic!("Link reports no devices"),
            }
        }

        fn drain(&mut self, source: &SourceHandle) {
            while let Some(event) = source.try_recv() {
                self.absorb(event);
            }
        }
    }

    #[test]
    fn bar_phase_wraps_into_the_bar() {
        assert_eq!(bar_phase(0.0), 0.0);
        assert_eq!(bar_phase(5.5), 1.5);
        assert_eq!(bar_phase(-0.5), 3.5);
        assert_eq!(bar_phase(-4.0), 0.0);
        let tiny = bar_phase(-1e-18);
        assert!((0.0..QUANTUM).contains(&tiny), "{tiny}");
    }

    #[test]
    fn tempo_divisor_halves_until_followable() {
        assert_eq!(tempo_divisor(MIN_BPM), 1);
        assert_eq!(tempo_divisor(120.0), 1);
        assert_eq!(tempo_divisor(MAX_FOLLOW_BPM), 1);
        assert_eq!(tempo_divisor(MAX_FOLLOW_BPM + 0.01), 2);
        assert_eq!(tempo_divisor(2.0 * MAX_FOLLOW_BPM), 2);
        assert_eq!(tempo_divisor(2.0 * MAX_FOLLOW_BPM + 0.01), 4);
        assert_eq!(tempo_divisor(MAX_BPM), 4);
        // Every Link tempo ends up followable.
        let mut bpm = MIN_BPM;
        while bpm <= MAX_BPM {
            let k = f64::from(tempo_divisor(bpm));
            assert!(bpm / k <= MAX_FOLLOW_BPM, "{bpm} BPM / {k}");
            assert!(bpm / k >= MIN_BPM, "{bpm} BPM / {k}");
            bpm += 0.5;
        }
    }

    /// A fake Link timeline: the session is at global beat `global`, and
    /// like Link, the beat magnitude it returns is this peer's own (here
    /// offset by three quanta) while its phase in the quantum is shared.
    fn fake_timeline(global: f64, quanta: &mut Vec<f64>) -> impl FnOnce(f64) -> f64 + '_ {
        move |q| {
            quanta.push(q);
            global - 3.0 * q
        }
    }

    #[test]
    fn follow_reports_the_session_as_is_up_to_the_follower_limit() {
        let mut quanta = Vec::new();
        let r = follow(128.0, fake_timeline(13.25, &mut quanta)).unwrap();
        assert_eq!(quanta, [QUANTUM]);
        assert_eq!(
            r,
            Report {
                phase: 1.25,
                bpm: 128.0,
                divisor: 1
            }
        );
    }

    #[test]
    fn follow_reports_a_fast_session_at_half_or_quarter_tempo() {
        // 800 BPM: half tempo, one reported bar = two session bars, phase
        // taken with quantum 8 (the shared one), not quantum 4.
        let mut quanta = Vec::new();
        let r = follow(800.0, fake_timeline(13.0, &mut quanta)).unwrap();
        assert_eq!(quanta, [8.0]);
        assert_eq!(
            r,
            Report {
                phase: 2.5,
                bpm: 400.0,
                divisor: 2
            }
        );
        // 999 BPM: quarter tempo, quantum 16.
        let mut quanta = Vec::new();
        let r = follow(MAX_BPM, fake_timeline(30.0, &mut quanta)).unwrap();
        assert_eq!(quanta, [16.0]);
        assert_eq!(r.divisor, 4);
        assert_eq!(r.phase, 3.5);
        assert_eq!(r.bpm, MAX_BPM / 4.0);
    }

    #[test]
    fn follow_rejects_unusable_values() {
        for bpm in [f64::NAN, f64::INFINITY, 0.0, -120.0] {
            assert_eq!(follow(bpm, |_| 1.0), None, "{bpm}");
        }
        assert_eq!(follow(120.0, |_| f64::NAN), None);
        assert_eq!(follow(120.0, |_| f64::INFINITY), None);
    }

    #[test]
    fn divisor_messages_name_the_tempi() {
        let half = Report {
            phase: 0.0,
            bpm: 400.0,
            divisor: 2,
        };
        let msg = divisor_message(800.0, half);
        assert!(msg.contains("800.00") && msg.contains("half") && msg.contains("400.00"));
        assert_eq!(parse_peers(&msg), None);
        let full = Report { divisor: 1, ..half };
        assert_eq!(parse_peers(&divisor_message(120.0, full)), None);
    }

    #[test]
    fn peer_messages_round_trip() {
        for n in [0, 1, 2, 17] {
            assert_eq!(parse_peers(&peers_message(n)), Some(n));
        }
        assert_eq!(parse_peers("Ableton Link enabled; 120.00 BPM"), None);
    }

    #[test]
    fn clock_pair_uses_the_midpoint_of_a_tight_bracket() {
        let mut host = [1_000u64, 1_400].into_iter();
        let pair = sample_clocks(|| host.next().unwrap(), || 77);
        assert_eq!(
            pair,
            ClockPair {
                host_ns: 1_200,
                link_us: 77
            }
        );
    }

    #[test]
    fn clock_pair_retries_after_preemption() {
        // First reading straddles a 5 ms stall; the second is tight.
        let mut host = [0u64, 5_000_000, 6_000_000, 6_000_200].into_iter();
        let mut link = [10i64, 6_000].into_iter();
        let pair = sample_clocks(|| host.next().unwrap(), || link.next().unwrap());
        assert_eq!(
            pair,
            ClockPair {
                host_ns: 6_000_100,
                link_us: 6_000
            }
        );
    }

    #[test]
    fn clock_pair_keeps_the_tightest_of_wide_readings() {
        let mut host = [0u64, 900_000, 1_000_000, 1_300_000, 2_000_000, 2_500_000].into_iter();
        let mut link = [1i64, 2, 3].into_iter();
        let pair = sample_clocks(|| host.next().unwrap(), || link.next().unwrap());
        assert_eq!(
            pair,
            ClockPair {
                host_ns: 1_150_000,
                link_us: 2
            }
        );
    }

    #[test]
    fn rejects_a_non_finite_tempo() {
        let config = LinkConfig {
            initial_bpm: f64::NAN,
            ..LinkConfig::default()
        };
        let err = start(config).err().expect("NaN tempo must be rejected");
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    /// A lone peer reports its initial tempo, a steadily advancing phase,
    /// and stops promptly.
    #[test]
    fn reports_observations_and_stops() {
        let _guard = network_lock();
        let source = start(LinkConfig {
            initial_bpm: 123.0,
            ..LinkConfig::default()
        })
        .unwrap();
        assert_eq!(source.name(), "link");

        let mut seen = Seen::default();
        let mut pairs_checked = 0;
        let deadline = Instant::now() + Duration::from_secs(5);
        while seen.observations < 40 && Instant::now() < deadline {
            let prev = seen.last;
            if let Some(event) = source.recv_timeout(Duration::from_millis(200)) {
                seen.absorb(event);
            }
            // Consecutive observations agree with each other: the phase
            // advances by elapsed host time × tempo.
            if let (Some(a), Some(b)) = (prev, seen.last) {
                if a.host_ns != b.host_ns && a.bpm == b.bpm {
                    let beats = (b.host_ns - a.host_ns) as f64 / 60e9 * a.bpm;
                    let expected = (a.phase + beats).rem_euclid(QUANTUM);
                    let err = phase_distance(expected, b.phase);
                    assert!(err < PHASE_TOLERANCE, "phase drift {err} beats");
                    pairs_checked += 1;
                }
            }
        }
        assert!(
            seen.observations >= 40,
            "only {} observations",
            seen.observations
        );
        assert!(pairs_checked >= 20);
        assert!(seen.statuses[0].starts_with("Ableton Link enabled"));
        // Alone, the session keeps our tempo. (Another Link app on this
        // network, such as a test in another process, would legitimately
        // take over, so only check when no peer was ever seen.)
        if seen.peers == Some(0) && seen.max_peers == 0 {
            let bpm = seen.last.unwrap().bpm;
            assert!((bpm - 123.0).abs() < 1e-3, "bpm {bpm}");
        }

        let t0 = Instant::now();
        source.stop();
        assert!(
            t0.elapsed() < Duration::from_secs(1),
            "stop took {:?}",
            t0.elapsed()
        );
    }

    /// Two sources in one process find each other, then agree on tempo and
    /// on the bar phase at the same host instant.
    #[test]
    fn two_sources_discover_each_other_and_agree() {
        let _guard = network_lock();
        let a = start(LinkConfig {
            initial_bpm: 100.0,
            ..LinkConfig::default()
        })
        .unwrap();
        let b = start(LinkConfig {
            initial_bpm: 131.0,
            ..LinkConfig::default()
        })
        .unwrap();
        let (mut seen_a, mut seen_b) = (Seen::default(), Seen::default());

        let deadline = Instant::now() + DISCOVERY_TIMEOUT;
        while !(seen_a.peers >= Some(1) && seen_b.peers >= Some(1)) {
            assert!(
                Instant::now() < deadline,
                "the two Link peers did not discover each other within {DISCOVERY_TIMEOUT:?} \
                 (needs IPv4 multicast with loopback; peers: {:?} / {:?})",
                seen_a.peers,
                seen_b.peers
            );
            std::thread::sleep(Duration::from_millis(20));
            seen_a.drain(&a);
            seen_b.drain(&b);
        }

        // Let both pick up the merged session, then compare fresh reports.
        std::thread::sleep(Duration::from_millis(300));
        for _ in 0..10 {
            std::thread::sleep(Duration::from_millis(20));
            seen_a.drain(&a);
            seen_b.drain(&b);
            let (oa, ob) = (seen_a.last.unwrap(), seen_b.last.unwrap());
            assert!(
                (oa.bpm - ob.bpm).abs() < 1e-3,
                "tempo disagrees: {} vs {}",
                oa.bpm,
                ob.bpm
            );
            // Carry b's report forward (or back) to a's instant.
            let dt_s = (oa.host_ns as f64 - ob.host_ns as f64) / 1e9;
            let b_at_a = (ob.phase + dt_s * ob.bpm / 60.0).rem_euclid(QUANTUM);
            let err = phase_distance(oa.phase, b_at_a);
            assert!(err < PHASE_TOLERANCE, "phase disagrees by {err} beats");
        }
        a.stop();
        b.stop();
    }

    /// The host-time stamp is right: an independent Link peer, evaluated
    /// on its own clock at the observation's host instant, sees the same
    /// bar phase. And the peer notices when the source leaves.
    #[test]
    fn observations_match_an_independent_peer() {
        let _guard = network_lock();
        let peer = AblLink::new(140.0);
        peer.enable(true);
        let source = start(LinkConfig::default()).unwrap();
        let mut seen = Seen::default();

        let deadline = Instant::now() + DISCOVERY_TIMEOUT;
        while !(seen.peers >= Some(1) && peer.num_peers() >= 1) {
            assert!(
                Instant::now() < deadline,
                "Link peers did not discover each other within {DISCOVERY_TIMEOUT:?} \
                 (needs IPv4 multicast with loopback)"
            );
            std::thread::sleep(Duration::from_millis(20));
            seen.drain(&source);
        }
        std::thread::sleep(Duration::from_millis(300));

        let mut state = SessionState::new();
        for _ in 0..10 {
            std::thread::sleep(Duration::from_millis(20));
            seen.drain(&source);
            let obs = seen.last.unwrap();
            // Map the observation's host time onto the peer's Link clock.
            let link_t = peer_link_time(&peer, obs.host_ns);
            peer.capture_app_session_state(&mut state);
            let expected = bar_phase(state.beat_at_time(link_t, QUANTUM));
            let err = phase_distance(obs.phase, expected);
            assert!(err < PHASE_TOLERANCE, "phase off by {err} beats");
            assert!((obs.bpm - state.tempo()).abs() < 1e-3);
        }

        // Other Link apps on this network may be in the session too, so
        // look for the count to drop rather than for zero.
        let before = peer.num_peers();
        source.stop();
        let deadline = Instant::now() + Duration::from_secs(3);
        while peer.num_peers() >= before && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            peer.num_peers() < before,
            "the source did not leave the session"
        );
        peer.enable(false);
    }

    /// A session tempo above what the follower takes (TEMPO-4): an
    /// independent peer pushes the session to 800 BPM; the source follows
    /// at half tempo with the bar phase every peer shares for an 8-beat
    /// quantum, says so, and returns to full tempo when the session does.
    ///
    /// Ignored by default: unlike the other tests, which only join, this
    /// one commits tempo changes, and those reach every Link app in the
    /// session on the LAN (and any other test process running at the same
    /// time). Run it on an isolated machine with
    /// `cargo test -p sync --features ableton-link -- --ignored`.
    /// `follow_reports_a_fast_session_at_half_or_quarter_tempo` covers the
    /// same arithmetic offline.
    #[test]
    #[ignore = "commits a tempo change to every Link session on the LAN"]
    fn follows_a_fast_session_at_half_tempo() {
        let _guard = network_lock();
        let peer = AblLink::new(120.0);
        peer.enable(true);
        let source = start(LinkConfig::default()).unwrap();
        let mut seen = Seen::default();

        let deadline = Instant::now() + DISCOVERY_TIMEOUT;
        while !(seen.peers >= Some(1) && peer.num_peers() >= 1) {
            assert!(
                Instant::now() < deadline,
                "Link peers did not discover each other within {DISCOVERY_TIMEOUT:?} \
                 (needs IPv4 multicast with loopback)"
            );
            std::thread::sleep(Duration::from_millis(20));
            seen.drain(&source);
        }

        let mut state = SessionState::new();
        let set_tempo = |state: &mut SessionState, bpm: f64| {
            peer.capture_app_session_state(state);
            state.set_tempo(bpm, peer.clock_micros());
            peer.commit_app_session_state(state);
        };
        let wait_for_bpm = |seen: &mut Seen, bpm: f64| {
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                std::thread::sleep(Duration::from_millis(20));
                seen.drain(&source);
                if seen.last.is_some_and(|o| (o.bpm - bpm).abs() < 1e-6) {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "the source never reported {bpm} BPM (last: {:?})",
                    seen.last
                );
            }
        };

        set_tempo(&mut state, 800.0);
        wait_for_bpm(&mut seen, 400.0);
        assert!(
            seen.statuses.iter().any(|s| s.contains("half tempo")),
            "{:?}",
            seen.statuses
        );
        for _ in 0..10 {
            std::thread::sleep(Duration::from_millis(20));
            seen.drain(&source);
            let obs = seen.last.unwrap();
            peer.capture_app_session_state(&mut state);
            assert!((state.tempo() - 800.0).abs() < 1e-6);
            let link_t = peer_link_time(&peer, obs.host_ns);
            let expected = bar_phase(state.beat_at_time(link_t, 2.0 * QUANTUM) / 2.0);
            let err = phase_distance(obs.phase, expected);
            assert!(err < PHASE_TOLERANCE, "half-tempo phase off by {err} beats");
        }

        let statuses = seen.statuses.len();
        set_tempo(&mut state, 120.0);
        wait_for_bpm(&mut seen, 120.0);
        assert!(
            seen.statuses[statuses..]
                .iter()
                .any(|s| s.contains("full tempo")),
            "{:?}",
            seen.statuses
        );
        source.stop();
        peer.enable(false);
    }

    /// `peer`'s Link time at host time `host_ns` (in the recent past),
    /// from an independent clock reading (not `sample_clocks`), retried
    /// until it is not split by preemption.
    fn peer_link_time(peer: &AblLink, host_ns: u64) -> i64 {
        let (link_now, host_now) = loop {
            let before = host_time::now_ns();
            let link = peer.clock_micros();
            let after = host_time::now_ns();
            if after - before < 50_000 {
                break (link, before + (after - before) / 2);
            }
        };
        let ago_us = (host_now as f64 - host_ns as f64) / 1e3;
        link_now - ago_us.round() as i64
    }
}
