//! Opus Quad mode: player5 poses as rekordbox in lighting mode, the one
//! role in which an Opus Quad reports its decks on the network, and turns
//! the decks' beat counters into coarse clock observations.
//!
//! The protocol, every offset and the reasons for each choice are in
//! `docs/protocols/opus-quad.md` (one source link per fact); code comments
//! only point at its sections.
//!
//! - [`packets`]: pure parsing and building of the packets involved.
//! - [`tracker`]: beat-start estimation from ~200 ms status packets.
//! - [`session`]: the source as a pure state machine (deck table, follow
//!   target, announcements).
//! - [`start`]: runs it on a background thread as a
//!   [`SourceHandle`].
//!
//! The unit reports beat number, beat within the bar, tempo, pitch, play
//! state and the tempo-master flag per deck, but only in status packets
//! about every 200 ms and with no beat packets, so phase is only good to
//! about ±200 ms before interpolation. Observations are therefore
//! [`Precision::Coarse`](crate::Precision::Coarse).
//!
//! Std only, no `unsafe`. Nothing here runs on an audio thread.

pub mod packets;
pub mod session;
pub mod source;
pub mod tracker;

#[cfg(test)]
mod tests;

use std::net::{Ipv4Addr, UdpSocket};
use std::time::Duration;

use crate::net::{FollowTarget, SourceHandle};
pub use packets::{
    lighting_request, opus_deck, parse_announce, parse_update, AnnouncePacket, DeckStatus,
    KeepAlive, LightingHello, ParseError, UpdatePacket,
};
pub use tracker::{BeatEstimate, BeatTracker};

/// Fastest keep-alive cadence accepted (beat-link's lower bound).
pub const MIN_ANNOUNCE_INTERVAL: Duration = Duration::from_millis(200);
/// Slowest keep-alive cadence accepted (beat-link's upper bound).
pub const MAX_ANNOUNCE_INTERVAL: Duration = Duration::from_millis(2000);
/// Default keep-alive cadence (beat-link's default; opus-quad.md,
/// "Joining as rekordbox lighting").
pub const DEFAULT_ANNOUNCE_INTERVAL: Duration = Duration::from_millis(1500);

/// Local and remote UDP ports. The protocol fixes them at 50000/50002;
/// tests override them to run on loopback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpusPorts {
    /// Local address both sockets bind to. Keep the default
    /// (`0.0.0.0`): a socket bound to a unicast address does not receive
    /// broadcasts on every platform.
    pub bind: Ipv4Addr,
    /// Local port receiving keep-alives (`0` = ephemeral).
    pub announce: u16,
    /// Local port receiving status packets (`0` = ephemeral).
    pub update: u16,
    /// Port our keep-alives are sent to.
    pub peer_announce: u16,
    /// Port on the unit our lighting requests are sent to.
    pub peer_update: u16,
}

impl Default for OpusPorts {
    fn default() -> Self {
        Self {
            bind: Ipv4Addr::UNSPECIFIED,
            announce: packets::ANNOUNCE_PORT,
            update: packets::UPDATE_PORT,
            peer_announce: packets::ANNOUNCE_PORT,
            peer_update: packets::UPDATE_PORT,
        }
    }
}

/// How to run the Opus Quad source.
#[derive(Clone, Debug, PartialEq)]
pub struct OpusConfig {
    /// Our IPv4 address on the booth network. `None`: found once the unit
    /// is seen, as the local address of a UDP socket connected toward it.
    pub interface: Option<Ipv4Addr>,
    /// MAC address to announce. `None`: a locally administered address
    /// derived from the interface address, `02:50:a:b:c:d`
    /// (opus-quad.md, "player5 policies"). Pass the real one when known.
    pub mac: Option<[u8; 6]>,
    /// Device number to announce (default `0x17`, what rekordbox lighting
    /// uses); replaced from `0x13..=0x27` if another device takes it.
    pub device_number: u8,
    /// Computer name sent in the lighting request (ASCII; the unit does
    /// not seem to care).
    pub computer_name: String,
    /// Keep-alive and lighting-request cadence, clamped to
    /// [`MIN_ANNOUNCE_INTERVAL`]..=[`MAX_ANNOUNCE_INTERVAL`].
    pub announce_interval: Duration,
    /// Where keep-alives are sent. `None`: `169.254.255.255` on a
    /// link-local network, else the /24 broadcast address of the
    /// interface.
    pub broadcast: Option<Ipv4Addr>,
    /// Socket ports.
    pub ports: OpusPorts,
    /// Deck to follow at start (change it with
    /// [`SourceCommand::Follow`](crate::net::SourceCommand::Follow)):
    /// the tempo master, or deck 1–4.
    pub follow: FollowTarget,
}

impl Default for OpusConfig {
    fn default() -> Self {
        Self {
            interface: None,
            mac: None,
            device_number: packets::DEFAULT_DEVICE_NUMBER,
            computer_name: "player5".to_string(),
            announce_interval: DEFAULT_ANNOUNCE_INTERVAL,
            broadcast: None,
            ports: OpusPorts::default(),
            follow: FollowTarget::Master,
        }
    }
}

impl OpusConfig {
    /// The settings a [`session::Session`] runs with: the interval
    /// clamped, an unspecified interface treated as unknown.
    #[must_use]
    pub fn settings(&self) -> session::Settings {
        let interval = self
            .announce_interval
            .clamp(MIN_ANNOUNCE_INTERVAL, MAX_ANNOUNCE_INTERVAL);
        session::Settings {
            device_number: self.device_number,
            mac: self.mac,
            interface: self.interface.filter(|ip| !ip.is_unspecified()),
            broadcast: self.broadcast,
            computer_name: self.computer_name.clone(),
            announce_interval_ns: interval.as_nanos() as u64,
            peer_announce_port: self.ports.peer_announce,
            peer_update_port: self.ports.peer_update,
            follow: self.follow,
        }
    }
}

/// Starts the Opus Quad source on its own thread: binds the announce and
/// update ports from `config.ports`, announces as rekordbox lighting,
/// keeps the deck table and reports observations of the followed deck.
///
/// Fails if a port cannot be bound (usually because rekordbox or another
/// DJ Link program, including player5's Pro DJ Link source, holds it).
pub fn start(config: OpusConfig) -> std::io::Result<SourceHandle> {
    let announce = source::bind(config.ports.bind, config.ports.announce, "announce")?;
    let update = source::bind(config.ports.bind, config.ports.update, "status")?;
    start_with_sockets(config, announce, update)
}

/// Like [`start`], on sockets the caller already bound (the ports in
/// `config.ports` that name local ports are ignored). Lets tests and
/// embedders choose ports and know them before the source runs.
pub fn start_with_sockets(
    config: OpusConfig,
    announce: UdpSocket,
    update: UdpSocket,
) -> std::io::Result<SourceHandle> {
    announce.set_broadcast(true)?;
    update.set_broadcast(true)?;
    announce.set_nonblocking(true)?;
    update.set_nonblocking(false)?;
    update.set_read_timeout(Some(source::POLL))?;
    let settings = config.settings();
    SourceHandle::spawn("opus", move |ctx| {
        source::run(ctx, announce, update, settings);
    })
}
