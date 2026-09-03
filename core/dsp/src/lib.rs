//! Synthesized drum voices and the master output section.
//!
//! Everything in this crate may run on the audio render thread, so the rules
//! from `CLAUDE.md` apply to every function here: no allocation, no locks, no
//! syscalls, no logging, no `std` math that could differ between platforms
//! (see [`math`] and ADR-0002). The crate holds no `unsafe` code.
//!
//! Voices are calibrated so that a full-velocity hit at `level = 1.0` peaks at
//! roughly −6 dBFS, the product's default master headroom.

#![forbid(unsafe_code)]

pub mod kick;
pub mod master;
pub mod math;
pub mod voice;

pub use kick::{Kick, KickParams};
pub use master::Master;
pub use voice::Voice;

/// Sample rates the voices are tuned for. Any positive rate works; these are
/// the ones exercised by the golden-master tests.
pub const SUPPORTED_SAMPLE_RATES: [f32; 3] = [44_100.0, 48_000.0, 96_000.0];
