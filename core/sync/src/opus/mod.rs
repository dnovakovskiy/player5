//! Opus Quad mode: rekordbox-lighting impersonation that yields BPM and beat
//! count with coarse (±200 ms) phase.
//!
//! PLACEHOLDER: implemented in its own change, with every protocol fact
//! sourced in `docs/protocols/opus-quad.md`. Expected public surface:
//! pure packet parsing/building plus
//! `start(config) -> io::Result<crate::net::SourceHandle>`.
