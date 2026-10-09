//! Joining the network as a virtual player: the documented startup
//! sequence (three hellos, three claims in each of three stages, 300 ms
//! apart), then keep-alives. See "Joining" in
//! `docs/protocols/pro-dj-link.md`.
//!
//! Pure and clock-driven: the caller feeds packets and the current time and
//! sends whatever comes back. Real hardware always wins: if another device
//! claims, announces or defends our number, we give it up rather than
//! defend it, and the source carries on listening passively.

use std::net::Ipv4Addr;

use super::packets::{
    build_assignment_request, build_claim_stage1, build_claim_stage2, build_claim_stage3,
    build_hello, build_keep_alive, KeepAlive,
};

/// Interval between startup packets.
pub(crate) const STEP_NS: u64 = 300_000_000;
/// Keep-alive interval once joined.
pub(crate) const KEEP_ALIVE_NS: u64 = 1_500_000_000;
/// Minimum time spent listening before our number goes on the wire: longer
/// than one CDJ keep-alive period, so every present player has announced
/// itself (some all-in-one units do not defend their numbers).
pub(crate) const LISTEN_BEFORE_CLAIM_NS: u64 = 2_100_000_000;

/// A packet to send to the announcement port.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Outgoing {
    /// To the broadcast address.
    Broadcast(Vec<u8>),
    /// Straight to one device.
    Unicast(Ipv4Addr, Vec<u8>),
}

/// Something worth telling the user about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum JoinEvent {
    /// Started the claim sequence.
    Claiming(u8),
    /// Finished; keep-alives are going out.
    Joined(u8),
    /// A mixer assigned us a different number.
    Assigned { number: u8, by: Ipv4Addr },
    /// Another device has our number; we stopped announcing.
    Yielded {
        number: u8,
        by: Ipv4Addr,
        name: String,
    },
}

/// What to do after feeding the joiner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum JoinAction {
    Send(Outgoing),
    Event(JoinEvent),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// No interface address yet.
    Waiting,
    /// Sent `n` hellos.
    Hello(u8),
    /// Sent `n` stage-1 claims.
    Stage1(u8),
    /// Sent `n` stage-2 claims.
    Stage2(u8),
    /// Sent `n` stage-3 claims.
    Stage3(u8),
    Joined,
    Yielded,
}

/// The join state machine.
#[derive(Clone, Debug)]
pub(crate) struct Joiner {
    name: String,
    mac: [u8; 6],
    number: u8,
    ip: Option<Ipv4Addr>,
    phase: Phase,
    listen_since: u64,
    next_at: u64,
    first_on_network: bool,
}

impl Joiner {
    /// A joiner that will claim `number` as `name`; `now` is when listening
    /// started.
    pub(crate) fn new(name: &str, number: u8, now: u64) -> Self {
        Self {
            name: name.to_owned(),
            mac: [0; 6],
            number,
            ip: None,
            phase: Phase::Waiting,
            listen_since: now,
            next_at: now,
            first_on_network: false,
        }
    }

    /// Sets the addresses we announce (our interface on the booth network).
    pub(crate) fn set_interface(&mut self, ip: Ipv4Addr, mac: [u8; 6]) {
        self.ip = Some(ip);
        self.mac = mac;
    }

    /// The number we hold or are claiming.
    #[cfg(test)]
    pub(crate) fn number(&self) -> u8 {
        self.number
    }

    /// Whether the claim finished and keep-alives are going out.
    #[cfg(test)]
    pub(crate) fn is_joined(&self) -> bool {
        self.phase == Phase::Joined
    }

    /// Whether we gave our number up.
    pub(crate) fn has_yielded(&self) -> bool {
        self.phase == Phase::Yielded
    }

    /// Whether `name`/`ip` is us (our own broadcasts come back to us).
    pub(crate) fn is_self(&self, name: &str, ip: Ipv4Addr) -> bool {
        self.ip == Some(ip) && name == self.name
    }

    /// When the joiner next wants [`Joiner::poll`], if it is waiting on time.
    pub(crate) fn deadline(&self) -> Option<u64> {
        match self.phase {
            Phase::Waiting | Phase::Yielded => None,
            _ => Some(self.next_at),
        }
    }

    fn claiming(&self) -> bool {
        matches!(
            self.phase,
            Phase::Hello(_) | Phase::Stage1(_) | Phase::Stage2(_) | Phase::Stage3(_)
        )
    }

    fn keep_alive(&self, peers: usize) -> Vec<u8> {
        build_keep_alive(&KeepAlive {
            number: self.number,
            name: self.name.clone(),
            mac: self.mac,
            ip: self.ip.unwrap_or(Ipv4Addr::UNSPECIFIED),
            peers: u8::try_from(peers.saturating_add(1)).unwrap_or(u8::MAX),
            device_type: 0x01,
            first_on_network: self.first_on_network,
            cdj3000_compatible: true,
        })
    }

    /// Advances to `now`. `peers` is how many other devices are on the
    /// network.
    pub(crate) fn poll(&mut self, now: u64, peers: usize) -> Vec<JoinAction> {
        let mut out = Vec::new();
        if self.phase == Phase::Waiting {
            if self.ip.is_none() {
                return out;
            }
            self.phase = Phase::Hello(0);
            self.next_at = now;
            self.first_on_network = peers == 0;
            out.push(JoinAction::Event(JoinEvent::Claiming(self.number)));
        }
        // At most one packet per call; transitions without a send loop.
        for _ in 0..8 {
            if now < self.next_at {
                break;
            }
            let ip = self.ip.unwrap_or(Ipv4Addr::UNSPECIFIED);
            match self.phase {
                Phase::Waiting | Phase::Yielded => break,
                Phase::Hello(n) if n < 3 => {
                    out.push(JoinAction::Send(Outgoing::Broadcast(build_hello(
                        &self.name,
                    ))));
                    self.phase = Phase::Hello(n + 1);
                    self.next_at = now + STEP_NS;
                    break;
                }
                Phase::Hello(_) => self.phase = Phase::Stage1(0),
                Phase::Stage1(n) if n < 3 => {
                    out.push(JoinAction::Send(Outgoing::Broadcast(build_claim_stage1(
                        &self.name,
                        self.mac,
                        n + 1,
                    ))));
                    self.phase = Phase::Stage1(n + 1);
                    self.next_at = now + STEP_NS;
                    break;
                }
                Phase::Stage1(_) => {
                    let earliest = self.listen_since + LISTEN_BEFORE_CLAIM_NS;
                    if now < earliest {
                        self.next_at = earliest;
                        break;
                    }
                    self.phase = Phase::Stage2(0);
                }
                Phase::Stage2(n) if n < 3 => {
                    out.push(JoinAction::Send(Outgoing::Broadcast(build_claim_stage2(
                        &self.name,
                        ip,
                        self.mac,
                        self.number,
                        n + 1,
                        false,
                    ))));
                    self.phase = Phase::Stage2(n + 1);
                    self.next_at = now + STEP_NS;
                    break;
                }
                Phase::Stage2(_) => self.phase = Phase::Stage3(0),
                Phase::Stage3(n) if n < 3 => {
                    out.push(JoinAction::Send(Outgoing::Broadcast(build_claim_stage3(
                        &self.name,
                        self.number,
                        n + 1,
                    ))));
                    self.phase = Phase::Stage3(n + 1);
                    self.next_at = now + STEP_NS;
                    break;
                }
                Phase::Stage3(_) => {
                    self.phase = Phase::Joined;
                    out.push(JoinAction::Event(JoinEvent::Joined(self.number)));
                    // Fall through to the first keep-alive.
                }
                Phase::Joined => {
                    out.push(JoinAction::Send(Outgoing::Broadcast(
                        self.keep_alive(peers),
                    )));
                    self.next_at = now + KEEP_ALIVE_NS;
                    break;
                }
            }
        }
        out
    }

    fn yield_to(&mut self, by: Ipv4Addr, name: &str) -> Vec<JoinAction> {
        if self.phase == Phase::Yielded {
            return Vec::new();
        }
        self.phase = Phase::Yielded;
        vec![JoinAction::Event(JoinEvent::Yielded {
            number: self.number,
            by,
            name: name.to_owned(),
        })]
    }

    /// Another device announced `number` (keep-alive) or claimed it (stage
    /// 2/3). Not for our own packets.
    pub(crate) fn on_number_seen(
        &mut self,
        number: u8,
        by: Ipv4Addr,
        name: &str,
    ) -> Vec<JoinAction> {
        if number == self.number && self.phase != Phase::Yielded {
            return self.yield_to(by, name);
        }
        Vec::new()
    }

    /// A device defended `number` against us.
    pub(crate) fn on_number_in_use(
        &mut self,
        number: u8,
        by: Ipv4Addr,
        name: &str,
    ) -> Vec<JoinAction> {
        self.on_number_seen(number, by, name)
    }

    /// A mixer said it will assign our number: answer it directly.
    pub(crate) fn on_assignment_intention(&mut self, mixer: Ipv4Addr) -> Vec<JoinAction> {
        match (self.phase, self.ip) {
            (Phase::Stage1(_) | Phase::Stage2(_), Some(ip)) => {
                vec![JoinAction::Send(Outgoing::Unicast(
                    mixer,
                    build_assignment_request(&self.name, ip, self.mac, false),
                ))]
            }
            _ => Vec::new(),
        }
    }

    /// A mixer assigned us `number` (0 = "any"): take it and go straight
    /// to the final stage.
    pub(crate) fn on_assignment(
        &mut self,
        number: u8,
        mixer: Ipv4Addr,
        now: u64,
    ) -> Vec<JoinAction> {
        if !self.claiming() {
            return Vec::new();
        }
        let mut out = Vec::new();
        if number != 0 && number != self.number {
            self.number = number;
            out.push(JoinAction::Event(JoinEvent::Assigned { number, by: mixer }));
        }
        if !matches!(self.phase, Phase::Stage3(_)) {
            self.phase = Phase::Stage3(0);
            self.next_at = now;
        }
        out
    }

    /// Someone confirmed our claim: skip the rest of the final stage.
    pub(crate) fn on_assignment_finished(&mut self, now: u64) {
        if let Phase::Stage3(n) = self.phase {
            if n > 0 {
                self.phase = Phase::Stage3(3);
                self.next_at = now;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::packets::{
        parse_keep_alive, parse_number_claim, PacketKind, Port, KEEP_ALIVE_LEN,
    };
    use super::*;

    const MS: u64 = 1_000_000;
    const IP: Ipv4Addr = Ipv4Addr::new(169, 254, 1, 5);
    const MAC: [u8; 6] = [0x02, 0x70, 169, 254, 1, 5];

    fn sends(actions: &[JoinAction]) -> Vec<Vec<u8>> {
        actions
            .iter()
            .filter_map(|a| match a {
                JoinAction::Send(Outgoing::Broadcast(p) | Outgoing::Unicast(_, p)) => {
                    Some(p.clone())
                }
                JoinAction::Event(_) => None,
            })
            .collect()
    }

    /// Runs the joiner in 10 ms steps and records (time, kind) of each send.
    fn run(j: &mut Joiner, until: u64) -> Vec<(u64, PacketKind, Vec<u8>)> {
        let mut log = Vec::new();
        let mut t = 0;
        while t <= until {
            for p in sends(&j.poll(t, 2)) {
                log.push((t, PacketKind::classify(Port::Announce, &p).unwrap(), p));
            }
            t += 10 * MS;
        }
        log
    }

    #[test]
    fn waits_for_an_interface() {
        let mut j = Joiner::new("player5", 5, 0);
        assert!(j.poll(10 * 1000 * MS, 0).is_empty());
        assert_eq!(j.deadline(), None);
    }

    #[test]
    fn follows_the_documented_startup_sequence() {
        let mut j = Joiner::new("player5", 5, 0);
        j.set_interface(IP, MAC);
        let log = run(&mut j, 8000 * MS);
        let kinds: Vec<PacketKind> = log.iter().map(|(_, k, _)| *k).collect();
        use PacketKind::*;
        assert_eq!(
            &kinds[..13],
            &[
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
                ClaimStage3,
                ClaimStage3,
                KeepAlive
            ]
        );
        // 300 ms apart through stage 1.
        for w in log[..6].windows(2) {
            assert_eq!(w[1].0 - w[0].0, STEP_NS);
        }
        // Our number is not on the wire before the listening period.
        assert!(log[6].0 >= LISTEN_BEFORE_CLAIM_NS);
        let c = parse_number_claim(&log[7].2).unwrap();
        assert_eq!(
            (c.stage, c.number, c.counter, c.ip),
            (2, Some(5), 2, Some(IP))
        );
        assert_eq!(c.auto_assign, Some(false));
        let c3 = parse_number_claim(&log[11].2).unwrap();
        assert_eq!((c3.stage, c3.number, c3.counter), (3, Some(5), 3));
        // Keep-alives every 1.5 s after joining.
        assert!(j.is_joined());
        let ka: Vec<&(u64, PacketKind, Vec<u8>)> =
            log.iter().filter(|(_, k, _)| *k == KeepAlive).collect();
        assert!(ka.len() >= 2);
        assert_eq!(ka[1].0 - ka[0].0, KEEP_ALIVE_NS);
        let k = parse_keep_alive(&ka[0].2).unwrap();
        assert_eq!(ka[0].2.len(), KEEP_ALIVE_LEN);
        assert_eq!(
            (k.number, k.name.as_str(), k.ip, k.mac),
            (5, "player5", IP, MAC)
        );
        assert_eq!((k.device_type, k.peers, k.cdj3000_compatible), (1, 3, true));
    }

    #[test]
    fn yields_to_a_device_with_our_number() {
        let mut j = Joiner::new("player5", 5, 0);
        j.set_interface(IP, MAC);
        let _ = j.poll(0, 0);
        let other = Ipv4Addr::new(169, 254, 1, 9);
        let a = j.on_number_seen(5, other, "CDJ-3000");
        assert_eq!(
            a,
            vec![JoinAction::Event(JoinEvent::Yielded {
                number: 5,
                by: other,
                name: "CDJ-3000".into()
            })]
        );
        assert!(j.has_yielded());
        assert!(run(&mut j, 10_000 * MS).is_empty(), "silent after yielding");
        // Other numbers are no conflict.
        let mut j = Joiner::new("player5", 5, 0);
        assert!(j.on_number_seen(2, other, "CDJ-3000").is_empty());
        // Even after joining, a later claim of our number wins.
        j.set_interface(IP, MAC);
        let _ = run(&mut j, 8000 * MS);
        assert!(j.is_joined());
        assert_eq!(j.on_number_in_use(5, other, "CDJ-3000").len(), 1);
        assert!(j.has_yielded());
    }

    #[test]
    fn takes_a_number_from_a_mixer() {
        let mut j = Joiner::new("player5", 5, 0);
        j.set_interface(IP, MAC);
        let mixer = Ipv4Addr::new(169, 254, 1, 33);
        // Ignored before claiming starts in earnest.
        assert!(j.on_assignment_intention(mixer).is_empty());
        let mut t = 0;
        while !matches!(j.phase, Phase::Stage1(1)) {
            let _ = j.poll(t, 1);
            t += 10 * MS;
        }
        let req = sends(&j.on_assignment_intention(mixer));
        let c = parse_number_claim(&req[0]).unwrap();
        assert!(c.assignment_request);
        assert_eq!((c.stage, c.number, c.ip), (2, None, Some(IP)));
        let a = j.on_assignment(3, mixer, t);
        assert_eq!(
            a,
            vec![JoinAction::Event(JoinEvent::Assigned {
                number: 3,
                by: mixer
            })]
        );
        let next = sends(&j.poll(t, 1));
        let c3 = parse_number_claim(&next[0]).unwrap();
        assert_eq!((c3.stage, c3.number, c3.counter), (3, Some(3), 1));
        j.on_assignment_finished(t + MS);
        let acts = j.poll(t + MS, 1);
        assert!(acts.contains(&JoinAction::Event(JoinEvent::Joined(3))));
        let ka = sends(&acts);
        assert_eq!(parse_keep_alive(&ka[0]).unwrap().number, 3);
        assert_eq!(j.number(), 3);
    }

    #[test]
    fn recognises_its_own_broadcasts() {
        let mut j = Joiner::new("player5", 5, 0);
        assert!(!j.is_self("player5", IP));
        j.set_interface(IP, MAC);
        assert!(j.is_self("player5", IP));
        assert!(!j.is_self("player5", Ipv4Addr::new(169, 254, 1, 6)));
        assert!(!j.is_self("CDJ-3000", IP));
    }
}
