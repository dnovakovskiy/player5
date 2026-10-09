//! Fixture, fuzz, state-machine and loopback tests for the Opus Quad
//! source. Fixtures live in `docs/protocols/fixtures/opus-quad/`.

use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::packets::*;
use super::session::{default_broadcast, fallback_mac, Action, Session, Settings, Via};
use super::{start_with_sockets, OpusConfig, OpusPorts};
use crate::follower::{Phase, Precision};
use crate::host_time;
use crate::net::{DeviceKind, FollowTarget, SourceCommand, SourceEvent};

/// Parses a fixture: `#` comment lines, then hex bytes.
fn hex(text: &str) -> Vec<u8> {
    text.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .flat_map(str::split_whitespace)
        .map(|h| u8::from_str_radix(h, 16).expect("hex byte"))
        .collect()
}

macro_rules! fixture {
    ($name:literal) => {
        hex(include_str!(concat!(
            "../../../../docs/protocols/fixtures/opus-quad/",
            $name,
            ".hex"
        )))
    };
}

const BEAT_LINK_MAC: [u8; 6] = [0x18, 0x3e, 0xef, 0xda, 0x5b, 0xca];

// ---------------------------------------------------------------- fixtures

#[test]
fn rekordbox_keep_alive_matches_the_published_bytes() {
    let bytes = fixture!("rekordbox-keep-alive");
    assert_eq!(bytes.len(), KEEP_ALIVE_LEN);
    let ours = KeepAlive::rekordbox(0x17, BEAT_LINK_MAC, Ipv4Addr::new(192, 168, 2, 11));
    assert_eq!(ours.to_bytes().to_vec(), bytes);
    let parsed = KeepAlive::parse(&bytes).unwrap();
    assert_eq!(parsed, ours);
    assert_eq!(parsed.name, REKORDBOX_NAME);
    assert_eq!(parsed.number, DEFAULT_DEVICE_NUMBER);
    assert_eq!(parsed.device_type(), 0x04);
    assert_eq!(
        parse_announce(&bytes).unwrap(),
        AnnouncePacket::KeepAlive(ours)
    );
}

#[test]
fn lighting_request_matches_the_published_bytes() {
    let bytes = fixture!("rekordbox-lighting-request");
    assert_eq!(bytes.len(), LIGHTING_REQUEST_LEN);
    assert_eq!(lighting_request(0x17, "macbook pro"), bytes);
    assert_eq!(packet_kind(&bytes), Ok(kind::LIGHTING_REQUEST));
    // Our number goes in both device-number bytes.
    let other = lighting_request(0x13, "macbook pro");
    assert_eq!((other[0x21], other[0x24]), (0x13, 0x13));
    // Non-ASCII is replaced, overlong names are cut with a zero left.
    let odd = lighting_request(0x17, "Café");
    assert_eq!(&odd[0x28..0x30], &[0, b'C', 0, b'a', 0, b'f', 0, b'?']);
    let long = lighting_request(0x17, &"x".repeat(500));
    assert_eq!(long.len(), LIGHTING_REQUEST_LEN);
    assert_eq!(long[LIGHTING_REQUEST_LEN - 1], 0);
    assert_eq!(long[LIGHTING_REQUEST_LEN - 3], b'x');
}

#[test]
fn opus_lighting_hello_prefix_parses() {
    let bytes = fixture!("opus-lighting-hello-prefix");
    let hello = LightingHello::parse(&bytes).unwrap();
    assert_eq!(hello.name, OPUS_NAME);
    assert_eq!(hello.device, 0x09);
    assert_eq!(opus_deck(hello.device), Some(1));
    assert!(hello.status.is_none());
    assert_eq!(parse_update(&bytes).unwrap(), UpdatePacket::Hello(hello));
}

#[test]
fn constructed_opus_keep_alive_round_trips() {
    let bytes = fixture!("constructed-opus-keep-alive");
    let ka = KeepAlive::parse(&bytes).unwrap();
    assert_eq!(ka.name, OPUS_NAME);
    assert_eq!(ka.number, 0x01);
    assert_eq!(ka.mac, [0x02, 0, 0, 0, 0, 0x09]);
    assert_eq!(ka.ip, Ipv4Addr::new(192, 168, 2, 10));
    assert_eq!(ka.peers, 2);
    assert_eq!(ka.device_type(), 0x01);
    assert_eq!(ka.to_bytes().to_vec(), bytes);
}

#[test]
fn constructed_status_deck1_master_parses_and_round_trips() {
    let bytes = fixture!("constructed-opus-status-deck1-master");
    let st = DeckStatus::parse(&bytes).unwrap();
    assert!(st.is_opus());
    assert_eq!(st.device, 0x09);
    assert_eq!(st.deck(), Some(1));
    assert_eq!(st.length, 0xd4);
    assert_eq!(st.track_id, 829);
    assert_eq!(st.flags, 0xec);
    assert!(st.is_playing() && st.is_master() && st.is_on_air() && !st.is_synced());
    assert_eq!(st.track_bpm(), Some(124.0));
    assert_eq!(st.pitch, 0x10_147b);
    let expected = 124.0 * f64::from(0x10_147b_u32) / f64::from(0x10_0000_u32);
    assert!((st.effective_bpm().unwrap() - expected).abs() < 1e-9);
    assert!((st.effective_bpm().unwrap() - 124.62).abs() < 0.01);
    assert_eq!(st.beat_number(), Some(65));
    assert_eq!(st.beat_within_bar(), Some(1));
    assert_eq!(st.master_meaningful, 1);
    assert_eq!(st.to_bytes(), bytes);
    assert_eq!(parse_update(&bytes).unwrap(), UpdatePacket::Status(st));
}

#[test]
fn constructed_status_with_zero_flags_parses_and_round_trips() {
    let bytes = fixture!("constructed-opus-status-zero-flags");
    let st = DeckStatus::parse(&bytes).unwrap();
    assert_eq!(st.deck(), Some(2));
    assert_eq!(st.flags, 0);
    // P2 = 0xfa still says the deck moves.
    assert!(st.is_playing());
    assert!(!st.is_master());
    assert_eq!(st.effective_bpm(), Some(128.0));
    assert_eq!(st.beat_number(), Some(258));
    assert_eq!(st.beat_within_bar(), Some(2));
    assert_eq!(st.to_bytes(), bytes);
}

#[test]
fn field_edge_cases() {
    let mut st = DeckStatus::parse(&fixture!("constructed-opus-status-deck1-master")).unwrap();
    st.track_bpm_x100 = 0xffff;
    assert_eq!(st.effective_bpm(), None);
    st.track_bpm_x100 = 12_000;
    st.pitch = 0;
    assert_eq!(st.effective_bpm(), None, "a stopped platter has no tempo");
    st.beat = u32::MAX;
    assert_eq!(st.beat_number(), None);
    st.bar_beat = 0;
    assert_eq!(st.beat_within_bar(), None);
    st.bar_beat = 5;
    assert_eq!(st.beat_within_bar(), None);
    // Short packets are judged by P1/P2, not the flag byte.
    st.length = STATUS_MIN_LEN;
    st.flags = FLAG_PLAYING;
    st.play_state_1 = 0x05;
    st.play_state_2 = 0x7e;
    assert!(!st.is_playing());
    st.play_state_1 = 0x03;
    assert!(st.is_playing());
    // Device numbers.
    assert_eq!(opus_deck(12), Some(4));
    assert_eq!(opus_deck(3), Some(3));
    assert_eq!(opus_deck(0x21), None);
    assert_eq!(opus_deck(13), None);
    st.name = "CDJ-3000".into();
    assert_eq!(st.deck(), None);
}

#[test]
fn rejects_malformed_packets() {
    assert_eq!(
        packet_kind(&[]),
        Err(ParseError::TooShort { len: 0, need: 11 })
    );
    assert_eq!(packet_kind(&[0u8; 40]), Err(ParseError::BadMagic));
    let ka = fixture!("rekordbox-keep-alive");
    assert!(matches!(
        KeepAlive::parse(&ka[..0x35]),
        Err(ParseError::TooShort { .. })
    ));
    let st = fixture!("constructed-opus-status-deck1-master");
    assert!(matches!(
        DeckStatus::parse(&st[..STATUS_MIN_LEN - 1]),
        Err(ParseError::TooShort { .. })
    ));
    assert_eq!(DeckStatus::parse(&ka), Err(ParseError::WrongKind(0x06)));
    assert_eq!(KeepAlive::parse(&st), Err(ParseError::WrongKind(0x0a)));
    // Longer keep-alives are accepted, like beat-link does.
    let mut long = ka.clone();
    long.extend_from_slice(&[0xaa; 8]);
    assert_eq!(KeepAlive::parse(&long), KeepAlive::parse(&ka));
    // Other kinds are reported, not errors.
    let mut meta = st.clone();
    meta[KIND_OFFSET] = kind::METADATA;
    assert_eq!(parse_update(&meta), Ok(UpdatePacket::Other(0x56)));
    // A long hello carries status fields.
    let mut hello = st.clone();
    hello[KIND_OFFSET] = kind::LIGHTING_HELLO;
    let UpdatePacket::Hello(h) = parse_update(&hello).unwrap() else {
        panic!("not a hello")
    };
    assert_eq!(h.status.unwrap().beat_number(), Some(65));
}

// -------------------------------------------------------------------- fuzz

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn byte(&mut self) -> u8 {
        self.next() as u8
    }
}

fn ascii_name(s: &str) -> bool {
    s.bytes().all(|b| b.is_ascii_graphic() || b == b' ')
}

#[test]
fn fuzz_parsers_and_session_never_panic() {
    let seeds = [
        fixture!("rekordbox-keep-alive"),
        fixture!("rekordbox-lighting-request"),
        fixture!("opus-lighting-hello-prefix"),
        fixture!("constructed-opus-keep-alive"),
        fixture!("constructed-opus-status-deck1-master"),
        fixture!("constructed-opus-status-zero-flags"),
    ];
    let kinds = [0x06, 0x0a, 0x10, 0x11, 0x56, 0x29, 0x00, 0xff];
    let mut rng = Rng(0x5eed_0f0b_u64 << 16 | 0x0b05);
    let mut session = Session::new(settings(Some(Ipv4Addr::new(10, 0, 0, 2))), 0);
    let mut out = Vec::new();
    let mut now = 0u64;
    for i in 0..30_000 {
        let mut p: Vec<u8> = match i % 3 {
            0 => (0..rng.below(600)).map(|_| rng.byte()).collect(),
            _ => {
                let mut p = seeds[rng.below(seeds.len())].clone();
                for _ in 0..rng.below(8) {
                    let at = rng.below(p.len());
                    p[at] = rng.byte();
                }
                if rng.below(4) == 0 {
                    let n = rng.below(p.len() + 1);
                    p.truncate(n);
                }
                if rng.below(4) == 0 {
                    for _ in 0..rng.below(300) {
                        p.push(rng.byte());
                    }
                }
                p
            }
        };
        if p.len() > KIND_OFFSET && rng.below(2) == 0 {
            p[..MAGIC.len()].copy_from_slice(&MAGIC);
            p[KIND_OFFSET] = kinds[rng.below(kinds.len())];
        }
        let _ = packet_kind(&p);
        let _ = parse_announce(&p);
        let _ = parse_update(&p);
        let _ = LightingHello::parse(&p);
        if let Ok(ka) = KeepAlive::parse(&p) {
            if ascii_name(&ka.name) {
                assert_eq!(KeepAlive::parse(&ka.to_bytes()), Ok(ka));
            }
        }
        if let Ok(st) = DeckStatus::parse(&p) {
            let _ = (st.effective_bpm(), st.is_playing(), st.deck());
            if ascii_name(&st.name) {
                assert_eq!(DeckStatus::parse(&st.to_bytes()), Ok(st));
            }
        }
        let from = SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 1 + rng.below(3) as u8), 50002);
        now += rng.below(80_000_000) as u64;
        if rng.below(2) == 0 {
            session.on_update(&p, from, now, &mut out);
        } else {
            session.on_announce(&p, from, now, &mut out);
        }
        if i % 7 == 0 {
            session.tick(now, &mut out);
        }
        if i % 1000 == 0 {
            let target = match rng.below(3) {
                0 => FollowTarget::Master,
                _ => FollowTarget::Device(rng.byte() % 6),
            };
            session.set_follow(target, &mut out);
        }
        out.clear();
    }
}

// ----------------------------------------------------------- state machine

const UNIT: Ipv4Addr = Ipv4Addr::new(192, 168, 2, 10);
const US: Ipv4Addr = Ipv4Addr::new(192, 168, 2, 20);
const MS: u64 = 1_000_000;

fn settings(interface: Option<Ipv4Addr>) -> Settings {
    Settings {
        device_number: DEFAULT_DEVICE_NUMBER,
        mac: None,
        interface,
        broadcast: None,
        computer_name: "player5".into(),
        announce_interval_ns: 1500 * MS,
        peer_announce_port: ANNOUNCE_PORT,
        peer_update_port: UPDATE_PORT,
        follow: FollowTarget::Master,
    }
}

fn unit_keep_alive() -> Vec<u8> {
    fixture!("constructed-opus-keep-alive")
}

fn status(deck: u8, playing: bool, master: bool, bpm_x100: u16, beat: u32) -> Vec<u8> {
    let mut flags = 0x84;
    if playing {
        flags |= FLAG_PLAYING;
    }
    if master {
        flags |= FLAG_MASTER;
    }
    DeckStatus {
        name: OPUS_NAME.into(),
        device: deck + 8,
        length: 0xd4,
        track_id: 42,
        play_state_1: if playing { 0x03 } else { 0x05 },
        flags,
        play_state_2: if playing { 0xfa } else { 0xfe },
        pitch: PITCH_NEUTRAL,
        track_bpm_x100: bpm_x100,
        master_meaningful: u8::from(master),
        beat,
        bar_beat: ((beat.max(1) - 1) % 4 + 1) as u8,
    }
    .to_bytes()
}

fn events(out: &[Action]) -> Vec<&SourceEvent> {
    out.iter()
        .filter_map(|a| match a {
            Action::Event(e) => Some(e),
            Action::Send { .. } => None,
        })
        .collect()
}

fn observations(out: &[Action]) -> Vec<(u64, Phase, Option<f64>, Option<u8>)> {
    events(out)
        .into_iter()
        .filter_map(|e| match e {
            SourceEvent::Observation {
                host_ns,
                phase,
                bpm,
                precision,
                device,
            } => {
                assert_eq!(*precision, Precision::Coarse);
                Some((*host_ns, *phase, *bpm, *device))
            }
            _ => None,
        })
        .collect()
}

fn from_unit(port: u16) -> SocketAddrV4 {
    SocketAddrV4::new(UNIT, port)
}

#[test]
fn discovers_the_unit_and_announces() {
    let mut s = Session::new(settings(Some(US)), 0);
    let mut out = Vec::new();
    s.tick(0, &mut out);
    // Announces right away, but has no unit to ask yet.
    let sends: Vec<_> = out
        .iter()
        .filter_map(|a| match a {
            Action::Send { via, to, bytes } => Some((*via, *to, bytes.clone())),
            Action::Event(_) => None,
        })
        .collect();
    assert_eq!(sends.len(), 1);
    assert_eq!(sends[0].0, Via::Announce);
    assert_eq!(
        sends[0].1,
        SocketAddrV4::new(Ipv4Addr::new(192, 168, 2, 255), ANNOUNCE_PORT)
    );
    let ka = KeepAlive::parse(&sends[0].2).unwrap();
    assert_eq!(ka, KeepAlive::rekordbox(0x17, fallback_mac(US), US));
    out.clear();

    s.on_announce(
        &unit_keep_alive(),
        from_unit(ANNOUNCE_PORT),
        10 * MS,
        &mut out,
    );
    assert_eq!(s.unit(), Some(UNIT));
    let ev = events(&out);
    assert!(ev.iter().any(
        |e| matches!(e, SourceEvent::Status { message, .. } if message.contains("Opus Quad found"))
    ));
    let devices = ev
        .iter()
        .find_map(|e| match e {
            SourceEvent::Devices(d) => Some(d.clone()),
            _ => None,
        })
        .expect("device table");
    assert_eq!(devices.len(), 4);
    assert!(devices
        .iter()
        .enumerate()
        .all(|(i, d)| d.number == i as u8 + 1
            && d.kind == DeviceKind::AllInOne
            && d.name == OPUS_NAME
            && d.address == UNIT.to_string()
            && d.bpm.is_none()));
    out.clear();

    // Discovery triggers an immediate keep-alive + lighting request.
    s.tick(20 * MS, &mut out);
    assert!(out.iter().any(|a| matches!(a,
        Action::Send { via: Via::Update, to, bytes }
            if *to == from_unit(UPDATE_PORT) && *bytes == lighting_request(0x17, "player5"))));
    out.clear();
    // Then on the configured cadence.
    s.tick(1000 * MS, &mut out);
    assert!(out.iter().all(|a| !matches!(a, Action::Send { .. })));
    s.tick(1521 * MS, &mut out);
    assert_eq!(
        out.iter()
            .filter(|a| matches!(a, Action::Send { .. }))
            .count(),
        2
    );
}

#[test]
fn finds_the_interface_once_the_unit_is_seen() {
    let mut s = Session::new(settings(None), 0);
    let mut out = Vec::new();
    s.tick(0, &mut out);
    assert!(out.iter().all(|a| !matches!(a, Action::Send { .. })));
    assert_eq!(s.interface_wanted(), None);
    // The unit's hello on the update port is enough to find it.
    s.on_update(
        &fixture!("opus-lighting-hello-prefix"),
        from_unit(UPDATE_PORT),
        5 * MS,
        &mut out,
    );
    assert_eq!(s.interface_wanted(), Some(UNIT));
    s.set_interface(US, &mut out);
    assert_eq!(s.interface_wanted(), None);
    out.clear();
    s.tick(10 * MS, &mut out);
    assert_eq!(
        out.iter()
            .filter(|a| matches!(a, Action::Send { .. }))
            .count(),
        2
    );
    // Failing to resolve warns once.
    let mut t = Session::new(settings(None), 0);
    let mut warn = Vec::new();
    t.interface_failed(&mut warn);
    t.interface_failed(&mut warn);
    assert_eq!(events(&warn).len(), 1);
}

#[test]
fn ignores_its_own_echo_and_defends_its_number() {
    let mut s = Session::new(settings(Some(US)), 0);
    let mut out = Vec::new();
    let own = KeepAlive::rekordbox(0x17, fallback_mac(US), US).to_bytes();
    s.on_announce(&own, SocketAddrV4::new(US, ANNOUNCE_PORT), MS, &mut out);
    assert!(s.devices().is_empty());
    // Real rekordbox with the same number elsewhere.
    let other = KeepAlive::rekordbox(0x17, [2, 0, 0, 0, 0, 7], Ipv4Addr::new(192, 168, 2, 30));
    s.on_announce(
        &other.to_bytes(),
        SocketAddrV4::new(other.ip, ANNOUNCE_PORT),
        2 * MS,
        &mut out,
    );
    assert_eq!(s.device_number(), 0x13);
    let devices = s.devices();
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].kind, DeviceKind::Rekordbox);
    // Another player5 with a higher MAC on our new number yields to us.
    let higher = KeepAlive::rekordbox(0x13, [0x0a, 0, 0, 0, 0, 1], Ipv4Addr::new(192, 168, 2, 31));
    s.on_announce(
        &higher.to_bytes(),
        SocketAddrV4::new(higher.ip, ANNOUNCE_PORT),
        2 * MS,
        &mut out,
    );
    assert_eq!(s.device_number(), 0x13);
    // A player on it does not.
    let mut cdj = higher.clone();
    cdj.name = "CDJ-3000".into();
    cdj.mac = [0x0a, 0, 0, 0, 0, 2];
    s.on_announce(
        &cdj.to_bytes(),
        SocketAddrV4::new(cdj.ip, ANNOUNCE_PORT),
        2 * MS,
        &mut out,
    );
    assert_eq!(s.device_number(), 0x14);
    out.clear();
    s.tick(3 * MS, &mut out);
    let sent = out.iter().find_map(|a| match a {
        Action::Send { bytes, .. } => Some(bytes.clone()),
        Action::Event(_) => None,
    });
    assert_eq!(KeepAlive::parse(&sent.unwrap()).unwrap().number, 0x14);
}

#[test]
fn follows_the_master_deck_with_coarse_bar_phase() {
    let mut s = Session::new(settings(Some(US)), 0);
    let mut out = Vec::new();
    s.on_announce(&unit_keep_alive(), from_unit(ANNOUNCE_PORT), 0, &mut out);
    out.clear();
    // Deck 1 master at 120 BPM: beat n starts at (n - 1) * 500 ms + 30 ms.
    // Deck 2 plays too, not master.
    let mut t = 0;
    while t < 4000 * MS {
        let beat1 = ((t.saturating_sub(30 * MS)) / (500 * MS)) as u32 + 1;
        s.on_update(
            &status(1, true, true, 12_000, beat1),
            from_unit(UPDATE_PORT),
            t,
            &mut out,
        );
        s.on_update(
            &status(2, true, false, 12_800, 7),
            from_unit(UPDATE_PORT),
            t + MS,
            &mut out,
        );
        t += 200 * MS;
    }
    assert_eq!(s.followed(), Some(1));
    let obs = observations(&out);
    assert!(obs.len() >= 5, "{obs:?}");
    for (host_ns, phase, bpm, device) in obs {
        assert_eq!(device, Some(1));
        assert_eq!(bpm, Some(120.0));
        // Nearest true beat start and its bar position.
        let k = ((host_ns as f64 - 30.0e6) / 500.0e6).round() as u64;
        let truth = 30 * MS + k * 500 * MS;
        assert!(host_ns.abs_diff(truth) <= 104 * MS, "{host_ns} vs {truth}");
        assert_eq!(phase, Phase::Bar((k % 4) as f64));
    }
    let devices = s.devices();
    assert_eq!(devices[0].master, Some(true));
    assert_eq!(devices[0].bpm, Some(120.0));
    assert_eq!(devices[1].playing, Some(true));
    assert_eq!(devices[2].playing, None);
}

#[test]
fn switches_decks_on_command_and_falls_back_without_master() {
    let mut s = Session::new(settings(Some(US)), 0);
    let mut out = Vec::new();
    s.on_announce(&unit_keep_alive(), from_unit(ANNOUNCE_PORT), 0, &mut out);
    // Nobody is master; only deck 3 plays.
    s.on_update(
        &status(3, true, false, 12_500, 10),
        from_unit(UPDATE_PORT),
        MS,
        &mut out,
    );
    s.on_update(
        &status(4, false, false, 12_500, 10),
        from_unit(UPDATE_PORT),
        MS,
        &mut out,
    );
    assert_eq!(s.followed(), Some(3));
    // A second deck starts: stay on deck 3.
    s.on_update(
        &status(4, true, false, 12_500, 11),
        from_unit(UPDATE_PORT),
        2 * MS,
        &mut out,
    );
    assert_eq!(s.followed(), Some(3));
    // Deck 4 becomes master: follow it.
    s.on_update(
        &status(4, true, true, 12_500, 11),
        from_unit(UPDATE_PORT),
        3 * MS,
        &mut out,
    );
    assert_eq!(s.followed(), Some(4));
    // Explicit deck.
    s.set_follow(FollowTarget::Device(2), &mut out);
    assert_eq!(s.followed(), Some(2));
    out.clear();
    s.on_update(
        &status(2, true, false, 13_000, 20),
        from_unit(UPDATE_PORT),
        200 * MS,
        &mut out,
    );
    s.on_update(
        &status(4, true, true, 12_500, 12),
        from_unit(UPDATE_PORT),
        201 * MS,
        &mut out,
    );
    s.on_update(
        &status(2, true, false, 13_000, 21),
        from_unit(UPDATE_PORT),
        400 * MS,
        &mut out,
    );
    let obs = observations(&out);
    assert_eq!(obs.len(), 1);
    assert_eq!(obs[0].3, Some(2));
    assert_eq!(obs[0].2, Some(130.0));
    // Out-of-range deck: nothing followed, and the user is told.
    out.clear();
    s.set_follow(FollowTarget::Device(7), &mut out);
    assert_eq!(s.followed(), None);
    assert!(events(&out)
        .iter()
        .any(|e| matches!(e, SourceEvent::Status { message, .. } if message.contains("deck 7"))));
}

#[test]
fn master_policy_prefers_what_is_audible() {
    let mut s = Session::new(settings(Some(US)), 0);
    let mut out = Vec::new();
    let mut feed = |s: &mut Session, deck, playing, master| {
        s.on_update(
            &status(deck, playing, master, 12_400, 9),
            from_unit(UPDATE_PORT),
            MS,
            &mut out,
        );
    };
    s.on_announce(
        &unit_keep_alive(),
        from_unit(ANNOUNCE_PORT),
        0,
        &mut Vec::new(),
    );
    // A stopped master and one playing deck: follow the playing one.
    feed(&mut s, 1, false, true);
    feed(&mut s, 2, true, false);
    assert_eq!(s.followed(), Some(2));
    // Nothing plays: the stopped master.
    feed(&mut s, 2, false, false);
    assert_eq!(s.followed(), Some(1));
    // Two playing masters during a hand-off: stay on the current one.
    feed(&mut s, 3, true, true);
    assert_eq!(s.followed(), Some(3));
    feed(&mut s, 1, true, true);
    assert_eq!(s.followed(), Some(3));
    feed(&mut s, 3, true, false);
    assert_eq!(s.followed(), Some(1));
}

#[test]
fn recovers_zero_status_flags_like_beat_link() {
    let mut s = Session::new(settings(Some(US)), 0);
    let mut out = Vec::new();
    s.on_announce(&unit_keep_alive(), from_unit(ANNOUNCE_PORT), 0, &mut out);
    let zero = fixture!("constructed-opus-status-zero-flags");
    // No earlier valid flags for deck 2: dropped.
    s.on_update(&zero, from_unit(UPDATE_PORT), MS, &mut out);
    assert_eq!(s.devices()[1].playing, None);
    // A valid packet (master), then a zero-flag one keeps "master".
    s.on_update(
        &status(2, true, true, 12_800, 257),
        from_unit(UPDATE_PORT),
        2 * MS,
        &mut out,
    );
    s.on_update(&zero, from_unit(UPDATE_PORT), 3 * MS, &mut out);
    let d = &s.devices()[1];
    assert_eq!(d.master, Some(true));
    assert_eq!(d.bpm, Some(128.0));
}

#[test]
fn ignores_foreign_status_and_second_units() {
    let mut s = Session::new(settings(Some(US)), 0);
    let mut out = Vec::new();
    s.on_announce(&unit_keep_alive(), from_unit(ANNOUNCE_PORT), 0, &mut out);
    let mut cdj = DeckStatus::parse(&status(1, true, true, 12_000, 5)).unwrap();
    cdj.name = "CDJ-3000".into();
    cdj.device = 1;
    s.on_update(&cdj.to_bytes(), from_unit(UPDATE_PORT), MS, &mut out);
    assert_eq!(s.devices()[0].playing, None);
    let elsewhere = SocketAddrV4::new(Ipv4Addr::new(192, 168, 2, 99), UPDATE_PORT);
    s.on_update(&status(1, true, true, 12_000, 5), elsewhere, MS, &mut out);
    assert_eq!(s.devices()[0].playing, None);
    assert_eq!(s.unit(), Some(UNIT));
}

#[test]
fn expires_the_unit_and_peers() {
    let mut s = Session::new(settings(Some(US)), 0);
    let mut out = Vec::new();
    s.on_announce(&unit_keep_alive(), from_unit(ANNOUNCE_PORT), 0, &mut out);
    let cdj = KeepAlive {
        name: "CDJ-2000NXS2".into(),
        subtype: 0x02,
        number: 3,
        byte_25: 0x01,
        mac: [2, 0, 0, 0, 0, 3],
        ip: Ipv4Addr::new(192, 168, 2, 3),
        peers: 2,
        tail: [0, 0, 0, 1, 0],
    };
    s.on_announce(
        &cdj.to_bytes(),
        SocketAddrV4::new(cdj.ip, ANNOUNCE_PORT),
        0,
        &mut out,
    );
    assert_eq!(s.devices().len(), 5);
    assert_eq!(s.devices()[3].kind, DeviceKind::Player);
    out.clear();
    s.tick(11_000 * MS, &mut out);
    assert_eq!(s.unit(), None);
    assert!(s.devices().is_empty());
    let ev = events(&out);
    assert!(ev.iter().any(
        |e| matches!(e, SourceEvent::Status { warning: true, message } if message.contains("lost"))
    ));
    assert!(ev
        .iter()
        .any(|e| matches!(e, SourceEvent::Devices(d) if d.is_empty())));
}

#[test]
fn warns_when_no_unit_shows_up() {
    let mut s = Session::new(settings(None), 0);
    let mut out = Vec::new();
    s.tick(1000 * MS, &mut out);
    assert!(out.is_empty());
    s.tick(6000 * MS, &mut out);
    s.tick(7000 * MS, &mut out);
    assert_eq!(events(&out).len(), 1);
}

#[test]
fn policies() {
    assert_eq!(
        fallback_mac(Ipv4Addr::new(169, 254, 1, 2)),
        [0x02, 0x50, 169, 254, 1, 2]
    );
    // Locally administered, unicast.
    assert_eq!(fallback_mac(US)[0] & 0x03, 0x02);
    assert_eq!(
        default_broadcast(Ipv4Addr::new(169, 254, 7, 9)),
        Ipv4Addr::new(169, 254, 255, 255)
    );
    assert_eq!(default_broadcast(US), Ipv4Addr::new(192, 168, 2, 255));
    assert_eq!(default_broadcast(Ipv4Addr::LOCALHOST), Ipv4Addr::LOCALHOST);
    let s = OpusConfig {
        announce_interval: Duration::from_secs(60),
        ..OpusConfig::default()
    }
    .settings();
    assert_eq!(s.announce_interval_ns, 2_000_000_000);
}

// ---------------------------------------------------------------- loopback

/// A pretend Opus Quad on loopback: announces itself to `our_announce`,
/// waits for a lighting request, then streams status for two decks.
struct FakeUnit {
    announce: UdpSocket,
    update: UdpSocket,
    our_announce: SocketAddrV4,
    origin: u64,
}

/// What the fake unit saw from player5.
#[derive(Default)]
struct Seen {
    keep_alive: Option<KeepAlive>,
    request: Option<Vec<u8>>,
}

/// Deck 1: master, 150 BPM. Deck 2: 120 BPM. Beat n of a deck starts at
/// `origin + offset + (n - 1) * period`.
const DECKS: [(u8, u16, u64, u64); 2] = [
    (1, 15_000, 400 * MS, 37 * MS),
    (2, 12_000, 500 * MS, 211 * MS),
];

impl FakeUnit {
    fn run(self, stop: &AtomicBool) -> Seen {
        let mut seen = Seen::default();
        let mut subscriber = None;
        let ka = KeepAlive {
            name: OPUS_NAME.into(),
            subtype: 0x02,
            number: 0x01,
            byte_25: 0x01,
            mac: [0x02, 0, 0, 0, 0, 0x09],
            ip: Ipv4Addr::LOCALHOST,
            peers: 2,
            tail: [0, 0, 0, 1, 0],
        }
        .to_bytes();
        self.announce.set_nonblocking(true).unwrap();
        self.update.set_nonblocking(true).unwrap();
        let mut buf = [0u8; 2048];
        let mut next_keep_alive = 0;
        let mut next_status = 0;
        while !stop.load(Ordering::Relaxed) {
            let now = host_time::now_ns();
            if now >= next_keep_alive {
                let _ = self.announce.send_to(&ka, self.our_announce);
                next_keep_alive = now + 100 * MS;
            }
            while let Ok((n, _)) = self.announce.recv_from(&mut buf) {
                if let Ok(k) = KeepAlive::parse(&buf[..n]) {
                    seen.keep_alive = Some(k);
                }
            }
            while let Ok((n, from)) = self.update.recv_from(&mut buf) {
                if packet_kind(&buf[..n]) == Ok(kind::LIGHTING_REQUEST) {
                    seen.request = Some(buf[..n].to_vec());
                    subscriber = Some(from);
                }
            }
            if let Some(to) = subscriber {
                if now >= next_status {
                    next_status = now + 50 * MS;
                    for (deck, bpm_x100, period, offset) in DECKS {
                        let start = self.origin + offset;
                        let beat = (now.saturating_sub(start) / period) as u32 + 1;
                        let mut st =
                            DeckStatus::parse(&status(deck, true, deck == 1, bpm_x100, beat))
                                .unwrap();
                        st.bar_beat = ((beat - 1) % 4 + 1) as u8;
                        let _ = self.update.send_to(&st.to_bytes(), to);
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        seen
    }
}

/// Checks an observation against the fake unit's timeline for `deck`.
fn check_observation(origin: u64, deck: u8, host_ns: u64, phase: Phase, bpm: Option<f64>) {
    let (_, bpm_x100, period, offset) = DECKS[usize::from(deck - 1)];
    assert!((bpm.unwrap() - f64::from(bpm_x100) / 100.0).abs() < 1e-9);
    let start = origin + offset;
    let k = ((host_ns as f64 - start as f64) / period as f64).round() as u64;
    let truth = start + k * period;
    let err = host_ns.abs_diff(truth);
    // Packets every 50 ms bound the error to ~25 ms plus scheduling noise.
    assert!(err < 80 * MS, "deck {deck}: off by {} ms", err / MS);
    assert_eq!(phase, Phase::Bar((k % 4) as f64), "deck {deck}");
}

#[test]
fn loopback_unit_is_found_followed_and_switched() {
    let lo = Ipv4Addr::LOCALHOST;
    let unit_announce = UdpSocket::bind((lo, 0)).unwrap();
    let unit_update = UdpSocket::bind((lo, 0)).unwrap();
    let our_announce = UdpSocket::bind((lo, 0)).unwrap();
    let our_update = UdpSocket::bind((lo, 0)).unwrap();
    let port = |s: &UdpSocket| s.local_addr().unwrap().port();
    let config = OpusConfig {
        broadcast: Some(lo),
        announce_interval: Duration::from_millis(200),
        ports: OpusPorts {
            bind: lo,
            announce: 0,
            update: 0,
            peer_announce: port(&unit_announce),
            peer_update: port(&unit_update),
        },
        ..OpusConfig::default()
    };
    let origin = host_time::now_ns();
    let fake = FakeUnit {
        our_announce: SocketAddrV4::new(lo, port(&our_announce)),
        announce: unit_announce,
        update: unit_update,
        origin,
    };
    let source = start_with_sockets(config, our_announce, our_update).unwrap();
    assert_eq!(source.name(), "opus");
    let stop = Arc::new(AtomicBool::new(false));
    let stop_fake = Arc::clone(&stop);
    let unit = std::thread::spawn(move || fake.run(&stop_fake));

    let deadline = Instant::now() + Duration::from_secs(20);
    let mut table_ok = false;
    let mut deck1 = 0;
    let mut deck2 = 0;
    let mut switched = false;
    while Instant::now() < deadline && !(table_ok && deck2 >= 3) {
        let Some(event) = source.recv_timeout(Duration::from_millis(100)) else {
            continue;
        };
        match event {
            SourceEvent::Devices(list) => {
                let d1 = list.iter().find(|d| d.number == 1);
                table_ok |= list.len() == 4
                    && list.iter().all(|d| d.kind == DeviceKind::AllInOne)
                    && d1.is_some_and(|d| {
                        d.master == Some(true) && d.playing == Some(true) && d.bpm == Some(150.0)
                    });
            }
            SourceEvent::Observation {
                host_ns,
                phase,
                bpm,
                precision,
                device,
            } => {
                assert_eq!(precision, Precision::Coarse);
                match device {
                    Some(1) if !switched => {
                        check_observation(origin, 1, host_ns, phase, bpm);
                        deck1 += 1;
                        if deck1 >= 4 {
                            source.command(SourceCommand::Follow(FollowTarget::Device(2)));
                            switched = true;
                        }
                    }
                    // A deck-1 beat may already be on its way.
                    Some(1) => {}
                    Some(2) => {
                        assert!(switched, "followed deck 2 before being told to");
                        check_observation(origin, 2, host_ns, phase, bpm);
                        deck2 += 1;
                    }
                    other => panic!("observation from {other:?}"),
                }
            }
            SourceEvent::Status { .. } => {}
        }
    }
    stop.store(true, Ordering::Relaxed);
    let seen = unit.join().unwrap();
    source.stop();
    assert!(table_ok, "no complete device table");
    assert!(deck1 >= 4 && deck2 >= 3, "deck1 {deck1}, deck2 {deck2}");
    // What player5 told the unit.
    let ka = seen.keep_alive.expect("keep-alive");
    assert_eq!(ka, KeepAlive::rekordbox(0x17, fallback_mac(lo), lo));
    assert_eq!(seen.request.unwrap(), lighting_request(0x17, "player5"));
}
