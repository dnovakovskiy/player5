//! What we know about every device on the network, and which one to
//! follow. Pure: fed parsed packets with receive times.

use std::collections::BTreeMap;
use std::net::Ipv4Addr;

use super::packets::{
    classify_device, BeatPacket, CdjStatus, KeepAlive, MixerStatus, OnAir, PrecisePosition,
};
use crate::net::{DeviceInfo, DeviceKind, FollowTarget};

/// A device that has sent nothing for this long has left ("Keep-alive").
pub(crate) const DEVICE_TIMEOUT_NS: u64 = 10_000_000_000;
/// Status arrives about every 200 ms; older than this it is stale.
pub(crate) const STATUS_FRESH_NS: u64 = 1_000_000_000;
/// Without status, a player counts as playing while beats keep coming:
/// within this long, or 2.5 beat intervals if that is longer.
const BEAT_WINDOW_NS: u64 = 2_000_000_000;
/// Status and beat tempo beat the coarser precise-position tempo for this
/// long.
const BPM_PRIORITY_NS: u64 = 1_000_000_000;

#[derive(Clone, Debug)]
struct Status {
    at: u64,
    playing: bool,
    master: bool,
    on_air: bool,
    handoff: Option<u8>,
    bar_meaningful: bool,
}

#[derive(Clone, Debug)]
struct Peer {
    name: String,
    address: Ipv4Addr,
    device_type: Option<u8>,
    last_seen: u64,
    status: Option<Status>,
    last_beat: Option<u64>,
    beat_interval: Option<u64>,
    bpm: Option<(f64, u64)>,
    on_air: Option<bool>,
}

impl Peer {
    fn new(name: &str, address: Ipv4Addr, now: u64) -> Self {
        Self {
            name: name.to_owned(),
            address,
            device_type: None,
            last_seen: now,
            status: None,
            last_beat: None,
            beat_interval: None,
            bpm: None,
            on_air: None,
        }
    }

    fn fresh_status(&self, now: u64) -> Option<&Status> {
        self.status
            .as_ref()
            .filter(|s| now.saturating_sub(s.at) <= STATUS_FRESH_NS)
    }
}

/// The device table.
#[derive(Clone, Debug, Default)]
pub(crate) struct DeviceTable {
    peers: BTreeMap<u8, Peer>,
}

impl DeviceTable {
    fn touch(&mut self, number: u8, name: &str, from: Ipv4Addr, now: u64) -> &mut Peer {
        let peer = self
            .peers
            .entry(number)
            .or_insert_with(|| Peer::new(name, from, now));
        peer.last_seen = now;
        peer.address = from;
        if !name.is_empty() {
            name.clone_into(&mut peer.name);
        }
        peer
    }

    /// Number of devices present.
    pub(crate) fn len(&self) -> usize {
        self.peers.len()
    }

    /// Whether `number` is present.
    #[cfg(test)]
    pub(crate) fn contains(&self, number: u8) -> bool {
        self.peers.contains_key(&number)
    }

    /// Name of `number`, if present.
    pub(crate) fn name(&self, number: u8) -> Option<&str> {
        self.peers.get(&number).map(|p| p.name.as_str())
    }

    /// Kind of `number` (guessed from the number if no keep-alive yet).
    pub(crate) fn kind(&self, number: u8) -> DeviceKind {
        match self.peers.get(&number) {
            Some(p) => classify_device(&p.name, number, p.device_type),
            None => classify_device("", number, None),
        }
    }

    /// Records a keep-alive.
    pub(crate) fn keep_alive(&mut self, k: &KeepAlive, from: Ipv4Addr, now: u64) {
        let peer = self.touch(k.number, &k.name, from, now);
        peer.device_type = Some(k.device_type);
    }

    /// Records a beat.
    pub(crate) fn beat(&mut self, b: &BeatPacket, from: Ipv4Addr, now: u64) {
        let peer = self.touch(b.device, &b.name, from, now);
        if let Some(prev) = peer.last_beat {
            let gap = now.saturating_sub(prev);
            peer.beat_interval = (gap > 0 && gap <= 4 * BEAT_WINDOW_NS).then_some(gap);
        }
        peer.last_beat = Some(now);
        if let Some(bpm) = b.effective_bpm() {
            peer.bpm = Some((bpm, now));
        }
    }

    /// Records a CDJ status.
    pub(crate) fn cdj_status(&mut self, s: &CdjStatus, from: Ipv4Addr, now: u64) {
        let peer = self.touch(s.device, &s.name, from, now);
        peer.status = Some(Status {
            at: now,
            playing: s.playing(),
            master: s.master(),
            on_air: s.on_air(),
            handoff: s.master_handoff_to(),
            bar_meaningful: s.beat_within_bar > 0,
        });
        if let Some(bpm) = s.effective_bpm() {
            peer.bpm = Some((bpm, now));
        }
    }

    /// Records a mixer status.
    pub(crate) fn mixer_status(&mut self, m: &MixerStatus, from: Ipv4Addr, now: u64) {
        let peer = self.touch(m.device, &m.name, from, now);
        peer.device_type.get_or_insert(2);
        peer.status = Some(Status {
            at: now,
            playing: false,
            master: m.master(),
            on_air: false,
            handoff: (m.master_handoff != 0xff && m.master_handoff != 0)
                .then_some(m.master_handoff),
            bar_meaningful: false,
        });
        if let Some(bpm) = m.effective_bpm() {
            peer.bpm = Some((bpm, now));
        }
    }

    /// Records a precise position (tempo only; see "Precise position").
    pub(crate) fn precise_position(&mut self, p: &PrecisePosition, from: Ipv4Addr, now: u64) {
        let peer = self.touch(p.device, &p.name, from, now);
        let stale = peer
            .bpm
            .map_or(true, |(_, at)| now.saturating_sub(at) > BPM_PRIORITY_NS);
        if let (true, Some(bpm)) = (stale, p.effective_bpm()) {
            peer.bpm = Some((bpm, now));
        }
    }

    /// Records the mixer's channels-on-air flags.
    pub(crate) fn on_air(&mut self, o: &OnAir) {
        for (i, flag) in o.channels.iter().enumerate() {
            let number = i as u8 + 1;
            if let (Some(flag), Some(peer)) = (flag, self.peers.get_mut(&number)) {
                peer.on_air = Some(*flag);
            }
        }
    }

    /// Drops devices not heard from for [`DEVICE_TIMEOUT_NS`]; returns
    /// whether any left.
    pub(crate) fn expire(&mut self, now: u64) -> bool {
        let before = self.peers.len();
        self.peers
            .retain(|_, p| now.saturating_sub(p.last_seen) <= DEVICE_TIMEOUT_NS);
        self.peers.len() != before
    }

    /// Whether `number` is playing: from fresh status, else from recent
    /// beats. `None` if nothing is known.
    pub(crate) fn playing(&self, number: u8, now: u64) -> Option<bool> {
        let peer = self.peers.get(&number)?;
        if let Some(s) = peer.fresh_status(now) {
            if !matches!(self.kind(number), DeviceKind::Mixer) {
                return Some(s.playing);
            }
        }
        let last = peer.last_beat?;
        let window = peer
            .beat_interval
            .map_or(BEAT_WINDOW_NS, |i| (i * 5 / 2).max(BEAT_WINDOW_NS));
        Some(now.saturating_sub(last) <= window)
    }

    /// The device reporting itself tempo master in fresh status. During a
    /// handoff both may claim it; the incoming one (not yielding) wins.
    pub(crate) fn master(&self, now: u64) -> Option<u8> {
        let masters = self
            .peers
            .iter()
            .filter_map(|(n, p)| p.fresh_status(now).filter(|s| s.master).map(|s| (*n, s)));
        let mut best: Option<(u8, bool)> = None;
        for (n, s) in masters {
            let settled = s.handoff.is_none();
            match best {
                Some((_, true)) if !settled => {}
                Some((_, b)) if b == settled => {}
                _ => best = Some((n, settled)),
            }
        }
        best.map(|(n, _)| n)
    }

    /// Which device to follow: the requested one, or for
    /// [`FollowTarget::Master`] the tempo master, else the lowest-numbered
    /// playing player.
    pub(crate) fn resolve(&self, target: FollowTarget, now: u64) -> Option<u8> {
        match target {
            FollowTarget::Device(n) => Some(n),
            FollowTarget::Master => self.master(now).or_else(|| {
                self.peers.keys().copied().find(|&n| {
                    self.kind(n) == DeviceKind::Player && self.playing(n, now) == Some(true)
                })
            }),
        }
    }

    /// Whether the beat-within-bar of `number`'s beats marks real bars:
    /// never for mixers; for players while their status says so, and from
    /// player numbers when no status has arrived.
    pub(crate) fn bar_meaningful(&self, number: u8, beat_within_bar: u8, now: u64) -> bool {
        if !(1..=4).contains(&beat_within_bar) {
            return false;
        }
        match self.kind(number) {
            DeviceKind::Mixer | DeviceKind::Rekordbox => false,
            _ => match self.peers.get(&number).and_then(|p| p.fresh_status(now)) {
                Some(s) => s.bar_meaningful,
                None => number < 0x21,
            },
        }
    }

    /// The table as the `net` layer reports it, ordered by number.
    pub(crate) fn snapshot(&self, now: u64) -> Vec<DeviceInfo> {
        self.peers
            .iter()
            .map(|(&number, p)| {
                let status = p.fresh_status(now);
                let kind = self.kind(number);
                let player = kind == DeviceKind::Player;
                DeviceInfo {
                    number,
                    name: p.name.clone(),
                    address: p.address.to_string(),
                    kind,
                    bpm: p.bpm.map(|(b, _)| (b * 100.0).round() / 100.0),
                    playing: if player {
                        self.playing(number, now)
                    } else {
                        None
                    },
                    master: status.map(|s| s.master),
                    on_air: p.on_air.or(status.filter(|_| player).map(|s| s.on_air)),
                }
            })
            .collect()
    }
}

/// Whether two snapshots differ enough to report: anything but tempo, or
/// tempo by more than `bpm_tolerance`.
pub(crate) fn differs(a: &[DeviceInfo], b: &[DeviceInfo], bpm_tolerance: f64) -> bool {
    if a.len() != b.len() {
        return true;
    }
    a.iter().zip(b).any(|(x, y)| {
        let bpm_moved = match (x.bpm, y.bpm) {
            (Some(p), Some(q)) => (p - q).abs() > bpm_tolerance,
            (None, None) => false,
            _ => true,
        };
        bpm_moved
            || x.number != y.number
            || x.name != y.name
            || x.address != y.address
            || x.kind != y.kind
            || x.playing != y.playing
            || x.master != y.master
            || x.on_air != y.on_air
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1_000_000_000;
    const MS: u64 = 1_000_000;

    fn ip(n: u8) -> Ipv4Addr {
        Ipv4Addr::new(169, 254, 0, n)
    }

    fn keep_alive(number: u8, name: &str, device_type: u8) -> KeepAlive {
        KeepAlive {
            number,
            name: name.into(),
            mac: [0; 6],
            ip: ip(number),
            peers: 1,
            device_type,
            first_on_network: false,
            cdj3000_compatible: true,
        }
    }

    #[test]
    fn master_from_status_beats_fallback() {
        let mut t = DeviceTable::default();
        t.keep_alive(&keep_alive(1, "CDJ-3000", 1), ip(1), 0);
        t.keep_alive(&keep_alive(2, "CDJ-3000", 1), ip(2), 0);
        t.keep_alive(&keep_alive(0x21, "DJM-V10", 2), ip(0x21), 0);
        assert_eq!(t.resolve(FollowTarget::Master, 0), None);
        t.cdj_status(&CdjStatus::new(1, 120.0, 0.0, true, false), ip(1), S);
        t.cdj_status(&CdjStatus::new(2, 124.0, 0.0, true, true), ip(2), S);
        assert_eq!(t.master(S), Some(2));
        assert_eq!(t.resolve(FollowTarget::Master, S), Some(2));
        assert_eq!(t.resolve(FollowTarget::Device(1), S), Some(1));
        // Status goes stale: fall back to the lowest playing player, which
        // is unknown without beats.
        assert_eq!(t.resolve(FollowTarget::Master, 3 * S), None);
        t.beat(&BeatPacket::new(2, 124.0, 0.0, 1), ip(2), 3 * S);
        t.beat(&BeatPacket::new(1, 120.0, 0.0, 1), ip(1), 3 * S);
        assert_eq!(t.resolve(FollowTarget::Master, 3 * S), Some(1));
        // The mixer's beats never make it a candidate.
        t.beat(&BeatPacket::new(0x21, 120.0, 0.0, 1), ip(0x21), 3 * S);
        assert_eq!(t.kind(0x21), DeviceKind::Mixer);
        assert!(!t.bar_meaningful(0x21, 1, 3 * S));
        assert!(t.bar_meaningful(1, 1, 3 * S));
        assert!(!t.bar_meaningful(1, 0, 3 * S));
    }

    #[test]
    fn handoff_prefers_the_incoming_master() {
        let mut t = DeviceTable::default();
        let mut outgoing = CdjStatus::new(1, 120.0, 0.0, true, true);
        outgoing.master_handoff = 3;
        t.cdj_status(&outgoing, ip(1), 0);
        assert_eq!(t.master(0), Some(1));
        t.cdj_status(&CdjStatus::new(3, 120.0, 0.0, true, true), ip(3), 0);
        assert_eq!(t.master(0), Some(3));
    }

    #[test]
    fn playing_from_beats_expires() {
        let mut t = DeviceTable::default();
        assert_eq!(t.playing(1, 0), None);
        t.beat(&BeatPacket::new(1, 120.0, 0.0, 1), ip(1), 0);
        t.beat(&BeatPacket::new(1, 120.0, 0.0, 2), ip(1), 500 * MS);
        assert_eq!(t.playing(1, 2 * S), Some(true));
        assert_eq!(t.playing(1, 3 * S), Some(false));
        // Slow tempo widens the window.
        t.beat(&BeatPacket::new(1, 30.0, 0.0, 3), ip(1), 4 * S);
        t.beat(&BeatPacket::new(1, 30.0, 0.0, 4), ip(1), 6 * S);
        assert_eq!(t.playing(1, 10 * S), Some(true));
    }

    #[test]
    fn devices_expire_and_snapshot() {
        let mut t = DeviceTable::default();
        t.keep_alive(&keep_alive(3, "CDJ-3000", 1), ip(3), 0);
        t.keep_alive(&keep_alive(0x21, "DJM-A9", 2), ip(0x21), 5 * S);
        t.cdj_status(&CdjStatus::new(3, 128.0, 1.0, true, true), ip(3), 5 * S);
        t.on_air(&OnAir {
            device: 0x21,
            name: "DJM-A9".into(),
            channels: [
                Some(false),
                Some(false),
                Some(true),
                Some(false),
                None,
                None,
            ],
        });
        let snap = t.snapshot(5 * S);
        assert_eq!(snap.len(), 2);
        assert_eq!(snap[0].number, 3);
        assert_eq!(snap[0].kind, DeviceKind::Player);
        assert_eq!(snap[0].bpm, Some(129.28));
        assert_eq!(
            (snap[0].playing, snap[0].master, snap[0].on_air),
            (Some(true), Some(true), Some(true))
        );
        assert_eq!(snap[1].kind, DeviceKind::Mixer);
        assert_eq!(snap[1].playing, None);
        t.keep_alive(&keep_alive(0x21, "DJM-A9", 2), ip(0x21), 7 * S);
        assert!(!t.expire(15 * S));
        assert!(t.expire(16 * S));
        assert_eq!(t.len(), 1);
        assert!(t.contains(0x21));
        assert!(!t.expire(16 * S));
    }

    #[test]
    fn precise_position_tempo_yields_to_beats() {
        let mut t = DeviceTable::default();
        t.beat(&BeatPacket::new(1, 120.0, 0.0, 1), ip(1), 0);
        let pp = PrecisePosition {
            device: 1,
            name: "CDJ-3000".into(),
            track_length_s: 300,
            playhead_ms: 1000,
            pitch_x100: 0,
            bpm_x10: 1201,
        };
        t.precise_position(&pp, ip(1), 100 * MS);
        assert_eq!(t.snapshot(100 * MS)[0].bpm, Some(120.0));
        t.precise_position(&pp, ip(1), 2 * S);
        assert_eq!(t.snapshot(2 * S)[0].bpm, Some(120.1));
    }

    #[test]
    fn snapshot_differences() {
        let mut t = DeviceTable::default();
        t.beat(&BeatPacket::new(1, 120.0, 0.0, 1), ip(1), 0);
        let a = t.snapshot(0);
        t.beat(&BeatPacket::new(1, 120.02, 0.0, 2), ip(1), 500 * MS);
        let b = t.snapshot(500 * MS);
        assert!(!differs(&a, &b, 0.05));
        assert!(differs(&a, &b, 0.0));
        assert!(differs(&a, &[], 0.05));
    }
}
