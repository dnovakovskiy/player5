//! Fixture, fuzz and loopback tests for the Pro DJ Link source.
//!
//! Fixtures are hex dumps in `docs/protocols/fixtures/prolink/`. Most are
//! real hardware captures; the `constructed-*` and `player5-*` ones are
//! built from the documented layouts and act as golden bytes for our
//! builders (regenerate with `UPDATE_PROLINK_FIXTURES=1 cargo test -p sync
//! prolink`).

use std::net::{Ipv4Addr, UdpSocket};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::packets::*;
use super::source::{Received, Tracker};
use super::*;
use crate::follower::{Phase, Precision};
use crate::host_time;
use crate::net::{DeviceKind, FollowTarget, SourceCommand, SourceEvent, SourceHandle};

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/protocols/fixtures/prolink")
}

fn parse_hex(text: &str) -> Vec<u8> {
    text.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .flat_map(str::split_whitespace)
        .map(|b| u8::from_str_radix(b, 16).expect("hex byte"))
        .collect()
}

fn fixture(name: &str) -> Vec<u8> {
    let path = fixture_dir().join(format!("{name}.hex"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert!(
        text.lines()
            .any(|l| l.starts_with("# Documented in: https://")),
        "{name}: every fixture names its documenting source"
    );
    parse_hex(&text)
}

const REAL_FIXTURES: &[&str] = &[
    "keep-alive-cdj-2000nexus",
    "keep-alive-djm-2000nexus",
    "keep-alive-virtual-cdj",
    "hello-cdj-2000nexus",
    "claim-stage1-cdj-2000nexus",
    "claim-stage2-cdj-2000nexus",
    "claim-stage3-cdj-2000nexus",
    "assignment-intention-djm-2000nexus",
    "assignment-request-cdj-2000nexus",
    "assignment-djm-2000nexus",
    "assignment-finished-cdj-2000nexus",
    "beat-cdj-2000nexus",
    "beat-djm-2000nexus",
    "on-air-djm-2000nexus",
    "status-cdj-2000nexus-playing-master",
    "status-cdj-2000nexus-idle",
    "status-djm-2000nexus",
];

// ---------------------------------------------------------------------------
// Real captures.

#[test]
fn real_keep_alives() {
    let k = parse_keep_alive(&fixture("keep-alive-cdj-2000nexus")).unwrap();
    assert_eq!(k.number, 3);
    assert_eq!(k.name, "CDJ-2000nexus");
    assert_eq!(k.ip, Ipv4Addr::new(172, 16, 42, 3));
    assert_eq!(k.mac, [0x74, 0x5e, 0x1c, 0x56, 0xc0, 0x70]);
    assert_eq!((k.peers, k.device_type, k.first_on_network), (4, 1, false));
    assert!(!k.cdj3000_compatible);
    assert_eq!(k.kind(), DeviceKind::Player);

    let m = parse_keep_alive(&fixture("keep-alive-djm-2000nexus")).unwrap();
    assert_eq!(
        (m.number, m.device_type, m.kind()),
        (0x21, 2, DeviceKind::Mixer)
    );
    assert_eq!(m.name, "DJM-2000nexus");

    let v = parse_keep_alive(&fixture("keep-alive-virtual-cdj")).unwrap();
    assert_eq!((v.number, v.name.as_str()), (5, "Virtual CDJ"));
    assert_eq!(v.ip, Ipv4Addr::new(172, 16, 42, 2));
}

#[test]
fn real_startup_packets() {
    let hello = fixture("hello-cdj-2000nexus");
    assert_eq!(
        PacketKind::classify(Port::Announce, &hello),
        Some(PacketKind::Hello)
    );
    assert_eq!(device_name(Port::Announce, &hello), "CDJ-2000nexus");

    let c1 = parse_number_claim(&fixture("claim-stage1-cdj-2000nexus")).unwrap();
    assert_eq!((c1.stage, c1.counter, c1.number), (1, 1, None));
    assert_eq!(c1.mac, Some([0x74, 0x5e, 0x1c, 0x56, 0xf4, 0xb5]));

    let c2 = parse_number_claim(&fixture("claim-stage2-cdj-2000nexus")).unwrap();
    assert_eq!((c2.stage, c2.counter, c2.number), (2, 1, Some(1)));
    assert_eq!(c2.ip, Some(Ipv4Addr::new(169, 254, 103, 172)));
    assert_eq!(c2.mac, Some([0x74, 0x5e, 0x1c, 0x56, 0x67, 0xac]));
    assert_eq!(
        (c2.auto_assign, c2.assignment_request),
        (Some(false), false)
    );

    let c3 = parse_number_claim(&fixture("claim-stage3-cdj-2000nexus")).unwrap();
    assert_eq!((c3.stage, c3.counter, c3.number), (3, 1, Some(1)));
}

#[test]
fn real_mixer_assignment_exchange_matches_our_builders() {
    let mixer_ip = Ipv4Addr::new(169, 254, 99, 60);
    let mixer_mac = [0x74, 0x5e, 0x1c, 0x35, 0x63, 0x3c];
    let intention = fixture("assignment-intention-djm-2000nexus");
    assert_eq!(
        PacketKind::classify(Port::Announce, &intention),
        Some(PacketKind::AssignmentIntention)
    );
    assert_eq!(
        build_assignment_intention("DJM-2000nexus", mixer_ip, mixer_mac),
        intention
    );

    let request = fixture("assignment-request-cdj-2000nexus");
    let r = parse_number_claim(&request).unwrap();
    assert!(r.assignment_request);
    assert_eq!((r.stage, r.number, r.auto_assign), (2, None, Some(true)));
    let cdj_ip = Ipv4Addr::new(169, 254, 192, 112);
    assert_eq!(r.ip, Some(cdj_ip));
    assert_eq!(
        build_assignment_request("CDJ-2000nexus", cdj_ip, r.mac.unwrap(), true),
        request
    );

    let assignment = fixture("assignment-djm-2000nexus");
    assert_eq!(parse_assignment(&assignment), Ok(3));
    assert_eq!(build_assignment("DJM-2000nexus", 3), assignment);

    let finished = fixture("assignment-finished-cdj-2000nexus");
    assert_eq!(parse_assignment_finished(&finished), Ok(2));
    assert_eq!(build_assignment_finished("CDJ-2000nexus", 2), finished);
}

#[test]
fn real_beats_parse_and_rebuild_byte_exact() {
    let raw = fixture("beat-cdj-2000nexus");
    let b = parse_beat(&raw).unwrap();
    assert_eq!((b.device, b.name.as_str()), (1, "CDJ-2000nexus"));
    assert_eq!(
        (b.bpm_x100, b.pitch, b.beat_within_bar),
        (13201, 0x000f_8312, 1)
    );
    assert_eq!(
        [
            b.next_beat_ms,
            b.second_beat_ms,
            b.next_bar_ms,
            b.fourth_beat_ms,
            b.second_bar_ms,
            b.eighth_beat_ms
        ],
        [0x1c6, 0x38d, 0x71a, 0x71a, 0xe34, 0xe34]
    );
    assert!((b.track_bpm().unwrap() - 132.01).abs() < 1e-9);
    assert!((b.pitch_percent() + 3.05).abs() < 0.01);
    // BPM × pitch / 0x6400000, the documented formula ("Pitch").
    let expected = f64::from(0x3391_u32) * f64::from(0x000f_8312_u32) / f64::from(0x0640_0000_u32);
    assert!((b.effective_bpm().unwrap() - expected).abs() < 1e-9);
    assert!((b.effective_bpm().unwrap() - 127.98).abs() < 0.005);
    assert_eq!(build_beat_packet(&b), raw);

    let raw = fixture("beat-djm-2000nexus");
    let m = parse_beat(&raw).unwrap();
    assert_eq!(
        (m.device, m.beat_within_bar, m.next_beat_ms),
        (0x21, 3, 500)
    );
    assert_eq!(m.effective_bpm(), Some(120.0));
    assert_eq!(build_beat_packet(&m), raw);
}

#[test]
fn real_on_air_parses_and_rebuilds() {
    let raw = fixture("on-air-djm-2000nexus");
    let o = parse_on_air(&raw).unwrap();
    assert_eq!(o.device, 0x21);
    assert_eq!(
        o.channels,
        [Some(false), Some(true), Some(true), Some(true), None, None]
    );
    assert_eq!(build_on_air(&o), raw);
}

#[test]
fn real_cdj_status() {
    let s = parse_cdj_status(&fixture("status-cdj-2000nexus-playing-master")).unwrap();
    assert_eq!(
        (s.device, s.packet_len, s.firmware.as_str()),
        (1, 0x11c, "1.44")
    );
    assert!(s.playing() && s.master() && !s.synced() && !s.on_air());
    assert_eq!((s.play_state, s.flags), (3, 0xe4));
    assert_eq!(s.track_bpm(), Some(132.01));
    assert!((s.effective_bpm().unwrap() - 127.98).abs() < 0.005);
    assert_eq!((s.beat_number(), s.beat_within_bar), (Some(2), 2));
    assert_eq!(s.master_handoff_to(), None);

    let idle = parse_cdj_status(&fixture("status-cdj-2000nexus-idle")).unwrap();
    assert_eq!(
        (idle.device, idle.packet_len, idle.firmware.as_str()),
        (3, 0xd4, "1.24")
    );
    assert!(!idle.playing() && !idle.master());
    assert_eq!(
        (idle.track_bpm(), idle.beat_number(), idle.beat_within_bar),
        (None, None, 0)
    );
}

#[test]
fn real_mixer_status_parses_and_rebuilds() {
    let raw = fixture("status-djm-2000nexus");
    let m = parse_mixer_status(&raw).unwrap();
    assert_eq!((m.device, m.flags, m.beat_within_bar), (0x21, 0xd0, 3));
    assert!(!m.master());
    assert_eq!(m.effective_bpm(), Some(120.0));
    assert_eq!(build_mixer_status(&m), raw);
}

#[test]
fn classification_depends_on_the_port() {
    let status = fixture("status-cdj-2000nexus-idle");
    let hello = fixture("hello-cdj-2000nexus");
    // Both carry kind 0x0a.
    assert_eq!(
        PacketKind::classify(Port::Status, &status),
        Some(PacketKind::CdjStatus)
    );
    assert_eq!(
        PacketKind::classify(Port::Announce, &hello),
        Some(PacketKind::Hello)
    );
    assert_eq!(
        PacketKind::classify(Port::Beat, &hello),
        Some(PacketKind::Unknown(0x0a))
    );
    assert_eq!(PacketKind::classify(Port::Beat, b"not dj link"), None);
    assert_eq!(PacketKind::classify(Port::Beat, &[]), None);
    assert_eq!(
        parse_beat(&status),
        Err(ParseError::WrongKind {
            expected: 0x28,
            found: 0x0a
        })
    );
    assert_eq!(
        parse_beat(&fixture("beat-cdj-2000nexus")[..0x50]),
        Err(ParseError::TooShort {
            needed: 0x60,
            got: 0x50
        })
    );
    assert_eq!(
        parse_keep_alive(b"Qspt1WmJOL"),
        Err(ParseError::NotProDjLink)
    );
}

#[test]
fn pitch_encodings() {
    assert_eq!(pitch_to_multiplier(0x0010_0000), 1.0);
    assert_eq!(pitch_to_multiplier(0), 0.0);
    assert_eq!(pitch_to_multiplier(0x0020_0000), 2.0);
    assert_eq!(percent_to_pitch(0.0), 0x0010_0000);
    assert_eq!(percent_to_pitch(-100.0), 0);
    assert_eq!(percent_to_pitch(500.0), 0x0020_0000);
    assert!((pitch_to_percent(percent_to_pitch(3.26)) - 3.26).abs() < 1e-4);
    let pp = PrecisePosition {
        device: 2,
        name: "CDJ-3000".into(),
        track_length_s: 300,
        playhead_ms: 0,
        pitch_x100: 326,
        bpm_x10: 1202,
    };
    assert_eq!(pp.pitch_percent(), 3.26);
    assert_eq!(pp.effective_bpm(), Some(120.2));
    let unknown = PrecisePosition {
        bpm_x10: 0xffff_ffff,
        pitch_x100: -1050,
        ..pp
    };
    assert_eq!(unknown.effective_bpm(), None);
    assert_eq!(unknown.pitch_percent(), -10.5);
    assert_eq!(
        parse_precise_position(&build_precise_position(&unknown)),
        Ok(unknown)
    );
}

// ---------------------------------------------------------------------------
// Constructed fixtures: golden bytes for what we send and for packet types
// no published capture contains.

const P5_IP: Ipv4Addr = Ipv4Addr::new(169, 254, 0, 5);

fn constructed() -> Vec<(&'static str, Vec<u8>, &'static str, &'static [&'static str])> {
    let mac = fallback_mac(P5_IP);
    let cdj3000 = CdjStatus::new(2, 128.0, -1.5, true, true);
    vec![
        (
            "player5-hello",
            build_hello("player5"),
            "https://djl-analysis.deepsymmetry.org/djl-analysis/startup.html#cdj-3000-initial-announcement",
            &["Our CDJ-3000-compatible hello: structure byte 04, length 0x26, payload 01 40."],
        ),
        (
            "player5-claim-stage1",
            build_claim_stage1("player5", mac, 1),
            "https://djl-analysis.deepsymmetry.org/djl-analysis/startup.html#startup-3000",
            &["Our CDJ-3000-compatible first-stage claim, N = 1, fallback MAC 02:70:a9:fe:00:05."],
        ),
        (
            "player5-claim-stage2",
            build_claim_stage2("player5", P5_IP, mac, 5, 1, false),
            "https://djl-analysis.deepsymmetry.org/djl-analysis/startup.html#startup-3000",
            &["Our second-stage claim for device 5 from 169.254.0.5, N = 1, a = 02 (specific number)."],
        ),
        (
            "player5-claim-stage3",
            build_claim_stage3("player5", 5, 1),
            "https://djl-analysis.deepsymmetry.org/djl-analysis/startup.html#startup-3000",
            &["Our final-stage claim for device 5, N = 1."],
        ),
        (
            "player5-keep-alive",
            build_keep_alive(&KeepAlive {
                number: 5,
                name: "player5".into(),
                mac,
                ip: P5_IP,
                peers: 3,
                device_type: 1,
                first_on_network: false,
                cdj3000_compatible: true,
            }),
            "https://djl-analysis.deepsymmetry.org/djl-analysis/startup.html#startup-3000",
            &["Our CDJ-3000-compatible keep-alive: device 5, 3 peers, byte 0x35 = 64."],
        ),
        (
            "constructed-number-in-use",
            build_number_in_use("CDJ-3000", 5, Ipv4Addr::new(169, 254, 0, 50)),
            "https://djl-analysis.deepsymmetry.org/djl-analysis/startup.html#channel-conflict-packet",
            &["A player at 169.254.0.50 defending device number 5."],
        ),
        (
            "constructed-precise-position-cdj-3000",
            build_precise_position(&PrecisePosition {
                device: 2,
                name: "CDJ-3000".into(),
                track_length_s: 312,
                playhead_ms: 61_234,
                pitch_x100: 326,
                bpm_x10: 1202,
            }),
            "https://djl-analysis.deepsymmetry.org/djl-analysis/beats.html#absolute-position-packets",
            &["Device 2: track 312 s, playhead 61.234 s, pitch +3.26 %, 120.2 BPM effective."],
        ),
        (
            "constructed-on-air-6ch",
            build_on_air(&OnAir {
                device: 0x21,
                name: "DJM-V10".into(),
                channels: [Some(true), Some(false), Some(false), Some(false), Some(true), Some(false)],
            }),
            "https://djl-analysis.deepsymmetry.org/djl-analysis/mixer_integration.html#channels-on-air",
            &["Six-channel on-air flags (subtype 03): channels 1 and 5 on air."],
        ),
        (
            "constructed-status-cdj-3000",
            build_cdj_status(&cdj3000),
            "https://djl-analysis.deepsymmetry.org/djl-analysis/vcdj.html#cdj-status-packets",
            &["0x200-byte CDJ-3000-length status: device 2 playing as tempo master,",
              "128.00 BPM track at -1.5 % pitch, beat 1, beat-within-bar 1."],
        ),
    ]
}

#[test]
fn constructed_fixtures_are_golden() {
    let update = std::env::var_os("UPDATE_PROLINK_FIXTURES").is_some();
    for (name, bytes, source, notes) in constructed() {
        let path = fixture_dir().join(format!("{name}.hex"));
        if update {
            let mut text = format!(
                "# Pro DJ Link fixture: {name}\n\
                 # Constructed from the documented layout, not a capture: no published\n\
                 # capture contains this packet. Golden bytes for the builders in\n\
                 # core/sync/src/prolink/packets.rs.\n\
                 # Documented in: {source}\n"
            );
            for n in notes {
                text.push_str(&format!("# {n}\n"));
            }
            text.push_str("# Bytes: UDP payload, hex, 16 per line.\n");
            for chunk in bytes.chunks(16) {
                let line: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
                text.push_str(&line.join(" "));
                text.push('\n');
            }
            std::fs::write(&path, text).unwrap();
        }
        assert_eq!(
            fixture(name),
            bytes,
            "{name} changed; regenerate if intended"
        );
    }
}

#[test]
fn constructed_fixtures_parse() {
    let ka = parse_keep_alive(&fixture("player5-keep-alive")).unwrap();
    assert_eq!((ka.number, ka.name.as_str(), ka.ip), (5, "player5", P5_IP));
    assert!(ka.cdj3000_compatible);
    let hello = fixture("player5-hello");
    assert_eq!(
        (hello.len(), hello[0x21], hello[0x24], hello[0x25]),
        (0x26, 4, 1, 0x40)
    );
    let n = parse_number_in_use(&fixture("constructed-number-in-use")).unwrap();
    assert_eq!((n.number, n.ip), (5, Ipv4Addr::new(169, 254, 0, 50)));
    let pp = parse_precise_position(&fixture("constructed-precise-position-cdj-3000")).unwrap();
    assert_eq!(
        (pp.device, pp.playhead_ms, pp.track_length_s),
        (2, 61_234, 312)
    );
    let o = parse_on_air(&fixture("constructed-on-air-6ch")).unwrap();
    assert_eq!(o.channels[4], Some(true));
    let s = parse_cdj_status(&fixture("constructed-status-cdj-3000")).unwrap();
    assert_eq!(s.packet_len, CDJ_STATUS_CDJ3000_LEN);
    assert!(s.playing() && s.master());
    assert!((s.effective_bpm().unwrap() - 126.08).abs() < 0.01);
}

#[test]
fn every_fixture_has_a_documenting_source() {
    let mut seen = 0;
    for entry in std::fs::read_dir(fixture_dir()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "hex") {
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            assert!(!fixture(&name).is_empty());
            seen += 1;
        }
    }
    assert_eq!(seen, REAL_FIXTURES.len() + constructed().len());
}

// ---------------------------------------------------------------------------
// Fuzzing: nothing may panic, whatever arrives.

struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn exercise(trackers: &mut [Tracker], data: &[u8], at: u64) {
    for port in [Port::Announce, Port::Beat, Port::Status] {
        let _ = PacketKind::classify(port, data);
        let _ = device_name(port, data);
        for t in trackers.iter_mut() {
            t.packet(&Received {
                port,
                data: data.to_vec(),
                from: Ipv4Addr::new(169, 254, 1, (at % 250) as u8 + 1),
                at,
            });
            let _ = t.take_output();
        }
    }
    let _ = parse_keep_alive(data);
    let _ = parse_number_claim(data);
    let _ = parse_number_in_use(data);
    let _ = parse_assignment(data);
    let _ = parse_assignment_finished(data);
    if let Ok(b) = parse_beat(data) {
        let _ = (b.effective_bpm(), b.track_bpm(), b.pitch_percent());
        let _ = build_beat_packet(&b);
    }
    if let Ok(p) = parse_precise_position(data) {
        let _ = (p.effective_bpm(), p.pitch_percent());
    }
    let _ = parse_on_air(data);
    if let Ok(s) = parse_cdj_status(data) {
        let _ = (
            s.playing(),
            s.effective_bpm(),
            s.beat_number(),
            s.master_handoff_to(),
        );
        let _ = build_cdj_status(&s);
    }
    if let Ok(m) = parse_mixer_status(data) {
        let _ = (m.master(), m.effective_bpm());
    }
}

#[test]
fn malformed_input_never_panics() {
    let mut rng = XorShift(0x9e37_79b9_7f4a_7c15);
    let passive = ProlinkConfig {
        passive: true,
        ..ProlinkConfig::default()
    };
    let active = ProlinkConfig {
        interface: Some(Ipv4Addr::new(169, 254, 1, 200)),
        ..ProlinkConfig::default()
    };
    let mut trackers = [
        Tracker::new(&passive, ProlinkPorts::default(), 0),
        Tracker::new(&active, ProlinkPorts::default(), 0),
    ];
    let kinds = [
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x08, 0x0a, 0x0b, 0x26, 0x27, 0x28, 0x29, 0x2a,
    ];
    let mut at = 0u64;
    // Random packets, half of them with a valid header and a known kind.
    for _ in 0..4000 {
        let len = rng.below(0x240);
        let mut data: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
        if rng.next() & 1 == 0 && len > KIND_OFFSET {
            let n = MAGIC.len().min(len);
            data[..n].copy_from_slice(&MAGIC[..n]);
            data[KIND_OFFSET] = kinds[rng.below(kinds.len())];
        }
        at += rng.below(20_000_000) as u64;
        exercise(&mut trackers, &data, at);
        for t in trackers.iter_mut() {
            t.tick(at);
            let _ = t.take_output();
        }
    }
    // Every fixture truncated at every length, and with random corruption.
    let mut fixtures: Vec<Vec<u8>> = REAL_FIXTURES.iter().map(|n| fixture(n)).collect();
    fixtures.extend(constructed().into_iter().map(|(_, b, _, _)| b));
    for f in &fixtures {
        for len in 0..=f.len() {
            exercise(&mut trackers, &f[..len], at);
        }
        for _ in 0..200 {
            let mut g = f.clone();
            for _ in 0..1 + rng.below(8) {
                let i = rng.below(g.len());
                g[i] = rng.next() as u8;
            }
            at += 1_000_000;
            exercise(&mut trackers, &g, at);
        }
    }
}

// ---------------------------------------------------------------------------
// Loopback: the real thread and sockets on 127.0.0.1, ephemeral ports.

fn loopback_sockets() -> (ProlinkSockets, ProlinkPorts) {
    let config = ProlinkConfig {
        listen_address: Ipv4Addr::LOCALHOST,
        ports: ProlinkPorts {
            announce: 0,
            beat: 0,
            status: 0,
        },
        ..ProlinkConfig::default()
    };
    let sockets = ProlinkSockets::bind(&config).unwrap();
    let ports = sockets.local_ports().unwrap();
    (sockets, ports)
}

fn send(socket: &UdpSocket, port: u16, data: &[u8]) -> u64 {
    let before = host_time::now_ns();
    socket.send_to(data, (Ipv4Addr::LOCALHOST, port)).unwrap();
    before
}

/// Collects events until `pred` matches one, or panics after `timeout`.
fn wait_for(
    src: &SourceHandle,
    timeout: Duration,
    mut pred: impl FnMut(&SourceEvent) -> bool,
) -> (SourceEvent, Vec<SourceEvent>) {
    let deadline = Instant::now() + timeout;
    let mut seen = Vec::new();
    while Instant::now() < deadline {
        if let Some(e) = src.recv_timeout(Duration::from_millis(20)) {
            if pred(&e) {
                return (e, seen);
            }
            seen.push(e);
        }
    }
    panic!("event not seen within {timeout:?}; got {seen:#?}");
}

fn observation_of(e: &SourceEvent, device: u8) -> bool {
    matches!(e, SourceEvent::Observation { device: Some(d), phase, .. } if *d == device && *phase != Phase::TempoOnly)
}

#[test]
fn loopback_passive_observations_devices_and_follow_switching() {
    let (sockets, ports) = loopback_sockets();
    let config = ProlinkConfig {
        passive: true,
        ports,
        ..ProlinkConfig::default()
    };
    let src = start_with_sockets(config, sockets).unwrap();
    let net = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let keep_alive = |n: u8| {
        build_keep_alive(&KeepAlive {
            number: n,
            name: "CDJ-3000".into(),
            mac: [0x74, 0x5e, 0x1c, 0, 0, n],
            ip: Ipv4Addr::LOCALHOST,
            peers: 3,
            device_type: 1,
            first_on_network: false,
            cdj3000_compatible: true,
        })
    };
    send(&net, ports.announce, &keep_alive(1));
    send(&net, ports.announce, &keep_alive(2));
    let (devices, _) = wait_for(
        &src,
        Duration::from_secs(2),
        |e| matches!(e, SourceEvent::Devices(d) if d.len() == 2),
    );
    let SourceEvent::Devices(d) = devices else {
        unreachable!()
    };
    assert_eq!((d[0].number, d[1].number), (1, 2));
    assert!(d
        .iter()
        .all(|x| x.kind == DeviceKind::Player && x.address == "127.0.0.1"));

    // Device 2 plays: followed (lowest playing player), bar phase from its
    // beat-within-bar, effective tempo, timestamped on arrival.
    for bb in 1..=4u8 {
        let sent = send(
            &net,
            ports.beat,
            &build_beat_packet(&BeatPacket::new(2, 125.0, 2.4, bb)),
        );
        let (e, _) = wait_for(&src, Duration::from_secs(2), |e| observation_of(e, 2));
        let SourceEvent::Observation {
            host_ns,
            phase,
            bpm,
            precision,
            ..
        } = e
        else {
            unreachable!()
        };
        assert_eq!(phase, Phase::Bar(f64::from(bb - 1)));
        assert!((bpm.unwrap() - 128.0).abs() < 0.01, "{bpm:?}");
        assert_eq!(precision, Precision::Fine);
        assert!(
            host_ns >= sent && host_ns - sent < 500_000_000,
            "{host_ns} vs {sent}"
        );
    }

    // Switch to device 1 explicitly: device 2's beats stop being reported.
    src.command(SourceCommand::Follow(FollowTarget::Device(1)));
    wait_for(
        &src,
        Duration::from_secs(2),
        |e| matches!(e, SourceEvent::Status { message, .. } if message.contains("following device 1")),
    );
    send(
        &net,
        ports.beat,
        &build_beat_packet(&BeatPacket::new(2, 125.0, 0.0, 1)),
    );
    send(
        &net,
        ports.beat,
        &build_beat_packet(&BeatPacket::new(1, 100.0, 0.0, 3)),
    );
    let (e, skipped) = wait_for(&src, Duration::from_secs(2), |e| observation_of(e, 1));
    assert!(matches!(e, SourceEvent::Observation { phase: Phase::Bar(p), .. } if p == 2.0));
    assert!(!skipped.iter().any(|e| observation_of(e, 2)));

    // Back to the master: with both playing and no status, the lowest
    // number (1); once status names device 2 master, device 2.
    src.command(SourceCommand::Follow(FollowTarget::Master));
    send(
        &net,
        ports.beat,
        &build_beat_packet(&BeatPacket::new(1, 100.0, 0.0, 4)),
    );
    wait_for(&src, Duration::from_secs(2), |e| observation_of(e, 1));
    send(
        &net,
        ports.status,
        &build_cdj_status(&CdjStatus::new(2, 125.0, 0.0, true, true)),
    );
    wait_for(
        &src,
        Duration::from_secs(2),
        |e| matches!(e, SourceEvent::Status { message, .. } if message.contains("device 2") && message.contains("tempo master")),
    );
    // Refresh the status so it cannot go stale on a slow machine.
    send(
        &net,
        ports.status,
        &build_cdj_status(&CdjStatus::new(2, 125.0, 0.0, true, true)),
    );
    send(
        &net,
        ports.beat,
        &build_beat_packet(&BeatPacket::new(1, 100.0, 0.0, 1)),
    );
    send(
        &net,
        ports.beat,
        &build_beat_packet(&BeatPacket::new(2, 125.0, 0.0, 2)),
    );
    let (e, skipped) = wait_for(&src, Duration::from_secs(2), |e| observation_of(e, 2));
    assert!(matches!(e, SourceEvent::Observation { phase: Phase::Bar(p), .. } if p == 1.0));
    assert!(!skipped.iter().any(|e| observation_of(e, 1)));

    let stopping = Instant::now();
    src.stop();
    assert!(stopping.elapsed() < Duration::from_secs(1));
}

#[test]
fn loopback_joins_then_yields_to_real_hardware() {
    let (sockets, ports) = loopback_sockets();
    // Stand-in for the rest of the network: our broadcasts go to its port.
    let net = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    net.set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    let config = ProlinkConfig {
        interface: Some(Ipv4Addr::LOCALHOST),
        broadcast: Some(Ipv4Addr::LOCALHOST),
        ports: ProlinkPorts {
            announce: net.local_addr().unwrap().port(),
            ..ports
        },
        ..ProlinkConfig::default()
    };
    let src = start_with_sockets(config, sockets).unwrap();
    let mut kinds = Vec::new();
    let mut buf = [0u8; 512];
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut keep_alive = None;
    while Instant::now() < deadline && keep_alive.is_none() {
        let Ok((n, _)) = net.recv_from(&mut buf) else {
            continue;
        };
        let kind = PacketKind::classify(Port::Announce, &buf[..n]).unwrap();
        kinds.push(kind);
        if kind == PacketKind::ClaimStage3 {
            // Like a settled player would: cut the final stage short.
            send(
                &net,
                ports.announce,
                &build_assignment_finished("CDJ-3000", 1),
            );
        }
        if kind == PacketKind::KeepAlive {
            keep_alive = Some(parse_keep_alive(&buf[..n]).unwrap());
        }
    }
    use PacketKind::*;
    assert_eq!(
        kinds[..10],
        [
            Hello,
            Hello,
            Hello,
            ClaimStage1,
            ClaimStage1,
            ClaimStage1,
            ClaimStage2,
            ClaimStage2,
            ClaimStage2,
            ClaimStage3,
        ]
    );
    // Normally the "assignment finished" cuts the final stage to one packet;
    // a slow machine may get a second one out first.
    let rest = &kinds[10..];
    assert_eq!(rest.last(), Some(&KeepAlive), "{kinds:?}");
    assert!(rest[..rest.len() - 1].iter().all(|k| *k == ClaimStage3) && rest.len() <= 3);
    let k = keep_alive.unwrap();
    assert_eq!(
        (k.number, k.name.as_str(), k.ip),
        (5, "player5", Ipv4Addr::LOCALHOST)
    );
    assert_eq!(k.mac, fallback_mac(Ipv4Addr::LOCALHOST));
    wait_for(
        &src,
        Duration::from_secs(2),
        |e| matches!(e, SourceEvent::Status { message, .. } if message.contains("joined the network as device 5")),
    );

    // A real player turns up on number 5: we yield and stop announcing.
    send(
        &net,
        ports.announce,
        &build_number_in_use("CDJ-3000", 5, Ipv4Addr::new(169, 254, 0, 50)),
    );
    wait_for(
        &src,
        Duration::from_secs(2),
        |e| matches!(e, SourceEvent::Status { warning: true, message } if message.contains("gave it up")),
    );
    while net.recv_from(&mut buf).is_ok() {}
    std::thread::sleep(Duration::from_millis(1600));
    assert!(
        net.recv_from(&mut buf).is_err(),
        "no keep-alives after yielding"
    );
    src.stop();
}

#[test]
fn busy_ports_fail_with_a_helpful_error() {
    let holder = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = holder.local_addr().unwrap().port();
    let config = ProlinkConfig {
        listen_address: Ipv4Addr::LOCALHOST,
        ports: ProlinkPorts {
            announce: port,
            beat: 0,
            status: 0,
        },
        ..ProlinkConfig::default()
    };
    let err = start(config).err().expect("port is taken");
    assert_eq!(err.kind(), std::io::ErrorKind::AddrInUse);
    assert!(err.to_string().contains("rekordbox"), "{err}");
    let bad = ProlinkConfig {
        device_number: 0,
        ..ProlinkConfig::default()
    };
    assert_eq!(
        start(bad).err().unwrap().kind(),
        std::io::ErrorKind::InvalidInput
    );
}
