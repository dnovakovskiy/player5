//! Opus Quad packets: pure parsing and building, no I/O.
//!
//! Every offset and constant here is documented, with its source, in
//! `docs/protocols/opus-quad.md`; the doc comments name the section.
//! Parsers never panic: any byte slice yields either a value or a
//! [`ParseError`] (the fuzz test in `tests.rs` holds them to that).

use std::fmt;
use std::net::Ipv4Addr;
use std::ops::RangeInclusive;

/// The ten bytes every DJ Link packet starts with (opus-quad.md, "Ports and
/// header").
pub const MAGIC: [u8; 10] = [0x51, 0x73, 0x70, 0x74, 0x31, 0x57, 0x6d, 0x4a, 0x4f, 0x4c];

/// Offset of the packet-kind byte that follows [`MAGIC`].
pub const KIND_OFFSET: usize = 0x0a;

/// UDP port keep-alives are broadcast to (opus-quad.md, "Ports and header").
pub const ANNOUNCE_PORT: u16 = 50000;

/// UDP port status packets and the lighting request go to (opus-quad.md,
/// "Ports and header").
pub const UPDATE_PORT: u16 = 50002;

/// Name the Opus Quad puts in its packets (opus-quad.md, "What the unit
/// sends").
pub const OPUS_NAME: &str = "OPUS-QUAD";

/// Name rekordbox, and therefore player5, announces (opus-quad.md,
/// "Joining as rekordbox lighting").
pub const REKORDBOX_NAME: &str = "rekordbox";

/// Device number in the published rekordbox-lighting packets, `0x17`
/// (opus-quad.md, "Joining as rekordbox lighting").
pub const DEFAULT_DEVICE_NUMBER: u8 = 0x17;

/// Numbers beat-link falls back to when its number is taken
/// (opus-quad.md, "Device number").
pub const FALLBACK_DEVICE_NUMBERS: RangeInclusive<u8> = 0x13..=0x27;

/// Packet kinds (byte [`KIND_OFFSET`]) this module knows (opus-quad.md,
/// "Ports and header" and "What the unit sends").
pub mod kind {
    /// Keep-alive, port 50000.
    pub const KEEP_ALIVE: u8 = 0x06;
    /// CDJ status, port 50002.
    pub const STATUS: u8 = 0x0a;
    /// The unit's "rekordbox lighting hello", port 50002.
    pub const LIGHTING_HELLO: u8 = 0x10;
    /// Our lighting request, port 50002.
    pub const LIGHTING_REQUEST: u8 = 0x11;
    /// Fragmented binary metadata (album art, phrase data), port 50002.
    pub const METADATA: u8 = 0x56;
}

/// Length of a keep-alive packet.
pub const KEEP_ALIVE_LEN: usize = 0x36;

/// Length of the lighting request.
pub const LIGHTING_REQUEST_LEN: usize = 0x128;

/// Bytes of a lighting hello we need (through the device number).
pub const LIGHTING_HELLO_MIN_LEN: usize = 0x22;

/// Shortest status packet accepted (beat-link's minimum; opus-quad.md,
/// "Status packet fields").
pub const STATUS_MIN_LEN: usize = 0xcc;

/// Status packets at least this long carry a meaningful flag byte; shorter
/// ones are judged by their play states (opus-quad.md, "Status packet
/// fields").
pub const STATUS_FLAGS_MIN_LEN: usize = 0xd4;

/// Status flag bit: playing (opus-quad.md, "Status packet fields").
pub const FLAG_PLAYING: u8 = 0x40;
/// Status flag bit: tempo master.
pub const FLAG_MASTER: u8 = 0x20;
/// Status flag bit: sync on.
pub const FLAG_SYNCED: u8 = 0x10;
/// Status flag bit: on air.
pub const FLAG_ON_AIR: u8 = 0x08;

/// Why a packet was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseError {
    /// Shorter than the fields we read.
    TooShort {
        /// Bytes received.
        len: usize,
        /// Bytes needed.
        need: usize,
    },
    /// Does not start with [`MAGIC`].
    BadMagic,
    /// A DJ Link packet, but of another kind.
    WrongKind(u8),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort { len, need } => write!(f, "packet too short ({len} < {need} bytes)"),
            Self::BadMagic => f.write_str("not a DJ Link packet"),
            Self::WrongKind(k) => write!(f, "unexpected packet kind {k:#04x}"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Checks the header and returns the kind byte.
pub fn packet_kind(bytes: &[u8]) -> Result<u8, ParseError> {
    if bytes.len() <= KIND_OFFSET {
        return Err(ParseError::TooShort {
            len: bytes.len(),
            need: KIND_OFFSET + 1,
        });
    }
    if bytes[..MAGIC.len()] != MAGIC {
        return Err(ParseError::BadMagic);
    }
    Ok(bytes[KIND_OFFSET])
}

fn expect(bytes: &[u8], kind: u8, need: usize) -> Result<(), ParseError> {
    let k = packet_kind(bytes)?;
    if k != kind {
        return Err(ParseError::WrongKind(k));
    }
    if bytes.len() < need {
        return Err(ParseError::TooShort {
            len: bytes.len(),
            need,
        });
    }
    Ok(())
}

/// Reads a NUL-padded device name, trimmed like beat-link does.
fn read_name(field: &[u8]) -> String {
    let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    String::from_utf8_lossy(&field[..end]).trim().to_string()
}

/// Writes `name` NUL-padded into `field`, truncating at a char boundary.
fn write_name(field: &mut [u8], name: &str) {
    field.fill(0);
    let mut end = name.len().min(field.len());
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    field[..end].copy_from_slice(&name.as_bytes()[..end]);
}

fn be16(b: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([b[at], b[at + 1]])
}

fn be32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// A keep-alive (kind `0x06`, port 50000), in the CDJ keep-alive layout
/// (opus-quad.md, "Joining as rekordbox lighting" and "What the unit
/// sends").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeepAlive {
    /// Device name, bytes `0x0c..0x20`.
    pub name: String,
    /// Structure variant at byte `0x21` (`02` on CDJs and mixers, `03` in
    /// rekordbox lighting's packet).
    pub subtype: u8,
    /// Device number, byte `0x24`.
    pub number: u8,
    /// Byte `0x25` (on a CDJ: whether it booted alone; `01` from
    /// rekordbox lighting).
    pub byte_25: u8,
    /// MAC address, bytes `0x26..0x2c`.
    pub mac: [u8; 6],
    /// IPv4 address, bytes `0x2c..0x30`.
    pub ip: Ipv4Addr,
    /// Byte `0x30` (peer count on a CDJ; `04` from rekordbox lighting).
    pub peers: u8,
    /// Bytes `0x31..0x36`; byte `0x34` is `01` on CDJs, `02` on mixers and
    /// `04` from rekordbox lighting.
    pub tail: [u8; 5],
}

impl KeepAlive {
    /// The keep-alive rekordbox lighting broadcasts, with our number,
    /// MAC and IP (opus-quad.md, "Joining as rekordbox lighting").
    #[must_use]
    pub fn rekordbox(number: u8, mac: [u8; 6], ip: Ipv4Addr) -> Self {
        Self {
            name: REKORDBOX_NAME.to_string(),
            subtype: 0x03,
            number,
            byte_25: 0x01,
            mac,
            ip,
            peers: 0x04,
            tail: [0x01, 0x00, 0x00, 0x04, 0x08],
        }
    }

    /// Byte `0x34`, which tells CDJs (`01`) from mixers (`02`).
    #[must_use]
    pub fn device_type(&self) -> u8 {
        self.tail[3]
    }

    /// Parses a keep-alive. Longer packets are accepted (beat-link does
    /// the same); only the first [`KEEP_ALIVE_LEN`] bytes are read.
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        expect(bytes, kind::KEEP_ALIVE, KEEP_ALIVE_LEN)?;
        let mut mac = [0; 6];
        mac.copy_from_slice(&bytes[0x26..0x2c]);
        let mut tail = [0; 5];
        tail.copy_from_slice(&bytes[0x31..0x36]);
        Ok(Self {
            name: read_name(&bytes[0x0c..0x20]),
            subtype: bytes[0x21],
            number: bytes[0x24],
            byte_25: bytes[0x25],
            mac,
            ip: Ipv4Addr::new(bytes[0x2c], bytes[0x2d], bytes[0x2e], bytes[0x2f]),
            peers: bytes[0x30],
            tail,
        })
    }

    /// Serialises the packet.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; KEEP_ALIVE_LEN] {
        let mut p = [0u8; KEEP_ALIVE_LEN];
        p[..MAGIC.len()].copy_from_slice(&MAGIC);
        p[KIND_OFFSET] = kind::KEEP_ALIVE;
        p[0x0b] = 0x00;
        write_name(&mut p[0x0c..0x20], &self.name);
        p[0x20] = 0x01;
        p[0x21] = self.subtype;
        p[0x22..0x24].copy_from_slice(&(KEEP_ALIVE_LEN as u16).to_be_bytes());
        p[0x24] = self.number;
        p[0x25] = self.byte_25;
        p[0x26..0x2c].copy_from_slice(&self.mac);
        p[0x2c..0x30].copy_from_slice(&self.ip.octets());
        p[0x30] = self.peers;
        p[0x31..0x36].copy_from_slice(&self.tail);
        p
    }
}

/// Builds the lighting request (kind `0x11`) that makes the unit send
/// status packets (opus-quad.md, "The lighting request (0x11)").
///
/// `computer_name` goes in as UTF-16BE code units from byte `0x28`, one
/// ASCII byte per unit as the sources show; other characters become `?`
/// and the name is cut at 127 characters so a terminating zero remains.
#[must_use]
pub fn lighting_request(number: u8, computer_name: &str) -> Vec<u8> {
    let mut p = vec![0u8; LIGHTING_REQUEST_LEN];
    p[..MAGIC.len()].copy_from_slice(&MAGIC);
    p[KIND_OFFSET] = kind::LIGHTING_REQUEST;
    write_name(&mut p[0x0b..0x1f], REKORDBOX_NAME);
    p[0x1f] = 0x01;
    p[0x20] = 0x01;
    p[0x21] = number;
    p[0x22..0x24].copy_from_slice(&((LIGHTING_REQUEST_LEN - 0x24) as u16).to_be_bytes());
    p[0x24] = number;
    p[0x25] = 0x01;
    let max_units = (LIGHTING_REQUEST_LEN - 0x28) / 2 - 1;
    for (i, c) in computer_name.chars().take(max_units).enumerate() {
        let b = if c.is_ascii() && !c.is_ascii_control() {
            c as u8
        } else {
            b'?'
        };
        p[0x29 + 2 * i] = b;
    }
    p
}

/// The packet the unit sends to port 50002 of a new rekordbox until it
/// receives our lighting request (kind `0x10`; opus-quad.md, "What the
/// unit sends").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LightingHello {
    /// Device name, bytes `0x0b..0x1f`.
    pub name: String,
    /// Device number byte `0x21` (`09` for deck 1 in the published prefix).
    pub device: u8,
    /// The status fields, when the packet is long enough to carry them
    /// (beat-link reads such packets as status).
    pub status: Option<DeckStatus>,
}

impl LightingHello {
    /// Parses a lighting hello.
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        expect(bytes, kind::LIGHTING_HELLO, LIGHTING_HELLO_MIN_LEN)?;
        let status = if bytes.len() >= STATUS_MIN_LEN {
            Some(DeckStatus::read(bytes))
        } else {
            None
        };
        Ok(Self {
            name: read_name(&bytes[0x0b..0x1f]),
            device: bytes[0x21],
            status,
        })
    }
}

/// Maps the device number in an Opus Quad packet to its deck, 1–4
/// (opus-quad.md, "Deck numbers"): `9..=12` are decks 1–4; `1..=4` are
/// taken as-is; anything else is not a deck.
#[must_use]
pub fn opus_deck(device: u8) -> Option<u8> {
    match device {
        1..=4 => Some(device),
        9..=12 => Some(device - 8),
        _ => None,
    }
}

/// The fields player5 reads from a CDJ status packet (kind `0x0a`, port
/// 50002; opus-quad.md, "Status packet fields").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeckStatus {
    /// Device name, bytes `0x0b..0x1f`.
    pub name: String,
    /// Raw device number, byte `0x21` (`9..=12` on the Opus Quad).
    pub device: u8,
    /// Packet length.
    pub length: usize,
    /// Track ID, bytes `0x2c..0x30` (a Device Library Plus ID on the Opus
    /// Quad).
    pub track_id: u32,
    /// Play state _P1_, byte `0x7b`.
    pub play_state_1: u8,
    /// Status flags _F_, byte `0x89`.
    pub flags: u8,
    /// Play state _P2_, byte `0x8b`.
    pub play_state_2: u8,
    /// Effective pitch _Pitch1_, bytes `0x8d..0x90` (`0x100000` = ±0 %).
    pub pitch: u32,
    /// Track tempo × 100, bytes `0x92..0x94` (`0xffff` = no track).
    pub track_bpm_x100: u16,
    /// _Mm_, byte `0x9e`.
    pub master_meaningful: u8,
    /// Beat counter, bytes `0xa0..0xa4` (`0xffffffff` = unknown).
    pub beat: u32,
    /// Beat within the bar, byte `0xa6` (1–4, `0` = unknown).
    pub bar_beat: u8,
}

/// Neutral pitch, `0x100000` (+0 %).
pub const PITCH_NEUTRAL: u32 = 0x10_0000;

impl DeckStatus {
    /// Parses a status packet.
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        expect(bytes, kind::STATUS, STATUS_MIN_LEN)?;
        Ok(Self::read(bytes))
    }

    /// Reads the fields; `bytes` must be at least [`STATUS_MIN_LEN`] long.
    fn read(bytes: &[u8]) -> Self {
        Self {
            name: read_name(&bytes[0x0b..0x1f]),
            device: bytes[0x21],
            length: bytes.len(),
            track_id: be32(bytes, 0x2c),
            play_state_1: bytes[0x7b],
            flags: bytes[0x89],
            play_state_2: bytes[0x8b],
            pitch: be32(bytes, 0x8c) & 0x00ff_ffff,
            track_bpm_x100: be16(bytes, 0x92),
            master_meaningful: bytes[0x9e],
            beat: be32(bytes, 0xa0),
            bar_beat: bytes[0xa6],
        }
    }

    /// Whether the packet came from an Opus Quad.
    #[must_use]
    pub fn is_opus(&self) -> bool {
        self.name == OPUS_NAME
    }

    /// The deck (1–4) for an Opus Quad packet.
    #[must_use]
    pub fn deck(&self) -> Option<u8> {
        if self.is_opus() {
            opus_deck(self.device)
        } else {
            None
        }
    }

    /// Track tempo, if a track with a tempo is loaded.
    #[must_use]
    pub fn track_bpm(&self) -> Option<f64> {
        match self.track_bpm_x100 {
            0 | 0xffff => None,
            x => Some(f64::from(x) / 100.0),
        }
    }

    /// Effective pitch as a speed multiplier (1.0 = ±0 %).
    #[must_use]
    pub fn pitch_multiplier(&self) -> f64 {
        f64::from(self.pitch) / f64::from(PITCH_NEUTRAL)
    }

    /// Effective tempo: track tempo × pitch.
    #[must_use]
    pub fn effective_bpm(&self) -> Option<f64> {
        let bpm = self.track_bpm()? * self.pitch_multiplier();
        (bpm.is_finite() && bpm > 0.0).then_some(bpm)
    }

    /// Beat counter (beat 1 is the first beat of the track; 0 while paused
    /// at the start), if known.
    #[must_use]
    pub fn beat_number(&self) -> Option<u32> {
        (self.beat != u32::MAX).then_some(self.beat)
    }

    /// Beat within the bar, 1–4, if known.
    #[must_use]
    pub fn beat_within_bar(&self) -> Option<u8> {
        (1..=4).contains(&self.bar_beat).then_some(self.bar_beat)
    }

    /// Whether _P2_ says the playhead is moving (`6a`, `7a`, `9a` or
    /// `fa`).
    #[must_use]
    pub fn p2_moving(&self) -> bool {
        matches!(self.play_state_2, 0x6a | 0x7a | 0x9a | 0xfa)
    }

    /// Whether the deck is playing. The play flag counts when the packet
    /// is long enough to have one; _P2_ "moving" also counts because the
    /// Opus Quad has been seen to clear the flag while playing.
    #[must_use]
    pub fn is_playing(&self) -> bool {
        if self.length >= STATUS_FLAGS_MIN_LEN {
            self.flags & FLAG_PLAYING != 0 || self.p2_moving()
        } else {
            matches!(self.play_state_1, 0x03 | 0x04)
                || (self.play_state_1 == 0x09 && self.p2_moving())
        }
    }

    /// Whether the deck reports itself tempo master.
    #[must_use]
    pub fn is_master(&self) -> bool {
        self.flags & FLAG_MASTER != 0
    }

    /// Whether sync is on.
    #[must_use]
    pub fn is_synced(&self) -> bool {
        self.flags & FLAG_SYNCED != 0
    }

    /// Whether the deck is on air.
    #[must_use]
    pub fn is_on_air(&self) -> bool {
        self.flags & FLAG_ON_AIR != 0
    }

    /// Serialises a status packet of `self.length` bytes (at least
    /// [`STATUS_MIN_LEN`]) for tests and simulators: the fields above plus
    /// the constant bytes of the documented layout, zero elsewhere.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let len = self.length.max(STATUS_MIN_LEN);
        let mut p = vec![0u8; len];
        p[..MAGIC.len()].copy_from_slice(&MAGIC);
        p[KIND_OFFSET] = kind::STATUS;
        write_name(&mut p[0x0b..0x1f], &self.name);
        p[0x1f] = 0x01;
        p[0x20] = 0x03;
        p[0x21] = self.device;
        p[0x22..0x24].copy_from_slice(&((len - 0x24) as u16).to_be_bytes());
        p[0x24] = self.device;
        p[0x26] = 0x01;
        let loaded = self.track_bpm_x100 != 0xffff;
        let moving = self.is_playing();
        p[0x27] = u8::from(moving);
        if loaded {
            p[0x28] = self.device;
            p[0x29] = 0x03;
            p[0x2a] = 0x01;
        }
        p[0x2c..0x30].copy_from_slice(&self.track_id.to_be_bytes());
        p[0x7b] = self.play_state_1;
        p[0x89] = self.flags;
        p[0x8a] = 0xff;
        p[0x8b] = self.play_state_2;
        let pitch = (self.pitch & 0x00ff_ffff).to_be_bytes();
        for at in [0x8c, 0x98, 0xc0, 0xc4] {
            p[at..at + 4].copy_from_slice(&pitch);
        }
        p[0x90..0x92].copy_from_slice(if loaded { &[0x80, 0x00] } else { &[0x7f, 0xff] });
        p[0x92..0x94].copy_from_slice(&self.track_bpm_x100.to_be_bytes());
        p[0x94..0x98].copy_from_slice(&[0x7f, 0xff, 0xff, 0xff]);
        p[0x9d] = match (loaded, moving) {
            (false, _) => 0x00,
            (true, true) => 0x0d,
            (true, false) => 0x01,
        };
        p[0x9e] = self.master_meaningful;
        p[0x9f] = 0xff;
        p[0xa0..0xa4].copy_from_slice(&self.beat.to_be_bytes());
        p[0xa4..0xa6].copy_from_slice(&[0x01, 0xff]);
        p[0xa6] = self.bar_beat;
        p
    }
}

/// A packet received on the announce port (50000).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnnouncePacket {
    /// A keep-alive.
    KeepAlive(KeepAlive),
    /// A well-formed DJ Link packet of another kind (number claims, hellos).
    Other(u8),
}

/// Parses a packet received on the announce port.
pub fn parse_announce(bytes: &[u8]) -> Result<AnnouncePacket, ParseError> {
    match packet_kind(bytes)? {
        kind::KEEP_ALIVE => KeepAlive::parse(bytes).map(AnnouncePacket::KeepAlive),
        k => Ok(AnnouncePacket::Other(k)),
    }
}

/// A packet received on the update port (50002).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdatePacket {
    /// A CDJ status packet.
    Status(DeckStatus),
    /// The unit's lighting hello.
    Hello(LightingHello),
    /// A well-formed DJ Link packet player5 does not use (metadata, mixer
    /// status, ...).
    Other(u8),
}

/// Parses a packet received on the update port.
pub fn parse_update(bytes: &[u8]) -> Result<UpdatePacket, ParseError> {
    match packet_kind(bytes)? {
        kind::STATUS => DeckStatus::parse(bytes).map(UpdatePacket::Status),
        kind::LIGHTING_HELLO => LightingHello::parse(bytes).map(UpdatePacket::Hello),
        k => Ok(UpdatePacket::Other(k)),
    }
}
