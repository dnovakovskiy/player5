//! Synthesized drum voices and the master output section.
//!
//! Everything in this crate may run on the audio render thread, so the rules
//! from `CLAUDE.md` apply to every function here: no allocation, no locks, no
//! syscalls, no logging, no `std` math that could differ between platforms
//! (see [`math`] and ADR-0002). The crate holds no `unsafe` code.
//!
//! Calibration: the kick at full velocity and `level = 1.0` peaks at roughly
//! −6 dBFS, the product's default master headroom. Every other voice peaks
//! lower (see each module's `CALIBRATION`), so that a busy full-kit pattern
//! at default levels still peaks near −6 dBFS.

#![forbid(unsafe_code)]

pub mod blocks;
pub mod clap;
pub mod cowbell;
pub mod hats;
pub mod kick;
pub mod kit;
pub mod master;
pub mod math;
pub mod params;
pub mod rim;
pub mod snare;
pub mod tom;
pub mod voice;

pub use clap::Clap;
pub use cowbell::Cowbell;
pub use hats::{ClosedHat, OpenHat};
pub use kick::{Kick, KickParams};
pub use kit::{slot, Kit, KIT_HEADROOM, VOICE_COUNT};
pub use master::Master;
pub use params::{Param, VoiceParams};
pub use rim::Rim;
pub use snare::Snare;
pub use tom::{Tom, TomRange};
pub use voice::Voice;

/// Sample rates the voices are tuned for. Any positive rate works; these are
/// the ones exercised by the golden-master tests.
pub const SUPPORTED_SAMPLE_RATES: [f32; 3] = [44_100.0, 48_000.0, 96_000.0];
