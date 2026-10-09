//! The Opus Quad source as a pure state machine: packets and clock ticks
//! in, packets to send and [`SourceEvent`]s out. No sockets, no clock
//! reads, so every behaviour is unit-testable; `source.rs` wires it to UDP.

use std::net::{Ipv4Addr, SocketAddrV4};

use super::packets::{
    lighting_request, parse_announce, parse_update, AnnouncePacket, DeckStatus, KeepAlive,
    UpdatePacket, FALLBACK_DEVICE_NUMBERS, OPUS_NAME, REKORDBOX_NAME,
};
use super::tracker::BeatTracker;
use crate::follower::{Phase, Precision};
use crate::net::{DeviceInfo, DeviceKind, FollowTarget, SourceEvent};

/// Devices not heard from for this long are dropped (beat-link's
/// `DeviceFinder.MAXIMUM_AGE`; opus-quad.md, "player5 policies").
pub const EXPIRY_NS: u64 = 10_000_000_000;

/// The device table is republished at most this often unless its
/// membership changes.
pub const DEVICES_MIN_INTERVAL_NS: u64 = 250_000_000;

/// After this long without seeing the unit, say so once.
pub const WAITING_WARNING_NS: u64 = 5_000_000_000;

/// Settings the session runs with (resolved from `OpusConfig`).
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// Device number to announce.
    pub device_number: u8,
    /// MAC to announce; derived from the interface address if `None`.
    pub mac: Option<[u8; 6]>,
    /// Our address on the booth network; discovered if `None`.
    pub interface: Option<Ipv4Addr>,
    /// Keep-alive destination address; derived if `None`.
    pub broadcast: Option<Ipv4Addr>,
    /// Computer name in the lighting request.
    pub computer_name: String,
    /// Keep-alive and lighting-request cadence.
    pub announce_interval_ns: u64,
    /// Port keep-alives are sent to.
    pub peer_announce_port: u16,
    /// Port on the unit the lighting request is sent to.
    pub peer_update_port: u16,
    /// Initial follow target.
    pub follow: FollowTarget,
}

/// Which local socket a packet leaves from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Via {
    /// The announce socket (port 50000).
    Announce,
    /// The update socket (port 50002).
    Update,
}

/// Something the shell must do.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Send a datagram.
    Send {
        /// Socket to send from.
        via: Via,
        /// Destination.
        to: SocketAddrV4,
        /// Payload.
        bytes: Vec<u8>,
    },
    /// Report an event to the owner.
    Event(SourceEvent),
}

/// A deterministic, locally administered MAC for an interface address:
/// `02:50:a:b:c:d` (opus-quad.md, "player5 policies").
#[must_use]
pub fn fallback_mac(ip: Ipv4Addr) -> [u8; 6] {
    let o = ip.octets();
    [0x02, 0x50, o[0], o[1], o[2], o[3]]
}

/// Where keep-alives go when no broadcast address is configured:
/// `169.254.255.255` on link-local networks, the address itself on
/// loopback, otherwise the /24 broadcast address (opus-quad.md, "player5
/// policies").
#[must_use]
pub fn default_broadcast(ip: Ipv4Addr) -> Ipv4Addr {
    let o = ip.octets();
    if ip.is_loopback() {
        ip
    } else if ip.is_link_local() {
        Ipv4Addr::new(169, 254, 255, 255)
    } else {
        Ipv4Addr::new(o[0], o[1], o[2], 255)
    }
}

/// Classifies a non-Opus keep-alive (opus-quad.md, "What the unit
/// sends").
fn classify(ka: &KeepAlive) -> DeviceKind {
    if ka.name == REKORDBOX_NAME {
        DeviceKind::Rekordbox
    } else if ka.name == OPUS_NAME {
        DeviceKind::AllInOne
    } else {
        match ka.device_type() {
            0x01 => DeviceKind::Player,
            0x02 => DeviceKind::Mixer,
            _ => DeviceKind::Other,
        }
    }
}

#[derive(Clone, Debug)]
struct Unit {
    addr: Ipv4Addr,
    last_seen: u64,
}

#[derive(Clone, Debug)]
struct Peer {
    info: KeepAlive,
    addr: Ipv4Addr,
    last_seen: u64,
}

#[derive(Clone, Debug, Default)]
struct Deck {
    status: Option<DeckStatus>,
    last_status: u64,
    last_valid_flags: u8,
}

/// The source's state.
#[derive(Debug)]
pub struct Session {
    settings: Settings,
    started: u64,
    number: u8,
    interface: Option<Ipv4Addr>,
    mac: Option<[u8; 6]>,
    unit: Option<Unit>,
    peers: Vec<Peer>,
    decks: [Deck; 4],
    follow: FollowTarget,
    followed: Option<u8>,
    follow_reported: Option<Option<u8>>,
    tracker: BeatTracker,
    next_announce: Option<u64>,
    published: Option<Vec<DeviceInfo>>,
    published_at: u64,
    warned_waiting: bool,
    warned_interface: bool,
}

impl Session {
    /// A session started at host time `now`.
    #[must_use]
    pub fn new(settings: Settings, now: u64) -> Self {
        let number = settings.device_number;
        let follow = settings.follow;
        let mut s = Self {
            settings,
            started: now,
            number,
            interface: None,
            mac: None,
            unit: None,
            peers: Vec::new(),
            decks: Default::default(),
            follow,
            followed: None,
            follow_reported: None,
            tracker: BeatTracker::new(),
            next_announce: None,
            published: None,
            published_at: 0,
            warned_waiting: false,
            warned_interface: false,
        };
        if let Some(ip) = s.settings.interface {
            s.adopt_interface(ip);
        }
        s
    }

    /// Device number currently announced.
    #[must_use]
    pub fn device_number(&self) -> u8 {
        self.number
    }

    /// Interface address in use, once known.
    #[must_use]
    pub fn interface(&self) -> Option<Ipv4Addr> {
        self.interface
    }

    /// Address of the unit, once seen.
    #[must_use]
    pub fn unit(&self) -> Option<Ipv4Addr> {
        self.unit.as_ref().map(|u| u.addr)
    }

    /// The deck being followed.
    #[must_use]
    pub fn followed(&self) -> Option<u8> {
        self.followed
    }

    /// The unit's address when the shell should find our interface
    /// address toward it (`connect()` on a UDP socket).
    #[must_use]
    pub fn interface_wanted(&self) -> Option<Ipv4Addr> {
        if self.interface.is_none() {
            self.unit()
        } else {
            None
        }
    }

    fn adopt_interface(&mut self, ip: Ipv4Addr) {
        self.interface = Some(ip);
        self.mac = Some(self.settings.mac.unwrap_or_else(|| fallback_mac(ip)));
        self.next_announce = None;
    }

    fn status(out: &mut Vec<Action>, warning: bool, message: String) {
        out.push(Action::Event(SourceEvent::Status { warning, message }));
    }

    /// Sets the interface address found by the shell.
    pub fn set_interface(&mut self, ip: Ipv4Addr, out: &mut Vec<Action>) {
        self.adopt_interface(ip);
        let mac = self.mac.unwrap_or_default();
        Self::status(
            out,
            false,
            format!(
                "announcing as rekordbox lighting, device {}, from {ip} (MAC {})",
                self.number,
                mac.map(|b| format!("{b:02x}")).join(":")
            ),
        );
    }

    /// The shell could not find an interface address toward the unit.
    pub fn interface_failed(&mut self, out: &mut Vec<Action>) {
        if !self.warned_interface {
            self.warned_interface = true;
            Self::status(
                out,
                true,
                "cannot find a local address toward the Opus Quad; set the interface".to_string(),
            );
        }
    }

    /// Changes the follow target.
    pub fn set_follow(&mut self, target: FollowTarget, out: &mut Vec<Action>) {
        self.follow = target;
        self.reselect(out);
    }

    /// Handles a datagram received on the announce port.
    pub fn on_announce(
        &mut self,
        bytes: &[u8],
        from: SocketAddrV4,
        now: u64,
        out: &mut Vec<Action>,
    ) {
        let Ok(AnnouncePacket::KeepAlive(ka)) = parse_announce(bytes) else {
            return;
        };
        if Some(ka.mac) == self.mac {
            return; // our own broadcast
        }
        if ka.name == OPUS_NAME {
            self.see_unit(*from.ip(), now, out);
        } else {
            let addr = *from.ip();
            match self.peers.iter_mut().find(|p| p.info.mac == ka.mac) {
                Some(p) => {
                    p.info = ka;
                    p.addr = addr;
                    p.last_seen = now;
                }
                None => self.peers.push(Peer {
                    info: ka,
                    addr,
                    last_seen: now,
                }),
            }
            self.defend_number(out);
        }
        self.maybe_publish(now, out);
    }

    /// Picks another device number if a peer uses ours (opus-quad.md,
    /// "Device number").
    fn defend_number(&mut self, out: &mut Vec<Action>) {
        let used = |n: u8| self.peers.iter().any(|p| p.info.number == n);
        if !used(self.number) {
            return;
        }
        match FALLBACK_DEVICE_NUMBERS.clone().find(|&n| !used(n)) {
            Some(n) => {
                Self::status(
                    out,
                    true,
                    format!("device number {} is taken; using {n}", self.number),
                );
                self.number = n;
                self.next_announce = None;
            }
            None => Self::status(
                out,
                true,
                format!(
                    "device number {} is taken and no other is free",
                    self.number
                ),
            ),
        }
    }

    /// Notes the unit at `addr`. Returns `false` for a second unit, which
    /// is ignored.
    fn see_unit(&mut self, addr: Ipv4Addr, now: u64, out: &mut Vec<Action>) -> bool {
        match &mut self.unit {
            Some(u) if u.addr == addr => {
                u.last_seen = now;
                true
            }
            Some(_) => false,
            None => {
                self.unit = Some(Unit {
                    addr,
                    last_seen: now,
                });
                Self::status(out, false, format!("Opus Quad found at {addr}"));
                // Ask for status right away rather than at the next tick.
                self.next_announce = None;
                true
            }
        }
    }

    /// Handles a datagram received on the update port at host time `now`.
    pub fn on_update(&mut self, bytes: &[u8], from: SocketAddrV4, now: u64, out: &mut Vec<Action>) {
        let status = match parse_update(bytes) {
            Ok(UpdatePacket::Status(st)) if st.is_opus() => st,
            Ok(UpdatePacket::Hello(hello)) if hello.name == OPUS_NAME => {
                if !self.see_unit(*from.ip(), now, out) {
                    return;
                }
                match hello.status {
                    Some(st) => st,
                    None => {
                        self.maybe_publish(now, out);
                        return;
                    }
                }
            }
            _ => return,
        };
        if !self.see_unit(*from.ip(), now, out) {
            return;
        }
        self.on_status(status, now, out);
        self.maybe_publish(now, out);
    }

    fn on_status(&mut self, mut st: DeckStatus, now: u64, out: &mut Vec<Action>) {
        let Some(deck) = st.deck() else { return };
        let slot = &mut self.decks[usize::from(deck - 1)];
        // A zero flag byte is a known Opus Quad glitch: reuse the last
        // valid one, or drop the packet if there is none (opus-quad.md,
        // "Quirks").
        if st.flags == 0 {
            if slot.last_valid_flags == 0 {
                return;
            }
            st.flags = slot.last_valid_flags;
        } else {
            slot.last_valid_flags = st.flags;
        }
        slot.last_status = now;
        slot.status = Some(st);
        self.reselect(out);
        if self.followed != Some(deck) {
            return;
        }
        let Some(st) = self.decks[usize::from(deck - 1)].status.as_ref() else {
            return;
        };
        let bpm = st.effective_bpm();
        if let Some(est) = self
            .tracker
            .update(now, st.beat_number(), st.is_playing(), bpm)
        {
            let phase = match st.beat_within_bar() {
                Some(b) => Phase::Bar(f64::from(b - 1)),
                None => Phase::Beat(0.0),
            };
            out.push(Action::Event(SourceEvent::Observation {
                host_ns: est.host_ns,
                phase,
                bpm,
                precision: Precision::Coarse,
                device: Some(deck),
            }));
        }
    }

    /// Chooses the deck to follow (opus-quad.md, "player5 policies").
    fn choose(&self) -> Option<u8> {
        let status = |d: u8| self.decks[usize::from(d - 1)].status.as_ref();
        self.unit.as_ref()?;
        match self.follow {
            FollowTarget::Device(d) => (1..=4).contains(&d).then_some(d),
            FollowTarget::Master => {
                // Playing tempo master (staying put during a hand-off),
                // else the deck already followed while it plays, else the
                // only playing deck, else a stopped master.
                let decks = |f: fn(&DeckStatus) -> bool| -> Vec<u8> {
                    (1..=4).filter(|&d| status(d).is_some_and(f)).collect()
                };
                let masters = decks(DeckStatus::is_master);
                let playing = decks(DeckStatus::is_playing);
                let playing_masters: Vec<u8> = masters
                    .iter()
                    .copied()
                    .filter(|d| playing.contains(d))
                    .collect();
                let current = self.followed;
                if let Some(c) = current.filter(|c| playing_masters.contains(c)) {
                    return Some(c);
                }
                if let Some(&d) = playing_masters.first() {
                    return Some(d);
                }
                if let Some(c) = current.filter(|c| playing.contains(c)) {
                    return Some(c);
                }
                if playing.len() == 1 {
                    return Some(playing[0]);
                }
                masters.first().copied()
            }
        }
    }

    fn reselect(&mut self, out: &mut Vec<Action>) {
        let chosen = self.choose();
        if chosen != self.followed {
            self.followed = chosen;
            self.tracker.reset();
        }
        if self.unit.is_some() && self.follow_reported != Some(chosen) {
            self.follow_reported = Some(chosen);
            let message = match (chosen, self.follow) {
                (Some(d), FollowTarget::Master) => format!("following deck {d} (tempo master)"),
                (Some(d), FollowTarget::Device(_)) => format!("following deck {d}"),
                (None, FollowTarget::Master) => {
                    "no deck to follow: no tempo master and not exactly one deck playing"
                        .to_string()
                }
                (None, FollowTarget::Device(d)) => format!("deck {d} does not exist (1-4)"),
            };
            Self::status(out, false, message);
        }
    }

    /// Periodic work at host time `now`: announcements, expiry, device
    /// table. Call at least every 100 ms.
    pub fn tick(&mut self, now: u64, out: &mut Vec<Action>) {
        self.expire(now, out);
        if self.unit.is_none()
            && !self.warned_waiting
            && now.saturating_sub(self.started) > WAITING_WARNING_NS
        {
            self.warned_waiting = true;
            Self::status(
                out,
                true,
                "no Opus Quad seen yet: check the network cable and that the unit is on"
                    .to_string(),
            );
        }
        self.announce(now, out);
        self.maybe_publish(now, out);
    }

    fn announce(&mut self, now: u64, out: &mut Vec<Action>) {
        let (Some(ip), Some(mac)) = (self.interface, self.mac) else {
            return;
        };
        if self.next_announce.is_some_and(|t| now < t) {
            return;
        }
        self.next_announce = Some(now + self.settings.announce_interval_ns);
        let broadcast = self
            .settings
            .broadcast
            .unwrap_or_else(|| default_broadcast(ip));
        out.push(Action::Send {
            via: Via::Announce,
            to: SocketAddrV4::new(broadcast, self.settings.peer_announce_port),
            bytes: KeepAlive::rekordbox(self.number, mac, ip)
                .to_bytes()
                .to_vec(),
        });
        if let Some(unit) = &self.unit {
            out.push(Action::Send {
                via: Via::Update,
                to: SocketAddrV4::new(unit.addr, self.settings.peer_update_port),
                bytes: lighting_request(self.number, &self.settings.computer_name),
            });
        }
    }

    fn expire(&mut self, now: u64, out: &mut Vec<Action>) {
        let stale = |t: u64| now.saturating_sub(t) > EXPIRY_NS;
        self.peers.retain(|p| !stale(p.last_seen));
        for deck in &mut self.decks {
            if deck.status.is_some() && stale(deck.last_status) {
                deck.status = None;
            }
        }
        if let Some(u) = &self.unit {
            if stale(u.last_seen) {
                let addr = u.addr;
                self.unit = None;
                self.decks = Default::default();
                self.follow_reported = None;
                Self::status(out, true, format!("lost the Opus Quad at {addr}"));
            }
        }
        self.reselect(out);
    }

    /// The device table: the unit's four decks plus other devices seen.
    #[must_use]
    pub fn devices(&self) -> Vec<DeviceInfo> {
        let mut list = Vec::new();
        if let Some(unit) = &self.unit {
            for (i, deck) in self.decks.iter().enumerate() {
                let st = deck.status.as_ref();
                list.push(DeviceInfo {
                    number: i as u8 + 1,
                    name: OPUS_NAME.to_string(),
                    address: unit.addr.to_string(),
                    kind: DeviceKind::AllInOne,
                    bpm: st
                        .and_then(DeckStatus::effective_bpm)
                        .map(|b| (b * 100.0).round() / 100.0),
                    playing: st.map(DeckStatus::is_playing),
                    master: st.map(DeckStatus::is_master),
                    on_air: st.map(DeckStatus::is_on_air),
                });
            }
        }
        for p in &self.peers {
            list.push(DeviceInfo {
                number: p.info.number,
                name: p.info.name.clone(),
                address: p.addr.to_string(),
                kind: classify(&p.info),
                bpm: None,
                playing: None,
                master: None,
                on_air: None,
            });
        }
        list.sort_by(|a, b| (a.number, &a.address).cmp(&(b.number, &b.address)));
        list
    }

    fn maybe_publish(&mut self, now: u64, out: &mut Vec<Action>) {
        let list = self.devices();
        let membership = |l: &[DeviceInfo]| -> Vec<(u8, String)> {
            l.iter().map(|d| (d.number, d.address.clone())).collect()
        };
        let due = match &self.published {
            None => !list.is_empty(),
            Some(prev) if *prev == list => false,
            Some(prev) => {
                membership(prev) != membership(&list)
                    || now.saturating_sub(self.published_at) >= DEVICES_MIN_INTERVAL_NS
            }
        };
        if due {
            self.published = Some(list.clone());
            self.published_at = now;
            out.push(Action::Event(SourceEvent::Devices(list)));
        }
    }
}
