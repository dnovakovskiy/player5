//! Clock sources and the timeline that maps beats to audio samples.
//!
//! The sequencer only ever talks to the [`ClockSource`] trait. The internal
//! clock implements it directly; every external source (Ableton Link, Pro
//! DJ Link, Opus Quad, MIDI clock, the browser bridge, tap) produces
//! [`Observation`]s that steer a [`FollowerClock`], which implements it
//! too. See ADR-0001 and ADR-0006.
//!
//! Beats are continuous `f64` values on an unbounded timeline; beat `0` is
//! wherever the source's grid puts it. Samples are the audio device's sample
//! clock, also as `f64` so sub-sample positions survive until the scheduler
//! rounds them.
//!
//! This crate runs on the control thread and on background network
//! threads. Nothing here is called from the render callback. The only
//! `unsafe` is the `mach_absolute_time` call in [`host_time`].

#![deny(unsafe_code)]

mod clock;
pub mod follower;
pub mod host_time;
mod internal;
#[cfg(feature = "ableton-link")]
pub mod link;
pub mod midi;
pub mod net;
pub mod opus;
pub mod prolink;
pub mod tap;

pub use clock::{AdjustedClock, ClockControls, ClockSource};
pub use follower::{FollowerClock, Observation, Phase, Precision};
pub use internal::InternalClock;
pub use midi::{MidiClockFollower, MidiMessage};
pub use tap::TapTempo;
