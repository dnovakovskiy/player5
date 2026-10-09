//! The running Pro DJ Link source: configuration, sockets, the receive
//! threads, and the [`Tracker`] that turns packets into
//! [`SourceEvent`]s.
//!
//! One receive thread per port timestamps each datagram with
//! [`host_time::now_ns`] the moment `recv_from` returns, so beat timing is
//! never delayed by work on another socket. The source thread owns all
//! state; it wakes for packets, for the join sequence's deadlines and at
//! least every 50 ms to check for stop requests.

use std::collections::{BTreeMap, VecDeque};
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddrV4, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::devices::{differs, DeviceTable, STATUS_FRESH_NS};
use super::join::{JoinAction, JoinEvent, Joiner, Outgoing};
use super::packets::{
    device_name, parse_assignment, parse_beat, parse_cdj_status, parse_keep_alive,
    parse_mixer_status, parse_number_claim, parse_number_in_use, parse_on_air,
    parse_precise_position, PacketKind, Port, ANNOUNCE_PORT, BEAT_PORT, NAME_LEN, OPUS_QUAD_NAME,
    STATUS_PORT,
};
use crate::follower::{Phase, Precision};
use crate::host_time;
use crate::net::{
    DeviceInfo, DeviceKind, FollowTarget, SourceCommand, SourceContext, SourceEvent, SourceHandle,
};

/// How often the source thread wakes at the least.
const POLL: Duration = Duration::from_millis(50);
/// Read timeout of the receive threads (bounds how long they take to stop).
const RX_TIMEOUT: Duration = Duration::from_millis(50);
/// Warn after this long without any Pro DJ Link packet.
const NO_TRAFFIC_NS: u64 = 5_000_000_000;
/// Warn if joined this long without a single status packet while players
/// are present.
const NO_STATUS_NS: u64 = 5_000_000_000;
/// Re-report the device list for tempo-only changes at most this often.
const DEVICES_MIN_INTERVAL_NS: u64 = 1_000_000_000;
/// Tempo changes smaller than this are not worth an observation.
const TEMPO_EPSILON: f64 = 0.001;
/// Backoff after a failed interface discovery or send.
const RETRY_NS: u64 = 5_000_000_000;
/// Our own broadcasts come back to us. An announcement byte-identical to
/// one we broadcast this recently is our echo, whatever source address the
/// operating system gave it.
const ECHO_WINDOW_NS: u64 = 3_000_000_000;
/// How many recent broadcasts to remember for echo detection.
const ECHO_MEMORY: usize = 8;
/// A beat packet byte-identical to the previous one from the same device
/// and address within this long is one broadcast received twice (two
/// interfaces on the booth network). Real beats are at least 150 ms apart
/// even at the follower's 400 BPM ceiling.
const DUPLICATE_BEAT_NS: u64 = 50_000_000;

/// The three UDP ports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProlinkPorts {
    /// Announcements and number negotiation (50000).
    pub announce: u16,
    /// Beats, precise position, on-air (50001).
    pub beat: u16,
    /// Status (50002).
    pub status: u16,
}

impl Default for ProlinkPorts {
    fn default() -> Self {
        Self {
            announce: ANNOUNCE_PORT,
            beat: BEAT_PORT,
            status: STATUS_PORT,
        }
    }
}

/// How to join (or just listen to) the booth network.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProlinkConfig {
    /// Device number to claim (default 5: player5 is the fifth device).
    /// 1..=127; if a real device has it, the source yields and listens.
    pub device_number: u8,
    /// Name announced on the network, ASCII, at most 20 bytes.
    pub name: String,
    /// Our address on the booth network. `None`: discovered from the
    /// first peer heard (by asking the OS which local address routes to
    /// it).
    pub interface: Option<Ipv4Addr>,
    /// MAC address to announce. `None`: a deterministic locally
    /// administered address derived from the interface address (see
    /// [`fallback_mac`]); `std` cannot read the real one.
    pub mac: Option<[u8; 6]>,
    /// Listen only; never send. Beats and on-air flags still arrive (they
    /// are broadcast); status, and with it the tempo master, does not.
    pub passive: bool,
    /// Destination ports, and the ports [`start`] listens on.
    pub ports: ProlinkPorts,
    /// Where to broadcast announcements. `None`: derived from the
    /// interface address (see [`default_broadcast`]).
    pub broadcast: Option<Ipv4Addr>,
    /// Local address [`start`] binds to. Must stay unspecified (0.0.0.0) to
    /// receive broadcasts; tests use 127.0.0.1.
    pub listen_address: Ipv4Addr,
    /// Which device to follow at start.
    pub follow: FollowTarget,
}

impl Default for ProlinkConfig {
    fn default() -> Self {
        Self {
            device_number: 5,
            name: "player5".to_owned(),
            interface: None,
            mac: None,
            passive: false,
            ports: ProlinkPorts::default(),
            broadcast: None,
            listen_address: Ipv4Addr::UNSPECIFIED,
            follow: FollowTarget::Master,
        }
    }
}

impl ProlinkConfig {
    /// Checks the device number and name.
    pub fn validate(&self) -> io::Result<()> {
        let invalid = |msg: String| Err(io::Error::new(io::ErrorKind::InvalidInput, msg));
        if !(1..=127).contains(&self.device_number) {
            return invalid(format!(
                "Pro DJ Link device number {} is out of range 1..=127",
                self.device_number
            ));
        }
        if self.name.is_empty()
            || self.name.len() > NAME_LEN
            || !self.name.bytes().all(|b| b.is_ascii_graphic() || b == b' ')
        {
            return invalid(format!(
                "Pro DJ Link device name {:?} must be 1 to 20 printable ASCII characters",
                self.name
            ));
        }
        Ok(())
    }
}

/// A deterministic, locally administered unicast MAC for `ip`:
/// `02:70:` followed by the four address bytes. Unique per host on the
/// subnet, but not the interface's real MAC (see "Limitations").
#[must_use]
pub fn fallback_mac(ip: Ipv4Addr) -> [u8; 6] {
    let o = ip.octets();
    [0x02, 0x70, o[0], o[1], o[2], o[3]]
}

/// The broadcast address assumed for `interface` when none is configured:
/// `169.254.255.255` for link-local addresses (the /16 every DJ Link
/// device self-assigns from), the address itself on loopback, else the /24
/// directed broadcast. Configure [`ProlinkConfig::broadcast`] for other
/// netmasks.
#[must_use]
pub fn default_broadcast(interface: Ipv4Addr) -> Ipv4Addr {
    let o = interface.octets();
    if interface.is_loopback() {
        interface
    } else if interface.is_link_local() {
        Ipv4Addr::new(169, 254, 255, 255)
    } else {
        Ipv4Addr::new(o[0], o[1], o[2], 255)
    }
}

/// Finds the local address that routes to `peer`, by connecting a UDP
/// socket (no packet is sent) and reading its local address.
pub fn discover_interface(peer: Ipv4Addr, port: u16) -> io::Result<Ipv4Addr> {
    let probe = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
    probe.connect((peer, port))?;
    match probe.local_addr()?.ip() {
        IpAddr::V4(ip) if !ip.is_unspecified() => Ok(ip),
        other => Err(io::Error::new(
            io::ErrorKind::AddrNotAvailable,
            format!("no usable local address towards {peer} (got {other})"),
        )),
    }
}

/// The three bound sockets. Bind them first ([`ProlinkSockets::bind`]) to
/// learn ephemeral ports in tests, then hand them to
/// [`start_with_sockets`].
#[derive(Debug)]
pub struct ProlinkSockets {
    announce: UdpSocket,
    beat: UdpSocket,
    status: UdpSocket,
}

impl ProlinkSockets {
    /// Binds `listen_address` on the configured ports. Fails if another
    /// program (rekordbox, another DJ Link tool) holds them: `std` cannot
    /// set `SO_REUSEADDR`.
    pub fn bind(config: &ProlinkConfig) -> io::Result<Self> {
        let addr = config.listen_address;
        let bind = |port: u16| {
            UdpSocket::bind((addr, port)).map_err(|e| {
                io::Error::new(
                    e.kind(),
                    format!(
                        "cannot listen on UDP {addr}:{port} for Pro DJ Link: {e} \
                         (is rekordbox or another DJ Link program running on this computer?)"
                    ),
                )
            })
        };
        Ok(Self {
            announce: bind(config.ports.announce)?,
            beat: bind(config.ports.beat)?,
            status: bind(config.ports.status)?,
        })
    }

    /// The ports actually bound.
    pub fn local_ports(&self) -> io::Result<ProlinkPorts> {
        Ok(ProlinkPorts {
            announce: self.announce.local_addr()?.port(),
            beat: self.beat.local_addr()?.port(),
            status: self.status.local_addr()?.port(),
        })
    }
}

/// Starts the Pro DJ Link source: binds the three ports and spawns the
/// thread. Events: [`SourceEvent::Observation`] on every beat of the
/// followed device (bar phase, effective BPM, [`Precision::Fine`]) and on
/// its tempo changes in between; [`SourceEvent::Devices`] when the device
/// list changes; [`SourceEvent::Status`] for joining, conflicts and
/// silence. Commands: [`SourceCommand::Follow`].
pub fn start(config: ProlinkConfig) -> io::Result<SourceHandle> {
    config.validate()?;
    let sockets = ProlinkSockets::bind(&config)?;
    start_with_sockets(config, sockets)
}

/// Like [`start`] with sockets bound by the caller. `config.ports` is then
/// only used as the destination of our announcements.
pub fn start_with_sockets(
    config: ProlinkConfig,
    sockets: ProlinkSockets,
) -> io::Result<SourceHandle> {
    config.validate()?;
    let listening = sockets.local_ports()?;
    let sender = sockets.status.try_clone()?;
    sender.set_broadcast(true)?;
    SourceHandle::spawn("prolink", move |ctx| {
        run(&ctx, &config, sockets, &sender, listening);
    })
}

/// A datagram as handed from a receive thread to the source thread.
#[derive(Clone, Debug)]
pub(crate) struct Received {
    pub(crate) port: Port,
    pub(crate) data: Vec<u8>,
    pub(crate) from: Ipv4Addr,
    pub(crate) at: u64,
}

fn receive(port: Port, socket: UdpSocket, tx: Sender<Received>, shutdown: Arc<AtomicBool>) {
    if socket.set_read_timeout(Some(RX_TIMEOUT)).is_err() {
        return;
    }
    let mut buf = [0u8; 2048];
    while !shutdown.load(Ordering::Relaxed) {
        match socket.recv_from(&mut buf) {
            Ok((n, from)) => {
                let at = host_time::now_ns();
                let from = match from.ip() {
                    IpAddr::V4(ip) => ip,
                    IpAddr::V6(ip) => match ip.to_ipv4_mapped() {
                        Some(ip) => ip,
                        None => continue,
                    },
                };
                let packet = Received {
                    port,
                    data: buf[..n].to_vec(),
                    from,
                    at,
                };
                if tx.send(packet).is_err() {
                    break;
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) => {}
            // e.g. an ICMP "port unreachable" surfacing on the socket: keep
            // listening, without spinning.
            Err(_) => thread::sleep(RX_TIMEOUT),
        }
    }
}

fn run(
    ctx: &SourceContext,
    config: &ProlinkConfig,
    sockets: ProlinkSockets,
    sender: &UdpSocket,
    listening: ProlinkPorts,
) {
    let shutdown = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let mut threads: Vec<JoinHandle<()>> = Vec::new();
    let ProlinkSockets {
        announce,
        beat,
        status,
    } = sockets;
    for (port, socket) in [
        (Port::Announce, announce),
        (Port::Beat, beat),
        (Port::Status, status),
    ] {
        let tx = tx.clone();
        let stop = Arc::clone(&shutdown);
        let spawned = thread::Builder::new()
            .name(format!("player5-prolink-rx-{}", port.number()))
            .spawn(move || receive(port, socket, tx, stop));
        match spawned {
            Ok(handle) => threads.push(handle),
            Err(e) => {
                let _ = ctx.send(SourceEvent::Status {
                    warning: true,
                    message: format!("cannot start the Pro DJ Link receiver: {e}"),
                });
                shutdown.store(true, Ordering::Relaxed);
                for t in threads {
                    let _ = t.join();
                }
                return;
            }
        }
    }
    drop(tx);

    let mut tracker = Tracker::new(config, listening, host_time::now_ns());
    let mut send_error_at: Option<u64> = None;
    'run: loop {
        if ctx.should_stop() {
            break;
        }
        let now = host_time::now_ns();
        while let Ok(SourceCommand::Follow(target)) = ctx.commands.try_recv() {
            tracker.follow(target, now);
        }
        let wait = tracker.deadline().map_or(POLL, |d| {
            Duration::from_nanos(d.saturating_sub(now)).min(POLL)
        });
        match rx.recv_timeout(wait) {
            Ok(first) => {
                for packet in drain_in_time_order(first, &rx) {
                    tracker.packet(&packet);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                let _ = ctx.send(SourceEvent::Status {
                    warning: true,
                    message: "Pro DJ Link receivers stopped".into(),
                });
                break;
            }
        }
        let now = host_time::now_ns();
        if let Some(peer) = tracker.wants_interface() {
            match discover_interface(peer, config.ports.announce) {
                Ok(ip) => tracker.set_interface(ip),
                Err(e) => tracker.interface_failed(peer, &e, now),
            }
        }
        tracker.tick(now);
        for output in tracker.take_output() {
            match output {
                Output::Event(event) => {
                    if !ctx.send(event) {
                        break 'run;
                    }
                }
                Output::Send { to, bytes } => {
                    let dest = SocketAddrV4::new(to, config.ports.announce);
                    if let Err(e) = sender.send_to(&bytes, dest) {
                        let quiet = send_error_at.is_some_and(|t| now.saturating_sub(t) < RETRY_NS);
                        if !quiet {
                            send_error_at = Some(now);
                            let _ = ctx.send(SourceEvent::Status {
                                warning: true,
                                message: format!(
                                    "cannot send Pro DJ Link announcement to {dest}: {e}"
                                ),
                            });
                        }
                    }
                }
            }
        }
    }
    shutdown.store(true, Ordering::Relaxed);
    for t in threads {
        let _ = t.join();
    }
}

/// `first` plus everything already queued behind it, ordered by receive
/// time. The three receive threads share one channel, so a packet stamped
/// a few microseconds earlier on one port can be queued after a later one
/// from another port; observations must leave in time order, because a
/// follower ignores reports older than the newest it has used.
pub(crate) fn drain_in_time_order(first: Received, rx: &mpsc::Receiver<Received>) -> Vec<Received> {
    let mut batch = vec![first];
    while let Ok(packet) = rx.try_recv() {
        batch.push(packet);
    }
    // Stable: packets from one socket keep their arrival order.
    batch.sort_by_key(|p| p.at);
    batch
}

/// What the tracker wants done.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Output {
    Event(SourceEvent),
    /// Send `bytes` to `to` on the announcement port.
    Send {
        to: Ipv4Addr,
        bytes: Vec<u8>,
    },
}

/// All protocol state of a running source, minus the sockets. Fed packets
/// and time; produces events and packets to send.
#[derive(Debug)]
pub(crate) struct Tracker {
    passive: bool,
    configured_mac: Option<[u8; 6]>,
    configured_broadcast: Option<Ipv4Addr>,
    interface: Option<Ipv4Addr>,
    broadcast: Option<Ipv4Addr>,
    table: DeviceTable,
    joiner: Option<Joiner>,
    target: FollowTarget,
    /// Last resolved device; outer `None` until first resolved.
    followed: Option<Option<u8>>,
    last_bpm: Option<f64>,
    started: u64,
    last_packet: Option<u64>,
    silent: bool,
    joined_at: Option<u64>,
    last_status: Option<u64>,
    status_warned: bool,
    reported: Vec<DeviceInfo>,
    reported_at: u64,
    opus_warned: bool,
    discover_from: Option<Ipv4Addr>,
    discover_after: u64,
    /// Broadcasts we sent recently (send time, bytes), to recognise their
    /// echo.
    sent: VecDeque<(u64, Vec<u8>)>,
    echo_warned: bool,
    /// Last beat packet per device (receive time, source, bytes), to drop
    /// the second copy of a broadcast received on two interfaces.
    last_beats: BTreeMap<u8, (u64, Ipv4Addr, Vec<u8>)>,
    duplicate_warned: bool,
    out: Vec<Output>,
}

impl Tracker {
    pub(crate) fn new(config: &ProlinkConfig, listening: ProlinkPorts, now: u64) -> Self {
        let mut t = Self {
            passive: config.passive,
            configured_mac: config.mac,
            configured_broadcast: config.broadcast,
            interface: None,
            broadcast: None,
            table: DeviceTable::default(),
            joiner: (!config.passive).then(|| Joiner::new(&config.name, config.device_number, now)),
            target: config.follow,
            followed: None,
            last_bpm: None,
            started: now,
            last_packet: None,
            silent: false,
            joined_at: None,
            last_status: None,
            status_warned: false,
            reported: Vec::new(),
            reported_at: now,
            opus_warned: false,
            discover_from: None,
            discover_after: now,
            sent: VecDeque::new(),
            echo_warned: false,
            last_beats: BTreeMap::new(),
            duplicate_warned: false,
            out: Vec::new(),
        };
        let ports = format!(
            "UDP {}/{}/{}",
            listening.announce, listening.beat, listening.status
        );
        let message = if config.passive {
            let mut m = format!("Pro DJ Link: listening passively on {ports}");
            if config.follow == FollowTarget::Master {
                m.push_str(
                    "; without joining, the tempo master is unknown, \
                     so the lowest-numbered playing player is followed",
                );
            }
            m
        } else {
            format!(
                "Pro DJ Link: listening on {ports}; will join as device {} \"{}\"",
                config.device_number, config.name
            )
        };
        t.status(false, message);
        if let Some(ip) = config.interface {
            t.set_interface(ip);
        }
        t
    }

    fn status(&mut self, warning: bool, message: String) {
        self.out
            .push(Output::Event(SourceEvent::Status { warning, message }));
    }

    pub(crate) fn take_output(&mut self) -> Vec<Output> {
        std::mem::take(&mut self.out)
    }

    pub(crate) fn deadline(&self) -> Option<u64> {
        self.joiner.as_ref().and_then(Joiner::deadline)
    }

    /// A peer to discover our interface address from, once.
    pub(crate) fn wants_interface(&mut self) -> Option<Ipv4Addr> {
        self.discover_from.take()
    }

    pub(crate) fn interface_failed(&mut self, peer: Ipv4Addr, e: &io::Error, now: u64) {
        self.discover_after = now + RETRY_NS;
        self.status(
            true,
            format!("cannot find a local address towards {peer}: {e}; set the interface address"),
        );
    }

    pub(crate) fn set_interface(&mut self, ip: Ipv4Addr) {
        if self.passive {
            return;
        }
        let broadcast = self
            .configured_broadcast
            .unwrap_or_else(|| default_broadcast(ip));
        let mac = self.configured_mac.unwrap_or_else(|| fallback_mac(ip));
        self.interface = Some(ip);
        self.broadcast = Some(broadcast);
        if let Some(j) = self.joiner.as_mut() {
            j.set_interface(ip, mac);
        }
        self.status(
            false,
            format!("Pro DJ Link: using interface {ip}, broadcasting to {broadcast}"),
        );
    }

    /// Whether an announcement is our own, come back to us. Recognised by
    /// content first: a host on the booth network through two interfaces
    /// (or with a wrong `interface` setting) sees its broadcasts come back
    /// from another address, and mistaking that echo for a device holding
    /// our number would make us give the number up to ourselves.
    fn is_own_announcement(&mut self, data: &[u8], from: Ipv4Addr, at: u64) -> bool {
        let echo = self
            .sent
            .iter()
            .any(|(t, bytes)| at.saturating_sub(*t) <= ECHO_WINDOW_NS && bytes.as_slice() == data);
        if echo {
            if let Some(ip) = self.interface.filter(|ip| *ip != from) {
                if !self.echo_warned {
                    self.echo_warned = true;
                    self.status(
                        true,
                        format!(
                            "Pro DJ Link: our own announcements come back from {from}, not from \
                             {ip}; this computer may reach the booth network through more than \
                             one interface. Set the interface and broadcast addresses of the \
                             booth network"
                        ),
                    );
                }
            }
            return true;
        }
        let name = device_name(Port::Announce, data);
        self.joiner.as_ref().is_some_and(|j| j.is_self(&name, from))
    }

    /// Whether a beat packet is the second copy of one broadcast, received
    /// through a second interface. Remembers the packet otherwise.
    fn duplicate_beat(&mut self, device: u8, data: &[u8], from: Ipv4Addr, at: u64) -> bool {
        let duplicate = self.last_beats.get(&device).is_some_and(|(t, src, bytes)| {
            *src == from && at.saturating_sub(*t) <= DUPLICATE_BEAT_NS && bytes.as_slice() == data
        });
        if duplicate {
            if !self.duplicate_warned {
                self.duplicate_warned = true;
                self.status(
                    true,
                    "Pro DJ Link: every beat arrives twice; this computer is probably on the \
                     booth network through two interfaces (e.g. Ethernet and Wi-Fi). Copies are \
                     dropped, but disconnect one of them"
                        .into(),
                );
            }
            return true;
        }
        self.last_beats.insert(device, (at, from, data.to_vec()));
        false
    }

    /// Remembers a broadcast for echo detection.
    fn remember_sent(&mut self, bytes: &[u8], now: u64) {
        self.sent
            .retain(|(t, _)| now.saturating_sub(*t) <= ECHO_WINDOW_NS);
        if self.sent.len() >= ECHO_MEMORY {
            self.sent.pop_front();
        }
        self.sent.push_back((now, bytes.to_vec()));
    }

    fn saw_traffic(&mut self, from: Ipv4Addr, at: u64) {
        self.last_packet = Some(at);
        if self.silent {
            self.silent = false;
            self.status(false, "Pro DJ Link: receiving traffic again".into());
        }
        if self.joiner.is_some()
            && self.interface.is_none()
            && self.discover_from.is_none()
            && at >= self.discover_after
            && !from.is_unspecified()
        {
            self.discover_from = Some(from);
        }
    }

    fn apply(&mut self, actions: Vec<JoinAction>, now: u64) {
        for action in actions {
            match action {
                JoinAction::Send(Outgoing::Broadcast(bytes)) => {
                    if let Some(to) = self.broadcast {
                        self.remember_sent(&bytes, now);
                        self.out.push(Output::Send { to, bytes });
                    }
                }
                JoinAction::Send(Outgoing::Unicast(to, bytes)) => {
                    self.out.push(Output::Send { to, bytes });
                }
                JoinAction::Event(JoinEvent::Claiming(n)) => {
                    self.status(false, format!("Pro DJ Link: claiming device number {n}"));
                }
                JoinAction::Event(JoinEvent::Joined(n)) => {
                    self.joined_at = Some(now);
                    self.status(
                        false,
                        format!("Pro DJ Link: joined the network as device {n}"),
                    );
                }
                JoinAction::Event(JoinEvent::Assigned { number, by }) => {
                    self.status(
                        false,
                        format!("Pro DJ Link: the mixer at {by} assigned device number {number}"),
                    );
                }
                JoinAction::Event(JoinEvent::Yielded { number, by, name }) => {
                    self.status(
                        true,
                        format!(
                            "Pro DJ Link: device number {number} belongs to {name} at {by}; \
                             player5 gave it up and only listens now (choose another device \
                             number to see the tempo master)"
                        ),
                    );
                }
            }
        }
    }

    pub(crate) fn follow(&mut self, target: FollowTarget, now: u64) {
        self.target = target;
        let message = match target {
            FollowTarget::Master => "Pro DJ Link: following the tempo master".to_owned(),
            FollowTarget::Device(n) => format!("Pro DJ Link: following device {n}"),
        };
        self.status(false, message);
        self.followed = None;
        self.refollow(now);
    }

    fn refollow(&mut self, now: u64) {
        let resolved = self.table.resolve(self.target, now);
        if self.followed == Some(resolved) {
            return;
        }
        self.followed = Some(resolved);
        self.last_bpm = None;
        let message = match resolved {
            Some(n) => {
                let name = self.table.name(n).unwrap_or("not seen yet");
                let why = match self.target {
                    FollowTarget::Device(_) => "as requested",
                    FollowTarget::Master if self.table.master(now) == Some(n) => "tempo master",
                    FollowTarget::Master => "lowest-numbered playing player",
                };
                format!("Pro DJ Link: tracking device {n} ({name}), {why}")
            }
            None => {
                "Pro DJ Link: nothing to follow yet (no tempo master, no player playing)".into()
            }
        };
        self.status(false, message);
    }

    fn following(&self, device: u8) -> bool {
        self.followed == Some(Some(device))
    }

    fn tempo_update(&mut self, device: u8, bpm: Option<f64>, at: u64) {
        let Some(bpm) = bpm else { return };
        if !self.following(device) {
            return;
        }
        if self
            .last_bpm
            .is_some_and(|b| (b - bpm).abs() <= TEMPO_EPSILON)
        {
            return;
        }
        self.last_bpm = Some(bpm);
        self.out.push(Output::Event(SourceEvent::Observation {
            host_ns: at,
            phase: Phase::TempoOnly,
            bpm: Some(bpm),
            precision: Precision::Fine,
            device: Some(device),
        }));
    }

    pub(crate) fn packet(&mut self, rx: &Received) {
        let Some(kind) = PacketKind::classify(rx.port, &rx.data) else {
            return;
        };
        let (data, from, at) = (rx.data.as_slice(), rx.from, rx.at);
        if rx.port == Port::Announce && self.is_own_announcement(data, from, at) {
            return;
        }
        self.saw_traffic(from, at);
        match kind {
            PacketKind::KeepAlive => {
                let Ok(k) = parse_keep_alive(data) else {
                    return;
                };
                if k.name == OPUS_QUAD_NAME && !self.opus_warned {
                    self.opus_warned = true;
                    self.status(
                        true,
                        "Pro DJ Link: an Opus Quad is on the network; it does not send \
                         beats to DJ Link devices, use the opus source to follow it"
                            .into(),
                    );
                }
                self.table.keep_alive(&k, from, at);
                if let Some(j) = self.joiner.as_mut() {
                    let actions = j.on_number_seen(k.number, from, &k.name);
                    self.apply(actions, at);
                }
            }
            PacketKind::ClaimStage2 | PacketKind::ClaimStage3 => {
                let Ok(c) = parse_number_claim(data) else {
                    return;
                };
                if let (Some(n), false, Some(j)) =
                    (c.number, c.assignment_request, self.joiner.as_mut())
                {
                    let actions = j.on_number_seen(n, from, &c.name);
                    self.apply(actions, at);
                }
            }
            PacketKind::NumberInUse => {
                let Ok(n) = parse_number_in_use(data) else {
                    return;
                };
                if let Some(j) = self.joiner.as_mut() {
                    let actions = j.on_number_in_use(n.number, from, &n.name);
                    self.apply(actions, at);
                }
            }
            PacketKind::AssignmentIntention => {
                if let Some(j) = self.joiner.as_mut() {
                    let actions = j.on_assignment_intention(from);
                    self.apply(actions, at);
                }
            }
            PacketKind::Assignment => {
                let Ok(number) = parse_assignment(data) else {
                    return;
                };
                if let Some(j) = self.joiner.as_mut() {
                    let actions = j.on_assignment(number, from, at);
                    self.apply(actions, at);
                }
            }
            PacketKind::AssignmentFinished => {
                if let Some(j) = self.joiner.as_mut() {
                    j.on_assignment_finished(at);
                }
            }
            PacketKind::Beat => {
                let Ok(b) = parse_beat(data) else { return };
                if self.duplicate_beat(b.device, data, from, at) {
                    return;
                }
                self.table.beat(&b, from, at);
                self.refollow(at);
                if self.following(b.device) {
                    let phase = if self.table.bar_meaningful(b.device, b.beat_within_bar, at) {
                        Phase::Bar(f64::from(b.beat_within_bar - 1))
                    } else {
                        Phase::Beat(0.0)
                    };
                    let bpm = b.effective_bpm();
                    if bpm.is_some() {
                        self.last_bpm = bpm;
                    }
                    self.out.push(Output::Event(SourceEvent::Observation {
                        host_ns: at,
                        phase,
                        bpm,
                        precision: Precision::Fine,
                        device: Some(b.device),
                    }));
                }
            }
            PacketKind::PrecisePosition => {
                let Ok(p) = parse_precise_position(data) else {
                    return;
                };
                self.table.precise_position(&p, from, at);
            }
            PacketKind::ChannelsOnAir => {
                let Ok(o) = parse_on_air(data) else { return };
                self.table.on_air(&o);
            }
            PacketKind::CdjStatus => {
                let Ok(s) = parse_cdj_status(data) else {
                    return;
                };
                self.last_status = Some(at);
                self.table.cdj_status(&s, from, at);
                self.refollow(at);
                if s.playing() {
                    self.tempo_update(s.device, s.effective_bpm(), at);
                }
            }
            PacketKind::MixerStatus => {
                let Ok(m) = parse_mixer_status(data) else {
                    return;
                };
                self.last_status = Some(at);
                self.table.mixer_status(&m, from, at);
                self.refollow(at);
                self.tempo_update(m.device, m.effective_bpm(), at);
            }
            _ => {}
        }
        self.report_devices(at);
    }

    pub(crate) fn tick(&mut self, now: u64) {
        let peers = self.table.len();
        if let Some(j) = self.joiner.as_mut() {
            let actions = j.poll(now, peers);
            self.apply(actions, now);
        }
        self.table.expire(now);
        self.refollow(now);
        let last = self.last_packet.unwrap_or(self.started);
        if !self.silent && now.saturating_sub(last) > NO_TRAFFIC_NS {
            self.silent = true;
            let message = if self.last_packet.is_some() {
                "Pro DJ Link: no traffic for 5 s; the booth network went quiet"
            } else {
                "Pro DJ Link: no traffic seen; check that this computer is on the booth \
                 network (wired, same subnet as the players)"
            };
            self.status(true, message.into());
        }
        self.check_status_flow(now);
        self.report_devices(now);
    }

    /// Joined, players present, yet no status: they cannot reach us.
    fn check_status_flow(&mut self, now: u64) {
        let Some(joined) = self.joined_at else { return };
        if self.status_warned || self.joiner.as_ref().is_some_and(Joiner::has_yielded) {
            return;
        }
        let players = self
            .table
            .snapshot(now)
            .iter()
            .any(|d| d.kind == DeviceKind::Player);
        let fresh = self
            .last_status
            .is_some_and(|t| now.saturating_sub(t) <= STATUS_FRESH_NS + NO_STATUS_NS);
        if players && !fresh && now.saturating_sub(joined) > NO_STATUS_NS {
            self.status_warned = true;
            let ip = self
                .interface
                .map_or_else(|| "?".to_owned(), |ip| ip.to_string());
            self.status(
                true,
                format!(
                    "Pro DJ Link: joined, but no status packets arrive; players may not reach \
                     {ip} on UDP 50002 (firewall, or a second interface on the same network)"
                ),
            );
        }
    }

    fn report_devices(&mut self, now: u64) {
        let snapshot = self.table.snapshot(now);
        let changed = differs(&snapshot, &self.reported, 0.05)
            || (snapshot != self.reported
                && now.saturating_sub(self.reported_at) >= DEVICES_MIN_INTERVAL_NS);
        if changed {
            self.reported_at = now;
            self.reported.clone_from(&snapshot);
            self.out.push(Output::Event(SourceEvent::Devices(snapshot)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::packets::{
        build_beat_packet, build_cdj_status, build_keep_alive, build_mixer_status,
        build_number_in_use, parse_keep_alive, BeatPacket, CdjStatus, KeepAlive, MixerStatus,
    };
    use super::*;

    const S: u64 = 1_000_000_000;
    const MS: u64 = 1_000_000;

    fn ip(n: u8) -> Ipv4Addr {
        Ipv4Addr::new(169, 254, 0, n)
    }

    fn rx(port: Port, data: Vec<u8>, from: Ipv4Addr, at: u64) -> Received {
        Received {
            port,
            data,
            from,
            at,
        }
    }

    fn keep_alive(number: u8, name: &str, device_type: u8) -> Vec<u8> {
        build_keep_alive(&KeepAlive {
            number,
            name: name.into(),
            mac: [0x74, 0x5e, 0x1c, 0, 0, number],
            ip: ip(number),
            peers: 2,
            device_type,
            first_on_network: false,
            cdj3000_compatible: true,
        })
    }

    fn events(t: &mut Tracker) -> Vec<SourceEvent> {
        t.take_output()
            .into_iter()
            .filter_map(|o| match o {
                Output::Event(e) => Some(e),
                Output::Send { .. } => None,
            })
            .collect()
    }

    fn observations(events: &[SourceEvent]) -> Vec<(u64, Phase, Option<f64>, Option<u8>)> {
        events
            .iter()
            .filter_map(|e| match e {
                SourceEvent::Observation {
                    host_ns,
                    phase,
                    bpm,
                    device,
                    precision,
                } => {
                    assert_eq!(*precision, Precision::Fine);
                    Some((*host_ns, *phase, *bpm, *device))
                }
                _ => None,
            })
            .collect()
    }

    fn passive() -> Tracker {
        let config = ProlinkConfig {
            passive: true,
            ..ProlinkConfig::default()
        };
        Tracker::new(&config, ProlinkPorts::default(), 0)
    }

    #[test]
    fn passive_follows_lowest_playing_player_with_bar_phase() {
        let mut t = passive();
        let start = events(&mut t);
        assert!(
            matches!(&start[0], SourceEvent::Status { warning: false, message } if message.contains("passively"))
        );
        t.packet(&rx(Port::Announce, keep_alive(2, "CDJ-3000", 1), ip(2), S));
        t.packet(&rx(Port::Announce, keep_alive(3, "CDJ-3000", 1), ip(3), S));
        let ev = events(&mut t);
        assert!(ev
            .iter()
            .any(|e| matches!(e, SourceEvent::Devices(d) if d.len() == 2)));
        // Device 3 plays first: it is followed.
        let beat3 = build_beat_packet(&BeatPacket::new(3, 128.0, 0.0, 2));
        t.packet(&rx(Port::Beat, beat3.clone(), ip(3), 2 * S));
        let obs = observations(&events(&mut t));
        assert_eq!(obs, vec![(2 * S, Phase::Bar(1.0), Some(128.0), Some(3))]);
        // Device 2 starts too: lower number wins from its first beat.
        let beat2 = build_beat_packet(&BeatPacket::new(2, 120.0, 2.0, 4));
        t.packet(&rx(Port::Beat, beat2, ip(2), 2 * S + 100 * MS));
        let obs = observations(&events(&mut t));
        assert_eq!(obs.len(), 1);
        assert_eq!(obs[0].1, Phase::Bar(3.0));
        assert!((obs[0].2.unwrap() - 122.4).abs() < 0.01);
        // Beats from 3 are no longer reported.
        t.packet(&rx(Port::Beat, beat3, ip(3), 2 * S + 200 * MS));
        assert!(observations(&events(&mut t)).is_empty());
        // Explicit follow target.
        t.follow(FollowTarget::Device(3), 2 * S + 300 * MS);
        let beat3 = build_beat_packet(&BeatPacket::new(3, 128.0, 0.0, 3));
        t.packet(&rx(Port::Beat, beat3, ip(3), 2 * S + 400 * MS));
        let obs = observations(&events(&mut t));
        assert_eq!(
            obs,
            vec![(2 * S + 400 * MS, Phase::Bar(2.0), Some(128.0), Some(3))]
        );
        // Passive never sends.
        t.tick(20 * S);
        assert!(t
            .take_output()
            .iter()
            .all(|o| matches!(o, Output::Event(_))));
    }

    #[test]
    fn mixer_beats_have_no_bar_phase() {
        let mut t = passive();
        t.packet(&rx(
            Port::Announce,
            keep_alive(0x21, "DJM-V10", 2),
            ip(0x21),
            0,
        ));
        t.follow(FollowTarget::Device(0x21), 0);
        let _ = events(&mut t);
        let beat = build_beat_packet(&BeatPacket::new(0x21, 125.0, 0.0, 3));
        t.packet(&rx(Port::Beat, beat, ip(0x21), S));
        let obs = observations(&events(&mut t));
        assert_eq!(obs, vec![(S, Phase::Beat(0.0), Some(125.0), Some(0x21))]);
    }

    #[test]
    fn status_identifies_the_master_and_tempo_changes() {
        let mut t = passive();
        t.packet(&rx(
            Port::Status,
            build_cdj_status(&CdjStatus::new(2, 126.0, 0.0, true, true)),
            ip(2),
            S,
        ));
        t.packet(&rx(
            Port::Status,
            build_cdj_status(&CdjStatus::new(1, 120.0, 0.0, true, false)),
            ip(1),
            S,
        ));
        let ev = events(&mut t);
        assert!(ev.iter().any(|e| matches!(e, SourceEvent::Status { message, .. } if message.contains("device 2") && message.contains("tempo master"))));
        // Tempo-only observation from the master's status.
        let obs = observations(&ev);
        assert_eq!(obs, vec![(S, Phase::TempoOnly, Some(126.0), Some(2))]);
        // Same tempo again: nothing new. Pitch moved: a new observation.
        t.packet(&rx(
            Port::Status,
            build_cdj_status(&CdjStatus::new(2, 126.0, 0.0, true, true)),
            ip(2),
            S + 200 * MS,
        ));
        assert!(observations(&events(&mut t)).is_empty());
        t.packet(&rx(
            Port::Status,
            build_cdj_status(&CdjStatus::new(2, 126.0, 1.0, true, true)),
            ip(2),
            S + 400 * MS,
        ));
        let obs = observations(&events(&mut t));
        assert_eq!(obs.len(), 1);
        assert!((obs[0].2.unwrap() - 127.26).abs() < 0.01);
        // Beats of the non-master are ignored, the master's are reported.
        t.packet(&rx(
            Port::Beat,
            build_beat_packet(&BeatPacket::new(1, 120.0, 0.0, 1)),
            ip(1),
            S + 500 * MS,
        ));
        assert!(observations(&events(&mut t)).is_empty());
        t.packet(&rx(
            Port::Beat,
            build_beat_packet(&BeatPacket::new(2, 126.0, 1.0, 1)),
            ip(2),
            S + 600 * MS,
        ));
        assert_eq!(observations(&events(&mut t))[0].1, Phase::Bar(0.0));
        // A mixer becoming master takes over.
        let mixer = MixerStatus {
            device: 0x21,
            name: "DJM-V10".into(),
            flags: 0xf0,
            pitch: 0x10_0000,
            bpm_x100: 12_600,
            master_handoff: 0xff,
            beat_within_bar: 2,
        };
        let mut two = CdjStatus::new(2, 126.0, 1.0, true, false);
        two.flags &= !0x20;
        t.packet(&rx(Port::Status, build_cdj_status(&two), ip(2), 2 * S));
        t.packet(&rx(
            Port::Status,
            build_mixer_status(&mixer),
            ip(0x21),
            2 * S,
        ));
        let ev = events(&mut t);
        assert!(ev.iter().any(
            |e| matches!(e, SourceEvent::Status { message, .. } if message.contains("device 33"))
        ));
    }

    #[test]
    fn joins_and_yields_on_conflict() {
        let config = ProlinkConfig {
            interface: Some(ip(50)),
            ..ProlinkConfig::default()
        };
        let mut t = Tracker::new(&config, ProlinkPorts::default(), 0);
        let mut sent = Vec::new();
        let mut now = 0;
        while now < 5 * S {
            t.tick(now);
            for o in t.take_output() {
                if let Output::Send { to, bytes } = o {
                    sent.push((to, bytes));
                }
            }
            now += 10 * MS;
        }
        assert_eq!(sent.len(), 13, "3 hellos, 3x3 claims, a keep-alive");
        assert!(sent
            .iter()
            .all(|(to, _)| *to == Ipv4Addr::new(169, 254, 255, 255)));
        let k = parse_keep_alive(&sent[12].1).unwrap();
        assert_eq!((k.number, k.ip, k.mac), (5, ip(50), fallback_mac(ip(50))));
        // Our own keep-alive echoed back is not a device.
        t.packet(&rx(Port::Announce, sent[12].1.clone(), ip(50), now));
        assert!(!t.table.contains(5));
        // A CDJ-3000 defends number 5: we yield, warn, and go quiet.
        t.packet(&rx(
            Port::Announce,
            build_number_in_use("CDJ-3000", 5, ip(5)),
            ip(5),
            now,
        ));
        let ev = events(&mut t);
        assert!(ev.iter().any(|e| matches!(e, SourceEvent::Status { warning: true, message } if message.contains("CDJ-3000"))));
        t.tick(now + 10 * S);
        assert!(t
            .take_output()
            .iter()
            .all(|o| matches!(o, Output::Event(_))));
    }

    /// Runs an active tracker until it has joined; returns it, the time and
    /// everything it sent.
    fn joined(interface: Ipv4Addr) -> (Tracker, u64, Vec<Vec<u8>>) {
        let config = ProlinkConfig {
            interface: Some(interface),
            ..ProlinkConfig::default()
        };
        let mut t = Tracker::new(&config, ProlinkPorts::default(), 0);
        let mut sent = Vec::new();
        let mut now = 0;
        while now < 5 * S {
            t.tick(now);
            for o in t.take_output() {
                if let Output::Send { bytes, .. } = o {
                    sent.push(bytes);
                }
            }
            now += 10 * MS;
        }
        (t, now, sent)
    }

    #[test]
    fn own_echo_from_another_address_is_not_a_conflict() {
        // Two interfaces on the booth network: our broadcasts come back
        // with the other interface's source address. Before echo detection
        // by content, player5 gave its number up to itself.
        let (mut t, now, sent) = joined(ip(50));
        let keep_alive = sent.last().unwrap().clone();
        assert_eq!(parse_keep_alive(&keep_alive).unwrap().number, 5);
        let other_nic = Ipv4Addr::new(169, 254, 77, 1);
        for i in 0..3 {
            t.packet(&rx(
                Port::Announce,
                keep_alive.clone(),
                other_nic,
                now + i * MS,
            ));
        }
        assert!(!t.joiner.as_ref().unwrap().has_yielded());
        assert!(!t.table.contains(5));
        let warnings: Vec<String> = events(&mut t)
            .into_iter()
            .filter_map(|e| match e {
                SourceEvent::Status {
                    warning: true,
                    message,
                } => Some(message),
                _ => None,
            })
            .collect();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("come back from 169.254.77.1"));
        // Keep-alives keep going out.
        t.tick(now + 2 * S);
        assert!(t
            .take_output()
            .iter()
            .any(|o| matches!(o, Output::Send { .. })));
        // A different machine also calling itself player5 and holding
        // number 5 is not an echo: real devices still win.
        let impostor = build_keep_alive(&KeepAlive {
            number: 5,
            name: "player5".into(),
            mac: fallback_mac(ip(60)),
            ip: ip(60),
            peers: 2,
            device_type: 1,
            first_on_network: false,
            cdj3000_compatible: true,
        });
        t.packet(&rx(Port::Announce, impostor, ip(60), now + 3 * S));
        assert!(t.joiner.as_ref().unwrap().has_yielded());
    }

    #[test]
    fn duplicate_beats_from_two_interfaces_are_dropped() {
        let mut t = passive();
        let _ = events(&mut t);
        let beat = build_beat_packet(&BeatPacket::new(1, 120.0, 0.0, 1));
        t.packet(&rx(Port::Beat, beat.clone(), ip(1), S));
        t.packet(&rx(Port::Beat, beat.clone(), ip(1), S + 300_000));
        let ev = events(&mut t);
        assert_eq!(observations(&ev).len(), 1, "one beat, one observation");
        assert!(ev.iter().any(
            |e| matches!(e, SourceEvent::Status { warning: true, message } if message.contains("twice"))
        ));
        // The next real beat is reported, and the beat interval learned
        // from the copies did not collapse to a fraction of a millisecond.
        let next = build_beat_packet(&BeatPacket::new(1, 120.0, 0.0, 2));
        t.packet(&rx(Port::Beat, next.clone(), ip(1), S + 500 * MS));
        t.packet(&rx(Port::Beat, next, ip(1), S + 500 * MS + 200_000));
        let obs = observations(&events(&mut t));
        assert_eq!(
            obs,
            vec![(S + 500 * MS, Phase::Bar(1.0), Some(120.0), Some(1))]
        );
        assert_eq!(t.table.playing(1, S + 2400 * MS), Some(true));
        // Identical bytes long after are a real beat (e.g. a loop), not a
        // copy; so is the same packet from another device's address.
        t.packet(&rx(Port::Beat, beat.clone(), ip(1), 3 * S));
        t.packet(&rx(Port::Beat, beat, ip(9), 3 * S + 100_000));
        assert_eq!(observations(&events(&mut t)).len(), 2);
    }

    #[test]
    fn batches_are_handed_over_in_receive_order() {
        let (tx, rxq) = mpsc::channel();
        let packet = |port, at| rx(port, vec![at as u8], ip(1), at);
        // The status thread queued its packet before the beat thread, but
        // received it later.
        tx.send(packet(Port::Status, 30)).unwrap();
        tx.send(packet(Port::Beat, 10)).unwrap();
        tx.send(packet(Port::Announce, 20)).unwrap();
        tx.send(packet(Port::Beat, 20)).unwrap();
        let first = rxq.recv().unwrap();
        let order: Vec<(u64, Port)> = drain_in_time_order(first, &rxq)
            .iter()
            .map(|p| (p.at, p.port))
            .collect();
        assert_eq!(
            order,
            vec![
                (10, Port::Beat),
                (20, Port::Announce),
                (20, Port::Beat),
                (30, Port::Status)
            ]
        );
    }

    #[test]
    fn observations_leave_in_time_order_across_ports() {
        // A tempo change in status received just after a beat must not be
        // reported before it: a follower drops reports older than the
        // newest it has used, and would lose the beat.
        let mut t = passive();
        t.packet(&rx(
            Port::Status,
            build_cdj_status(&CdjStatus::new(2, 126.0, 0.0, true, true)),
            ip(2),
            S,
        ));
        let _ = events(&mut t);
        let (tx, rxq) = mpsc::channel();
        tx.send(rx(
            Port::Status,
            build_cdj_status(&CdjStatus::new(2, 126.0, 2.0, true, true)),
            ip(2),
            S + 100 * MS + 5_000,
        ))
        .unwrap();
        tx.send(rx(
            Port::Beat,
            build_beat_packet(&BeatPacket::new(2, 126.0, 0.0, 3)),
            ip(2),
            S + 100 * MS,
        ))
        .unwrap();
        let first = rxq.recv().unwrap();
        for p in drain_in_time_order(first, &rxq) {
            t.packet(&p);
        }
        let obs = observations(&events(&mut t));
        assert_eq!(obs.len(), 2);
        assert!(obs.windows(2).all(|w| w[0].0 < w[1].0), "{obs:?}");
        assert_eq!(obs[0].1, Phase::Bar(2.0));
        assert_eq!(obs[1].1, Phase::TempoOnly);
    }

    #[test]
    fn discovers_the_interface_from_the_first_peer() {
        let mut t = Tracker::new(&ProlinkConfig::default(), ProlinkPorts::default(), 0);
        assert_eq!(t.wants_interface(), None);
        t.packet(&rx(Port::Announce, keep_alive(1, "CDJ-3000", 1), ip(1), S));
        assert_eq!(t.wants_interface(), Some(ip(1)));
        assert_eq!(t.wants_interface(), None);
        t.set_interface(ip(9));
        assert_eq!(t.broadcast, Some(Ipv4Addr::new(169, 254, 255, 255)));
    }

    #[test]
    fn warns_about_silence_and_recovery() {
        let mut t = passive();
        let _ = events(&mut t);
        t.tick(6 * S);
        let ev = events(&mut t);
        assert!(ev
            .iter()
            .any(|e| matches!(e, SourceEvent::Status { warning: true, .. })));
        t.packet(&rx(
            Port::Announce,
            keep_alive(1, "CDJ-3000", 1),
            ip(1),
            7 * S,
        ));
        let ev = events(&mut t);
        assert!(ev.iter().any(|e| matches!(e, SourceEvent::Status { warning: false, message } if message.contains("again"))));
    }

    #[test]
    fn config_validation_and_defaults() {
        let c = ProlinkConfig::default();
        assert_eq!(
            (c.device_number, c.name.as_str(), c.passive),
            (5, "player5", false)
        );
        assert_eq!(
            c.ports,
            ProlinkPorts {
                announce: 50000,
                beat: 50001,
                status: 50002
            }
        );
        assert!(c.validate().is_ok());
        assert!(ProlinkConfig {
            device_number: 0,
            ..c.clone()
        }
        .validate()
        .is_err());
        assert!(ProlinkConfig {
            name: "x".repeat(21),
            ..c.clone()
        }
        .validate()
        .is_err());
        assert!(ProlinkConfig {
            name: "pläyer".into(),
            ..c
        }
        .validate()
        .is_err());
        assert_eq!(
            default_broadcast(Ipv4Addr::new(169, 254, 3, 4)),
            Ipv4Addr::new(169, 254, 255, 255)
        );
        assert_eq!(
            default_broadcast(Ipv4Addr::new(192, 168, 1, 20)),
            Ipv4Addr::new(192, 168, 1, 255)
        );
        assert_eq!(default_broadcast(Ipv4Addr::LOCALHOST), Ipv4Addr::LOCALHOST);
        let mac = fallback_mac(Ipv4Addr::new(10, 0, 0, 7));
        assert_eq!(mac, [0x02, 0x70, 10, 0, 0, 7]);
        assert_eq!(mac[0] & 0x03, 0x02, "locally administered, unicast");
    }
}
