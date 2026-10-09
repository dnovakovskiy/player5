//! Pro DJ Link packet layouts: pure parsing and building.
//!
//! Every offset and encoding here is digested, with a source link per fact,
//! in `docs/protocols/pro-dj-link.md`; section names below refer to that
//! file. Parsers never panic: anything short, foreign or malformed comes back
//! as a [`ParseError`]. Builders produce exactly the documented layouts and
//! are used for our own announcements, the tests and simulators.

use std::fmt;
use std::net::Ipv4Addr;

use crate::net::DeviceKind;

/// The ten bytes every Pro DJ Link packet starts with ("Header").
pub const MAGIC: [u8; 10] = [0x51, 0x73, 0x70, 0x74, 0x31, 0x57, 0x6d, 0x4a, 0x4f, 0x4c];

/// Port for announcements and device-number negotiation ("Ports").
pub const ANNOUNCE_PORT: u16 = 50000;
/// Port for beats, precise position and mixer broadcasts ("Ports").
pub const BEAT_PORT: u16 = 50001;
/// Port for device status, unicast to joined devices ("Ports").
pub const STATUS_PORT: u16 = 50002;

/// Offset of the packet-kind byte.
pub const KIND_OFFSET: usize = 0x0a;
/// Length of the device-name field.
pub const NAME_LEN: usize = 20;
/// Raw pitch value meaning "+0 %" in beat and status packets ("Pitch").
pub const NEUTRAL_PITCH: u32 = 0x0010_0000;

/// Length of a keep-alive packet.
pub const KEEP_ALIVE_LEN: usize = 0x36;
/// Length of a beat packet.
pub const BEAT_LEN: usize = 0x60;
/// Length of a precise position packet (CDJ-3000).
pub const PRECISE_POSITION_LEN: usize = 0x3c;
/// Length of a mixer status packet.
pub const MIXER_STATUS_LEN: usize = 0x38;
/// Shortest CDJ status packet we accept (the reference implementation's
/// minimum; "CDJ status").
pub const CDJ_STATUS_MIN_LEN: usize = 0xcc;
/// CDJ status length from nexus players on; shorter packets carry no
/// status flags ("CDJ status").
pub const CDJ_STATUS_NEXUS_LEN: usize = 0xd4;
/// CDJ status length sent by the CDJ-3000.
pub const CDJ_STATUS_CDJ3000_LEN: usize = 0x200;

/// Status flag bit: playing ("Status flags").
pub const FLAG_PLAYING: u8 = 0x40;
/// Status flag bit: tempo master.
pub const FLAG_MASTER: u8 = 0x20;
/// Status flag bit: sync on.
pub const FLAG_SYNCED: u8 = 0x10;
/// Status flag bit: on air (needs a mixer).
pub const FLAG_ON_AIR: u8 = 0x08;
/// Status flag bit: degraded to BPM-only sync.
pub const FLAG_BPM_SYNC: u8 = 0x02;

/// Which of the three Pro DJ Link ports a packet arrived on. Kind bytes are
/// only meaningful together with the port.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Port {
    /// UDP 50000: announcements, number negotiation.
    Announce,
    /// UDP 50001: beats, precise position, mixer broadcasts.
    Beat,
    /// UDP 50002: status.
    Status,
}

impl Port {
    /// The standard port number.
    #[must_use]
    pub const fn number(self) -> u16 {
        match self {
            Self::Announce => ANNOUNCE_PORT,
            Self::Beat => BEAT_PORT,
            Self::Status => STATUS_PORT,
        }
    }
}

/// What a packet is, from its port and kind byte ("Packet types").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PacketKind {
    /// 50000/`0a`: initial announcement ("hello").
    Hello,
    /// 50000/`00`: first-stage device-number claim.
    ClaimStage1,
    /// 50000/`01`: a mixer will assign our number.
    AssignmentIntention,
    /// 50000/`02`: second-stage claim (or, with byte `0b` = 1, a request to
    /// a mixer for an assignment).
    ClaimStage2,
    /// 50000/`03`: a mixer assigns a number.
    Assignment,
    /// 50000/`04`: final-stage claim.
    ClaimStage3,
    /// 50000/`05`: number assignment finished.
    AssignmentFinished,
    /// 50000/`06`: keep-alive.
    KeepAlive,
    /// 50000/`08`: "that number is mine" (channel conflict).
    NumberInUse,
    /// 50001/`02`: fader start.
    FaderStart,
    /// 50001/`03`: channels on air.
    ChannelsOnAir,
    /// 50001/`0b`: precise (absolute) position, CDJ-3000.
    PrecisePosition,
    /// 50001/`26`: tempo master handoff request.
    MasterHandoffRequest,
    /// 50001/`27`: tempo master handoff response.
    MasterHandoffResponse,
    /// 50001/`28`: beat.
    Beat,
    /// 50001/`2a`: sync control.
    SyncControl,
    /// 50002/`05`: media query.
    MediaQuery,
    /// 50002/`06`: media response.
    MediaResponse,
    /// 50002/`0a`: CDJ status.
    CdjStatus,
    /// 50002/`19`: load track command.
    LoadTrack,
    /// 50002/`1a`: load track acknowledgment.
    LoadTrackAck,
    /// 50002/`29`: mixer status.
    MixerStatus,
    /// 50002/`34`: load settings command.
    LoadSettings,
    /// A Pro DJ Link packet whose kind byte is not in the table for its
    /// port.
    Unknown(u8),
}

impl PacketKind {
    /// Classifies a datagram received on `port`. `None` if it does not carry
    /// the Pro DJ Link header.
    #[must_use]
    pub fn classify(port: Port, packet: &[u8]) -> Option<Self> {
        if packet.len() <= KIND_OFFSET || packet[..MAGIC.len()] != MAGIC {
            return None;
        }
        let kind = packet[KIND_OFFSET];
        Some(match (port, kind) {
            (Port::Announce, 0x0a) => Self::Hello,
            (Port::Announce, 0x00) => Self::ClaimStage1,
            (Port::Announce, 0x01) => Self::AssignmentIntention,
            (Port::Announce, 0x02) => Self::ClaimStage2,
            (Port::Announce, 0x03) => Self::Assignment,
            (Port::Announce, 0x04) => Self::ClaimStage3,
            (Port::Announce, 0x05) => Self::AssignmentFinished,
            (Port::Announce, 0x06) => Self::KeepAlive,
            (Port::Announce, 0x08) => Self::NumberInUse,
            (Port::Beat, 0x02) => Self::FaderStart,
            (Port::Beat, 0x03) => Self::ChannelsOnAir,
            (Port::Beat, 0x0b) => Self::PrecisePosition,
            (Port::Beat, 0x26) => Self::MasterHandoffRequest,
            (Port::Beat, 0x27) => Self::MasterHandoffResponse,
            (Port::Beat, 0x28) => Self::Beat,
            (Port::Beat, 0x2a) => Self::SyncControl,
            (Port::Status, 0x05) => Self::MediaQuery,
            (Port::Status, 0x06) => Self::MediaResponse,
            (Port::Status, 0x0a) => Self::CdjStatus,
            (Port::Status, 0x19) => Self::LoadTrack,
            (Port::Status, 0x1a) => Self::LoadTrackAck,
            (Port::Status, 0x29) => Self::MixerStatus,
            (Port::Status, 0x34) => Self::LoadSettings,
            (_, other) => Self::Unknown(other),
        })
    }
}

/// Why a packet could not be parsed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseError {
    /// Shorter than the header, or the magic bytes are wrong.
    NotProDjLink,
    /// A Pro DJ Link packet, but of another kind.
    WrongKind {
        /// Kind byte the parser expects.
        expected: u8,
        /// Kind byte found.
        found: u8,
    },
    /// Too short for the fields the parser reads.
    TooShort {
        /// Bytes needed.
        needed: usize,
        /// Bytes present.
        got: usize,
    },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotProDjLink => write!(f, "not a Pro DJ Link packet"),
            Self::WrongKind { expected, found } => {
                write!(f, "packet kind {found:#04x}, expected {expected:#04x}")
            }
            Self::TooShort { needed, got } => {
                write!(f, "packet too short: {got} bytes, need {needed}")
            }
        }
    }
}

impl std::error::Error for ParseError {}

// ---------------------------------------------------------------------------
// Byte helpers. All bounds-checked; callers check the length first, but a
// short read still yields zero rather than a panic.

fn u8_at(p: &[u8], at: usize) -> u8 {
    p.get(at).copied().unwrap_or(0)
}

fn be16(p: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([u8_at(p, at), u8_at(p, at + 1)])
}

fn be32(p: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([
        u8_at(p, at),
        u8_at(p, at + 1),
        u8_at(p, at + 2),
        u8_at(p, at + 3),
    ])
}

fn ip_at(p: &[u8], at: usize) -> Ipv4Addr {
    Ipv4Addr::new(
        u8_at(p, at),
        u8_at(p, at + 1),
        u8_at(p, at + 2),
        u8_at(p, at + 3),
    )
}

fn mac_at(p: &[u8], at: usize) -> [u8; 6] {
    let mut mac = [0; 6];
    for (i, b) in mac.iter_mut().enumerate() {
        *b = u8_at(p, at + i);
    }
    mac
}

/// Reads a NUL-padded ASCII name; anything after the first NUL is ignored
/// and non-ASCII bytes become `?`.
fn name_at(p: &[u8], at: usize) -> String {
    let end = (at + NAME_LEN).min(p.len());
    let field = p.get(at..end).unwrap_or(&[]);
    field
        .iter()
        .take_while(|&&b| b != 0)
        .map(|&b| {
            if b.is_ascii_graphic() || b == b' ' {
                char::from(b)
            } else {
                '?'
            }
        })
        .collect::<String>()
        .trim()
        .to_owned()
}

/// The sender's name from any Pro DJ Link packet received on `port` (it
/// sits one byte later on port 50000). Empty if the packet is too short.
#[must_use]
pub fn device_name(port: Port, packet: &[u8]) -> String {
    match port {
        Port::Announce => name_at(packet, 0x0c),
        Port::Beat | Port::Status => name_at(packet, 0x0b),
    }
}

fn check(packet: &[u8], kind: u8, needed: usize) -> Result<(), ParseError> {
    if packet.len() <= KIND_OFFSET || packet[..MAGIC.len()] != MAGIC {
        return Err(ParseError::NotProDjLink);
    }
    let found = packet[KIND_OFFSET];
    if found != kind {
        return Err(ParseError::WrongKind {
            expected: kind,
            found,
        });
    }
    if packet.len() < needed {
        return Err(ParseError::TooShort {
            needed,
            got: packet.len(),
        });
    }
    Ok(())
}

/// Converts a raw beat/status pitch to a speed multiplier (`1.0` = +0 %).
#[must_use]
pub fn pitch_to_multiplier(raw: u32) -> f64 {
    f64::from(raw) / f64::from(NEUTRAL_PITCH)
}

/// Converts a raw beat/status pitch to a percentage (`0.0` = +0 %).
#[must_use]
pub fn pitch_to_percent(raw: u32) -> f64 {
    (pitch_to_multiplier(raw) - 1.0) * 100.0
}

/// Converts a pitch percentage to the raw beat/status encoding, clamped to
/// the documented −100 %..+100 % range.
#[must_use]
pub fn percent_to_pitch(percent: f64) -> u32 {
    let raw = (1.0 + percent / 100.0) * f64::from(NEUTRAL_PITCH);
    if raw.is_finite() {
        raw.round().clamp(0.0, f64::from(2 * NEUTRAL_PITCH)) as u32
    } else {
        NEUTRAL_PITCH
    }
}

fn effective(bpm_x100: u16, pitch: u32) -> Option<f64> {
    if bpm_x100 == 0xffff || bpm_x100 == 0 {
        return None;
    }
    let bpm = f64::from(bpm_x100) / 100.0 * pitch_to_multiplier(pitch);
    (bpm.is_finite() && bpm > 0.0).then_some(bpm)
}

// ---------------------------------------------------------------------------
// Port 50000: announcements and number negotiation.

/// A keep-alive ("I am still here"), broadcast by every device on port
/// 50000 ("Keep-alive").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeepAlive {
    /// Device number `D`.
    pub number: u8,
    /// Device name (model name for hardware).
    pub name: String,
    /// MAC address field.
    pub mac: [u8; 6],
    /// IP address field.
    pub ip: Ipv4Addr,
    /// Number of devices the sender sees, itself included.
    pub peers: u8,
    /// Byte `34`: `01` for players, `02` for mixers.
    pub device_type: u8,
    /// Byte `25` = `02`: the sender was first on the network when it booted.
    pub first_on_network: bool,
    /// Byte `35` = `64`: the CDJ-3000-compatible variant.
    pub cdj3000_compatible: bool,
}

impl KeepAlive {
    /// What kind of device this announcement describes.
    #[must_use]
    pub fn kind(&self) -> DeviceKind {
        classify_device(&self.name, self.number, Some(self.device_type))
    }
}

/// Best guess at a device's kind from its name, number and (if a keep-alive
/// was seen) its device-type byte ("Device kinds").
#[must_use]
pub fn classify_device(name: &str, number: u8, device_type: Option<u8>) -> DeviceKind {
    if name == OPUS_QUAD_NAME {
        return DeviceKind::AllInOne;
    }
    if name.to_ascii_lowercase().starts_with("rekordbox") {
        return DeviceKind::Rekordbox;
    }
    match device_type {
        Some(2) => DeviceKind::Mixer,
        Some(1) if (1..=6).contains(&number) => DeviceKind::Player,
        Some(_) => DeviceKind::Other,
        None if (1..=6).contains(&number) => DeviceKind::Player,
        None if number >= 0x21 => DeviceKind::Mixer,
        None => DeviceKind::Other,
    }
}

/// The name the Opus Quad announces ("Opus Quad").
pub const OPUS_QUAD_NAME: &str = "OPUS-QUAD";

/// Parses a keep-alive.
pub fn parse_keep_alive(packet: &[u8]) -> Result<KeepAlive, ParseError> {
    check(packet, 0x06, KEEP_ALIVE_LEN)?;
    Ok(KeepAlive {
        number: packet[0x24],
        name: name_at(packet, 0x0c),
        mac: mac_at(packet, 0x26),
        ip: ip_at(packet, 0x2c),
        peers: packet[0x30],
        device_type: packet[0x34],
        first_on_network: packet[0x25] == 2,
        cdj3000_compatible: packet[0x35] == 0x64,
    })
}

/// A device-number claim, stage 1, 2 or 3 ("Joining").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NumberClaim {
    /// 1, 2 or 3.
    pub stage: u8,
    /// Sender name.
    pub name: String,
    /// Packet counter `N` within the stage (1..=3).
    pub counter: u8,
    /// The number being claimed (stages 2 and 3).
    pub number: Option<u8>,
    /// Sender IP (stage 2).
    pub ip: Option<Ipv4Addr>,
    /// Sender MAC (stages 1 and 2).
    pub mac: Option<[u8; 6]>,
    /// Stage 2: `true` when auto-assigning, `false` for a specific number.
    pub auto_assign: Option<bool>,
    /// Stage 2 with byte `0b` = `01`: a request to a mixer for an
    /// assignment, sent unicast.
    pub assignment_request: bool,
}

/// Parses a stage 1, 2 or 3 number claim (kinds `00`, `02`, `04`).
pub fn parse_number_claim(packet: &[u8]) -> Result<NumberClaim, ParseError> {
    if packet.len() <= KIND_OFFSET || packet[..MAGIC.len()] != MAGIC {
        return Err(ParseError::NotProDjLink);
    }
    match packet[KIND_OFFSET] {
        0x00 => {
            check(packet, 0x00, 0x2c)?;
            Ok(NumberClaim {
                stage: 1,
                name: name_at(packet, 0x0c),
                counter: packet[0x24],
                number: None,
                ip: None,
                mac: Some(mac_at(packet, 0x26)),
                auto_assign: None,
                assignment_request: false,
            })
        }
        0x02 => {
            check(packet, 0x02, 0x32)?;
            let number = packet[0x2e];
            Ok(NumberClaim {
                stage: 2,
                name: name_at(packet, 0x0c),
                counter: packet[0x2f],
                number: (number != 0).then_some(number),
                ip: Some(ip_at(packet, 0x24)),
                mac: Some(mac_at(packet, 0x28)),
                auto_assign: Some(packet[0x31] == 1),
                assignment_request: packet[0x0b] == 1,
            })
        }
        0x04 => {
            check(packet, 0x04, 0x26)?;
            Ok(NumberClaim {
                stage: 3,
                name: name_at(packet, 0x0c),
                counter: packet[0x25],
                number: Some(packet[0x24]),
                ip: None,
                mac: None,
                auto_assign: None,
                assignment_request: false,
            })
        }
        found => Err(ParseError::WrongKind {
            expected: 0x02,
            found,
        }),
    }
}

/// "That number is mine": sent unicast to a device claiming a number in use
/// ("Channel conflicts").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NumberInUse {
    /// Name of the defending device.
    pub name: String,
    /// The defended number `D`.
    pub number: u8,
    /// IP of the defending device.
    pub ip: Ipv4Addr,
}

/// Parses a channel-conflict packet (kind `08`).
pub fn parse_number_in_use(packet: &[u8]) -> Result<NumberInUse, ParseError> {
    check(packet, 0x08, 0x29)?;
    Ok(NumberInUse {
        name: name_at(packet, 0x0c),
        number: packet[0x24],
        ip: ip_at(packet, 0x25),
    })
}

/// Parses a mixer's number assignment (kind `03`) and returns the assigned
/// number (`0` = "use any").
pub fn parse_assignment(packet: &[u8]) -> Result<u8, ParseError> {
    check(packet, 0x03, 0x26)?;
    Ok(packet[0x24])
}

/// Parses an "assignment finished" packet (kind `05`) and returns the
/// sender's own device number.
pub fn parse_assignment_finished(packet: &[u8]) -> Result<u8, ParseError> {
    check(packet, 0x05, 0x26)?;
    Ok(packet[0x24])
}

// ---------------------------------------------------------------------------
// Port 50001: beats, precise position, on-air.

/// A beat packet: broadcast by a player at the start of every beat while it
/// plays an analysed track, and by the mixer as a metronome ("Beat packets").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BeatPacket {
    /// Device number of the sender.
    pub device: u8,
    /// Sender name.
    pub name: String,
    /// Milliseconds to the next beat at +0 % pitch (`0xffffffff`: the track
    /// ends first). Same convention for the five fields below.
    pub next_beat_ms: u32,
    /// Milliseconds to the beat after that.
    pub second_beat_ms: u32,
    /// Milliseconds to the next downbeat.
    pub next_bar_ms: u32,
    /// Milliseconds to the fourth upcoming beat.
    pub fourth_beat_ms: u32,
    /// Milliseconds to the downbeat after next.
    pub second_bar_ms: u32,
    /// Milliseconds to the eighth upcoming beat.
    pub eighth_beat_ms: u32,
    /// Raw pitch (`NEUTRAL_PITCH` = +0 %).
    pub pitch: u32,
    /// Track BPM × 100 (`0xffff`: unknown).
    pub bpm_x100: u16,
    /// Beat within the bar, 1..=4 (meaningless from a mixer).
    pub beat_within_bar: u8,
}

impl BeatPacket {
    /// A packet for `device` at `track_bpm` and `pitch_percent`, with the
    /// upcoming-beat timings filled in for a steady grid. For tests and the
    /// simulator.
    #[must_use]
    pub fn new(device: u8, track_bpm: f64, pitch_percent: f64, beat_within_bar: u8) -> Self {
        let beat_ms = if track_bpm > 0.0 {
            60_000.0 / track_bpm
        } else {
            0.0
        };
        let at = |beats: u32| (beat_ms * f64::from(beats)).round() as u32;
        let to_bar = if (1..=4).contains(&beat_within_bar) {
            u32::from(5 - beat_within_bar)
        } else {
            4
        };
        Self {
            device,
            name: "CDJ-3000".to_owned(),
            next_beat_ms: at(1),
            second_beat_ms: at(2),
            next_bar_ms: at(to_bar),
            fourth_beat_ms: at(4),
            second_bar_ms: at(to_bar + 4),
            eighth_beat_ms: at(8),
            pitch: percent_to_pitch(pitch_percent),
            bpm_x100: (track_bpm * 100.0).round().clamp(0.0, 65_534.0) as u16,
            beat_within_bar,
        }
    }

    /// Track BPM (before pitch), if known.
    #[must_use]
    pub fn track_bpm(&self) -> Option<f64> {
        effective(self.bpm_x100, NEUTRAL_PITCH)
    }

    /// Effective (playing) BPM: track BPM × pitch.
    #[must_use]
    pub fn effective_bpm(&self) -> Option<f64> {
        effective(self.bpm_x100, self.pitch)
    }

    /// Pitch as a percentage.
    #[must_use]
    pub fn pitch_percent(&self) -> f64 {
        pitch_to_percent(self.pitch)
    }
}

/// Parses a beat packet (50001, kind `28`).
pub fn parse_beat(packet: &[u8]) -> Result<BeatPacket, ParseError> {
    check(packet, 0x28, BEAT_LEN)?;
    Ok(BeatPacket {
        device: packet[0x21],
        name: name_at(packet, 0x0b),
        next_beat_ms: be32(packet, 0x24),
        second_beat_ms: be32(packet, 0x28),
        next_bar_ms: be32(packet, 0x2c),
        fourth_beat_ms: be32(packet, 0x30),
        second_bar_ms: be32(packet, 0x34),
        eighth_beat_ms: be32(packet, 0x38),
        pitch: be32(packet, 0x54),
        bpm_x100: be16(packet, 0x5a),
        beat_within_bar: packet[0x5c],
    })
}

/// CDJ-3000 precise position, sent every 30 ms while a track is loaded
/// ("Precise position").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrecisePosition {
    /// Device number.
    pub device: u8,
    /// Sender name.
    pub name: String,
    /// Track length in whole seconds.
    pub track_length_s: u32,
    /// Playhead position in milliseconds.
    pub playhead_ms: u32,
    /// Pitch slider × 100 (326 = +3.26 %), signed.
    pub pitch_x100: i32,
    /// Effective BPM × 10 (`0xffffffff`: unknown).
    pub bpm_x10: u32,
}

impl PrecisePosition {
    /// Effective BPM, if known.
    #[must_use]
    pub fn effective_bpm(&self) -> Option<f64> {
        (self.bpm_x10 != 0xffff_ffff && self.bpm_x10 != 0).then(|| f64::from(self.bpm_x10) / 10.0)
    }

    /// Pitch as a percentage.
    #[must_use]
    pub fn pitch_percent(&self) -> f64 {
        f64::from(self.pitch_x100) / 100.0
    }
}

/// Parses a precise position packet (50001, kind `0b`).
pub fn parse_precise_position(packet: &[u8]) -> Result<PrecisePosition, ParseError> {
    check(packet, 0x0b, PRECISE_POSITION_LEN)?;
    Ok(PrecisePosition {
        device: packet[0x21],
        name: name_at(packet, 0x0b),
        track_length_s: be32(packet, 0x24),
        playhead_ms: be32(packet, 0x28),
        pitch_x100: be32(packet, 0x2c) as i32,
        bpm_x10: be32(packet, 0x38),
    })
}

/// Which mixer channels are on air ("Channels on air").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OnAir {
    /// Sender (mixer) device number.
    pub device: u8,
    /// Sender name.
    pub name: String,
    /// Channels 1..=6; `None` for 5 and 6 in the four-channel variant.
    pub channels: [Option<bool>; 6],
}

/// Parses a channels-on-air packet (50001, kind `03`), four- or
/// six-channel.
pub fn parse_on_air(packet: &[u8]) -> Result<OnAir, ParseError> {
    check(packet, 0x03, 0x28)?;
    let mut channels = [None; 6];
    for (i, ch) in channels.iter_mut().take(4).enumerate() {
        *ch = Some(packet[0x24 + i] != 0);
    }
    if packet.len() >= 0x35 {
        channels[4] = Some(packet[0x2d] != 0);
        channels[5] = Some(packet[0x2e] != 0);
    }
    Ok(OnAir {
        device: packet[0x21],
        name: name_at(packet, 0x0b),
        channels,
    })
}

// ---------------------------------------------------------------------------
// Port 50002: status.

/// A CDJ status packet, unicast every ~200 ms to every joined device
/// ("CDJ status").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CdjStatus {
    /// Device (player) number.
    pub device: u8,
    /// Sender name.
    pub name: String,
    /// Length of the packet (selects how `playing` is derived).
    pub packet_len: usize,
    /// Activity byte `A` (0 idle, 1 playing/searching/loading).
    pub activity: u8,
    /// Track type `Tr` (0 none, 1 rekordbox, 2 unanalysed, 5 CD, 6
    /// streaming).
    pub track_type: u8,
    /// Play state `P1` (3 playing, 4 looping, 5 paused, 6 at cue, 9
    /// searching, ...).
    pub play_state: u8,
    /// Firmware version, ASCII.
    pub firmware: String,
    /// Status flags `F` (see the `FLAG_*` constants).
    pub flags: u8,
    /// Second play state `P2` (moving vs stopped).
    pub play_state_2: u8,
    /// Effective pitch `Pitch1`, raw.
    pub pitch: u32,
    /// Track BPM × 100 at the playhead (`0xffff`: no track).
    pub bpm_x100: u16,
    /// Master handoff `Mh`: `0xff` normally, else the device the master
    /// role is being yielded to.
    pub master_handoff: u8,
    /// Beat counter from 1 (`0xffffffff`: unavailable).
    pub beat: u32,
    /// Beat within the bar, 1..=4 (0 when not available).
    pub beat_within_bar: u8,
}

impl CdjStatus {
    /// Whether the player is playing. Uses the play flag on nexus and later
    /// packets, else infers it from the play states as the reference
    /// implementation does.
    #[must_use]
    pub fn playing(&self) -> bool {
        if self.packet_len >= CDJ_STATUS_NEXUS_LEN {
            self.flags & FLAG_PLAYING != 0
        } else {
            matches!(self.play_state, 3 | 4)
                || (self.play_state == 9 && matches!(self.play_state_2, 0x6a | 0x7a | 0x9a | 0xfa))
        }
    }

    /// Whether this player reports itself as tempo master.
    #[must_use]
    pub fn master(&self) -> bool {
        self.flags & FLAG_MASTER != 0
    }

    /// Whether sync is on.
    #[must_use]
    pub fn synced(&self) -> bool {
        self.flags & FLAG_SYNCED != 0
    }

    /// Whether the player believes it is audible in the mix.
    #[must_use]
    pub fn on_air(&self) -> bool {
        self.flags & FLAG_ON_AIR != 0
    }

    /// Track BPM at the playhead, if a track is loaded.
    #[must_use]
    pub fn track_bpm(&self) -> Option<f64> {
        effective(self.bpm_x100, NEUTRAL_PITCH)
    }

    /// Effective BPM (track BPM × effective pitch).
    #[must_use]
    pub fn effective_bpm(&self) -> Option<f64> {
        effective(self.bpm_x100, self.pitch)
    }

    /// Beat number, if available.
    #[must_use]
    pub fn beat_number(&self) -> Option<u32> {
        (self.beat != 0xffff_ffff).then_some(self.beat)
    }

    /// The device the master role is being handed to, during a handoff.
    #[must_use]
    pub fn master_handoff_to(&self) -> Option<u8> {
        (self.master_handoff != 0xff && self.master_handoff != 0).then_some(self.master_handoff)
    }

    /// A plausible CDJ-3000 status for tests and simulators.
    #[must_use]
    pub fn new(
        device: u8,
        track_bpm: f64,
        pitch_percent: f64,
        playing: bool,
        master: bool,
    ) -> Self {
        let mut flags = 0x84; // bits 7 and 2 are always set ("Status flags")
        if playing {
            flags |= FLAG_PLAYING;
        }
        if master {
            flags |= FLAG_MASTER;
        }
        Self {
            device,
            name: "CDJ-3000".to_owned(),
            packet_len: CDJ_STATUS_CDJ3000_LEN,
            activity: u8::from(playing),
            track_type: 1,
            play_state: if playing { 3 } else { 5 },
            firmware: "3.30".to_owned(),
            flags,
            play_state_2: if playing { 0x7a } else { 0x7e },
            pitch: percent_to_pitch(pitch_percent),
            bpm_x100: (track_bpm * 100.0).round().clamp(0.0, 65_534.0) as u16,
            master_handoff: 0xff,
            beat: 1,
            beat_within_bar: 1,
        }
    }
}

/// Parses a CDJ status packet (50002, kind `0a`).
pub fn parse_cdj_status(packet: &[u8]) -> Result<CdjStatus, ParseError> {
    check(packet, 0x0a, CDJ_STATUS_MIN_LEN)?;
    let firmware = packet[0x7c..0x80]
        .iter()
        .take_while(|&&b| b != 0)
        .map(|&b| {
            if b.is_ascii_graphic() {
                char::from(b)
            } else {
                '?'
            }
        })
        .collect();
    Ok(CdjStatus {
        device: packet[0x21],
        name: name_at(packet, 0x0b),
        packet_len: packet.len(),
        activity: packet[0x27],
        track_type: packet[0x2a],
        play_state: packet[0x7b],
        firmware,
        flags: packet[0x89],
        play_state_2: packet[0x8b],
        pitch: be32(packet, 0x8c),
        bpm_x100: be16(packet, 0x92),
        master_handoff: packet[0x9f],
        beat: be32(packet, 0xa0),
        beat_within_bar: packet[0xa6],
    })
}

/// A mixer status packet, unicast to joined devices ("Mixer status").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MixerStatus {
    /// Mixer device number (`0x21` for a lone DJM).
    pub device: u8,
    /// Sender name.
    pub name: String,
    /// Status flags (`f0` master, `d0` not).
    pub flags: u8,
    /// Pitch, raw (always +0 % so far).
    pub pitch: u32,
    /// BPM × 100 (the master's tempo, when a rekordbox track plays).
    pub bpm_x100: u16,
    /// Master handoff byte `Mh`.
    pub master_handoff: u8,
    /// Beat within the bar (not synchronised with the master player).
    pub beat_within_bar: u8,
}

impl MixerStatus {
    /// Whether the mixer is tempo master.
    #[must_use]
    pub fn master(&self) -> bool {
        self.flags & FLAG_MASTER != 0
    }

    /// The mixer's BPM, if valid.
    #[must_use]
    pub fn effective_bpm(&self) -> Option<f64> {
        effective(self.bpm_x100, self.pitch)
    }
}

/// Parses a mixer status packet (50002, kind `29`).
pub fn parse_mixer_status(packet: &[u8]) -> Result<MixerStatus, ParseError> {
    check(packet, 0x29, MIXER_STATUS_LEN)?;
    Ok(MixerStatus {
        device: packet[0x21],
        name: name_at(packet, 0x0b),
        flags: packet[0x27],
        pitch: be32(packet, 0x28),
        bpm_x100: be16(packet, 0x2e),
        master_handoff: packet[0x36],
        beat_within_bar: packet[0x37],
    })
}

// ---------------------------------------------------------------------------
// Builders.

fn put_name(buf: &mut [u8], at: usize, name: &str) {
    for (i, b) in name.bytes().take(NAME_LEN).enumerate() {
        buf[at + i] = if b.is_ascii() && b != 0 { b } else { b'?' };
    }
}

fn put16(buf: &mut [u8], at: usize, v: u16) {
    buf[at..at + 2].copy_from_slice(&v.to_be_bytes());
}

fn put32(buf: &mut [u8], at: usize, v: u32) {
    buf[at..at + 4].copy_from_slice(&v.to_be_bytes());
}

/// Header of port-50000 packets: magic, kind, `0b` subtype, name at `0c`,
/// `01`, structure byte at `21`, total length at `22`.
fn announce_packet(kind: u8, subtype: u8, name: &str, structure: u8, len: usize) -> Vec<u8> {
    let mut p = vec![0; len];
    p[..10].copy_from_slice(&MAGIC);
    p[0x0a] = kind;
    p[0x0b] = subtype;
    put_name(&mut p, 0x0c, name);
    p[0x20] = 0x01;
    p[0x21] = structure;
    put16(&mut p, 0x22, len as u16);
    p
}

/// Header of port-50001/50002 packets: magic, kind, name at `0b`, `1f`
/// byte, subtype at `20`, device at `21`, remaining length at `22`.
fn update_packet(kind: u8, name: &str, b1f: u8, subtype: u8, device: u8, len: usize) -> Vec<u8> {
    let mut p = vec![0; len];
    p[..10].copy_from_slice(&MAGIC);
    p[0x0a] = kind;
    put_name(&mut p, 0x0b, name);
    p[0x1f] = b1f;
    p[0x20] = subtype;
    p[0x21] = device;
    put16(&mut p, 0x22, (len - 0x24) as u16);
    p
}

/// Builds the CDJ-3000-compatible initial announcement ("hello") we send
/// three times when joining.
#[must_use]
pub fn build_hello(name: &str) -> Vec<u8> {
    let mut p = announce_packet(0x0a, 0, name, 0x04, 0x26);
    p[0x24] = 0x01;
    p[0x25] = 0x40;
    p
}

/// Builds a CDJ-3000-compatible first-stage claim.
#[must_use]
pub fn build_claim_stage1(name: &str, mac: [u8; 6], counter: u8) -> Vec<u8> {
    let mut p = announce_packet(0x00, 0, name, 0x03, 0x2c);
    p[0x24] = counter;
    p[0x25] = 0x01;
    p[0x26..0x2c].copy_from_slice(&mac);
    p
}

/// Builds a CDJ-3000-compatible second-stage claim for `number`.
#[must_use]
pub fn build_claim_stage2(
    name: &str,
    ip: Ipv4Addr,
    mac: [u8; 6],
    number: u8,
    counter: u8,
    auto_assign: bool,
) -> Vec<u8> {
    let mut p = announce_packet(0x02, 0, name, 0x03, 0x32);
    p[0x24..0x28].copy_from_slice(&ip.octets());
    p[0x28..0x2e].copy_from_slice(&mac);
    p[0x2e] = number;
    p[0x2f] = counter;
    p[0x30] = 0x01;
    p[0x31] = if auto_assign { 0x01 } else { 0x02 };
    p
}

/// Builds a CDJ-3000-compatible final-stage claim for `number`.
#[must_use]
pub fn build_claim_stage3(name: &str, number: u8, counter: u8) -> Vec<u8> {
    let mut p = announce_packet(0x04, 0, name, 0x03, 0x26);
    p[0x24] = number;
    p[0x25] = counter;
    p
}

/// Builds the request a device sends straight back to a mixer that
/// announced it will assign the number (kind `02`, byte `0b` = `01`, `D` =
/// 0).
#[must_use]
pub fn build_assignment_request(
    name: &str,
    ip: Ipv4Addr,
    mac: [u8; 6],
    auto_assign: bool,
) -> Vec<u8> {
    let mut p = announce_packet(0x02, 0x01, name, 0x02, 0x32);
    p[0x24..0x28].copy_from_slice(&ip.octets());
    p[0x28..0x2e].copy_from_slice(&mac);
    p[0x2f] = 0x01;
    p[0x30] = 0x01;
    p[0x31] = if auto_assign { 0x01 } else { 0x02 };
    p
}

/// Builds a mixer-style assignment-intention packet (kind `01`), for tests.
#[must_use]
pub fn build_assignment_intention(name: &str, ip: Ipv4Addr, mac: [u8; 6]) -> Vec<u8> {
    let mut p = announce_packet(0x01, 0, name, 0x02, 0x2f);
    p[0x24..0x28].copy_from_slice(&ip.octets());
    p[0x28..0x2e].copy_from_slice(&mac);
    p[0x2e] = 0x01;
    p
}

/// Builds a mixer-style number assignment (kind `03`), for tests.
#[must_use]
pub fn build_assignment(name: &str, number: u8) -> Vec<u8> {
    let mut p = announce_packet(0x03, 0x01, name, 0x02, 0x27);
    p[0x24] = number;
    p[0x25] = 0x01;
    p
}

/// Builds an "assignment finished" packet (kind `05`) from device
/// `sender`, for tests.
#[must_use]
pub fn build_assignment_finished(name: &str, sender: u8) -> Vec<u8> {
    let mut p = announce_packet(0x05, 0, name, 0x02, 0x26);
    p[0x24] = sender;
    p[0x25] = 0x01;
    p
}

/// Builds a channel-conflict packet defending `number`.
#[must_use]
pub fn build_number_in_use(name: &str, number: u8, ip: Ipv4Addr) -> Vec<u8> {
    let mut p = announce_packet(0x08, 0, name, 0x02, 0x29);
    p[0x24] = number;
    p[0x25..0x29].copy_from_slice(&ip.octets());
    p
}

/// Builds a keep-alive. With `cdj3000_compatible` it is the variant that
/// lets CDJ-3000s on numbers 5 and 6 coexist with us ("Joining").
#[must_use]
pub fn build_keep_alive(k: &KeepAlive) -> Vec<u8> {
    let mut p = announce_packet(0x06, 0, &k.name, 0x02, KEEP_ALIVE_LEN);
    p[0x24] = k.number;
    p[0x25] = if k.first_on_network { 0x02 } else { 0x01 };
    p[0x26..0x2c].copy_from_slice(&k.mac);
    p[0x2c..0x30].copy_from_slice(&k.ip.octets());
    p[0x30] = k.peers;
    p[0x34] = k.device_type;
    p[0x35] = if k.cdj3000_compatible { 0x64 } else { 0x00 };
    p
}

/// Builds a beat packet.
#[must_use]
pub fn build_beat_packet(b: &BeatPacket) -> Vec<u8> {
    let mut p = update_packet(0x28, &b.name, 0x01, 0x00, b.device, BEAT_LEN);
    put32(&mut p, 0x24, b.next_beat_ms);
    put32(&mut p, 0x28, b.second_beat_ms);
    put32(&mut p, 0x2c, b.next_bar_ms);
    put32(&mut p, 0x30, b.fourth_beat_ms);
    put32(&mut p, 0x34, b.second_bar_ms);
    put32(&mut p, 0x38, b.eighth_beat_ms);
    p[0x3c..0x54].fill(0xff);
    put32(&mut p, 0x54, b.pitch);
    put16(&mut p, 0x5a, b.bpm_x100);
    p[0x5c] = b.beat_within_bar;
    p[0x5f] = b.device;
    p
}

/// Builds a precise position packet.
#[must_use]
pub fn build_precise_position(pp: &PrecisePosition) -> Vec<u8> {
    let mut p = update_packet(0x0b, &pp.name, 0x02, 0x00, pp.device, PRECISE_POSITION_LEN);
    put32(&mut p, 0x24, pp.track_length_s);
    put32(&mut p, 0x28, pp.playhead_ms);
    put32(&mut p, 0x2c, pp.pitch_x100 as u32);
    put32(&mut p, 0x38, pp.bpm_x10);
    p
}

/// Builds a channels-on-air packet; the six-channel variant if channel 5
/// or 6 is set.
#[must_use]
pub fn build_on_air(o: &OnAir) -> Vec<u8> {
    let six = o.channels[4].is_some() || o.channels[5].is_some();
    // Subtype `02` is what a DJM-2000nexus sends for four channels (real
    // capture; the layout diagram shows `00`), `03` the six-channel one.
    let (len, subtype) = if six { (0x35, 0x03) } else { (0x2d, 0x02) };
    let mut p = update_packet(0x03, &o.name, 0x01, subtype, o.device, len);
    for i in 0..4 {
        p[0x24 + i] = u8::from(o.channels[i] == Some(true));
    }
    if six {
        p[0x2d] = u8::from(o.channels[4] == Some(true));
        p[0x2e] = u8::from(o.channels[5] == Some(true));
    }
    p
}

/// Shortest status packet a player sends, and so the shortest we build.
const CDJ_STATUS_BUILD_MIN_LEN: usize = 0xd0;

/// Builds a CDJ status packet of `s.packet_len` bytes (clamped to
/// `0xd0..=0x400`, the shortest real length up).
#[must_use]
pub fn build_cdj_status(s: &CdjStatus) -> Vec<u8> {
    let len = s.packet_len.clamp(CDJ_STATUS_BUILD_MIN_LEN, 0x400);
    let mut p = update_packet(0x0a, &s.name, 0x01, 0x03, s.device, len);
    let loaded = s.bpm_x100 != 0xffff;
    let playing = s.playing();
    p[0x24] = s.device;
    p[0x26] = 0x01;
    p[0x27] = s.activity;
    p[0x28] = if loaded { s.device } else { 0 };
    p[0x29] = if loaded { 0x03 } else { 0 };
    p[0x2a] = s.track_type;
    p[0x6f] = 0x04;
    p[0x73] = 0x04;
    p[0x7b] = s.play_state;
    for (i, b) in s.firmware.bytes().take(4).enumerate() {
        p[0x7c + i] = b;
    }
    p[0x89] = s.flags;
    p[0x8a] = 0xff;
    p[0x8b] = s.play_state_2;
    put32(&mut p, 0x8c, s.pitch);
    put16(&mut p, 0x90, if loaded { 0x8000 } else { 0x7fff });
    put16(&mut p, 0x92, s.bpm_x100);
    put16(&mut p, 0x94, 0x7fff);
    put16(&mut p, 0x96, 0xffff);
    put32(&mut p, 0x98, if playing { s.pitch } else { 0 });
    p[0x9d] = if playing { 0x0d } else { 0x01 };
    p[0x9e] = u8::from(s.master());
    p[0x9f] = s.master_handoff;
    put32(&mut p, 0xa0, s.beat);
    put16(&mut p, 0xa4, 0x01ff);
    p[0xa6] = s.beat_within_bar;
    p[0xb6] = 0x01;
    put32(&mut p, 0xc0, s.pitch);
    put32(&mut p, 0xc4, if playing { s.pitch } else { 0 });
    p[0xcc] = 0x1f;
    p
}

/// Builds a mixer status packet.
#[must_use]
pub fn build_mixer_status(m: &MixerStatus) -> Vec<u8> {
    let mut p = update_packet(0x29, &m.name, 0x01, 0x00, m.device, MIXER_STATUS_LEN);
    p[0x24] = m.device;
    p[0x27] = m.flags;
    put32(&mut p, 0x28, m.pitch);
    p[0x2c] = 0x80;
    put16(&mut p, 0x2e, m.bpm_x100);
    put32(&mut p, 0x30, NEUTRAL_PITCH);
    p[0x35] = 0x09;
    p[0x36] = m.master_handoff;
    p[0x37] = m.beat_within_bar;
    p
}
