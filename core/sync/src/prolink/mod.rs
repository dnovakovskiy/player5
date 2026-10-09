//! Pro DJ Link (CDJ-3000 / XDJ players, DJM mixers): packet parsing and
//! building, joining the network as a virtual player, and the network
//! source that follows the booth's tempo master.
//!
//! Every protocol fact used here is digested, with a source link per fact,
//! in `docs/protocols/pro-dj-link.md`; real hardware captures used as test
//! fixtures live in `docs/protocols/fixtures/prolink/`.
//!
//! - [`packets`]: pure, never-panicking `parse_*` functions and `build_*`
//!   functions for every packet the source uses (re-exported here).
//! - [`start`]: the running source. It listens on UDP 50000–50002, joins as
//!   device 5 named "player5" (unless [`ProlinkConfig::passive`]), keeps a
//!   device table and reports a bar-phase observation for every beat of the
//!   followed device, timestamped on arrival.
//!
//! Precision is [`crate::Precision::Fine`] throughout: beat packets mark
//! the beat to within network jitter. CDJ-3000 precise position packets
//! give the playhead in milliseconds but no beat information; turning them
//! into phase needs the track's beat grid (a metadata query this source
//! does not make), so they only refine tempo and the device list.

pub mod packets;

mod devices;
mod join;
mod source;

#[cfg(test)]
mod tests;

pub use packets::{
    build_assignment, build_assignment_finished, build_assignment_intention,
    build_assignment_request, build_beat_packet, build_cdj_status, build_claim_stage1,
    build_claim_stage2, build_claim_stage3, build_hello, build_keep_alive, build_mixer_status,
    build_number_in_use, build_on_air, build_precise_position, classify_device, device_name,
    parse_assignment, parse_assignment_finished, parse_beat, parse_cdj_status, parse_keep_alive,
    parse_mixer_status, parse_number_claim, parse_number_in_use, parse_on_air,
    parse_precise_position, percent_to_pitch, pitch_to_multiplier, pitch_to_percent, BeatPacket,
    CdjStatus, KeepAlive, MixerStatus, NumberClaim, NumberInUse, OnAir, PacketKind, ParseError,
    Port, PrecisePosition, ANNOUNCE_PORT, BEAT_PORT, MAGIC, STATUS_PORT,
};
pub use source::{
    default_broadcast, discover_interface, fallback_mac, start, start_with_sockets, ProlinkConfig,
    ProlinkPorts, ProlinkSockets,
};
