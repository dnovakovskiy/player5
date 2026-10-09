//! `player5-bridge`: follows the booth network clock (Pro DJ Link, Opus
//! Quad, Ableton Link, or a simulated clock) and serves tempo / beat /
//! phase to browsers over WebSocket, optionally together with the built web
//! app. Protocol: `docs/protocols/bridge-websocket.md`; design: ADR-0007.
//!
//! Standard library only (plus `serde_json` and the `sync` crate): the
//! WebSocket server, SHA-1 and base64 are implemented here. Nothing in this
//! crate runs on an audio thread.

#![forbid(unsafe_code)]

pub mod base64;
pub mod clock;
pub mod http;
pub mod server;
pub mod sha1;
pub mod sources;
pub mod ws;

pub use server::{start, Config, Server};
pub use sources::{SourceKind, SourceOptions};

/// Default TCP port.
pub const DEFAULT_PORT: u16 = 17_505;
