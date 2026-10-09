//! Ableton Link as a clock source (cargo feature `ableton-link`, off by
//! default).
//!
//! [`start`] joins (or starts) a Link session on a background thread, polls
//! the session state every few milliseconds and reports what it sees as
//! [`SourceEvent`]s: a bar-phase [`SourceEvent::Observation`] per poll,
//! timestamped on [`crate::host_time`], and a [`SourceEvent::Status`]
//! whenever the number of peers changes.
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

use crate::follower::{Phase, Precision};
use crate::host_time;
use crate::net::{SourceContext, SourceEvent, SourceHandle};

/// The Link quantum player5 uses: one 4-beat bar, so Link's shared phase
/// is the position in the bar ([`Phase::Bar`]).
pub const QUANTUM: f64 = 4.0;

/// Lowest session tempo Link allows (see the protocol notes, "Tempo").
pub const MIN_BPM: f64 = 20.0;

/// Highest session tempo Link allows (see the protocol notes, "Tempo").
pub const MAX_BPM: f64 = 999.0;

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
/// the session tempo, [`Precision::Exact`] and no device number. Status
/// messages report the peer count when it changes. [`SourceCommand`]s
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
        let beat = state.beat_at_time(now.link_us, QUANTUM);
        let bpm = state.tempo();
        if beat.is_finite() && bpm.is_finite() && bpm > 0.0 {
            let observation = SourceEvent::Observation {
                host_ns: now.host_ns,
                phase: Phase::Bar(bar_phase(beat)),
                bpm: Some(bpm),
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
                    assert!((MIN_BPM..=MAX_BPM + 0.01).contains(&bpm), "bpm {bpm}");
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
        // network would legitimately take over, so only check when alone.)
        if seen.peers == Some(0) {
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
            // Map the observation's host time onto the peer's Link clock
            // with an independent reading (not `sample_clocks`), retried
            // until it is not split by preemption.
            let (link_now, host_now) = loop {
                let before = host_time::now_ns();
                let link = peer.clock_micros();
                let after = host_time::now_ns();
                if after - before < 50_000 {
                    break (link, before + (after - before) / 2);
                }
            };
            let ago_us = (host_now - obs.host_ns) as f64 / 1e3;
            let link_t = link_now - ago_us.round() as i64;
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
}
