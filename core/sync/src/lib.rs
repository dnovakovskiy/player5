//! Clock sources and the timeline that maps beats to audio samples.
//!
//! Every tempo source (`Internal`, Ableton Link, Pro DJ Link, Opus Quad, MIDI
//! clock, the browser bridge, tap) implements [`ClockSource`]. The sequencer
//! only ever talks to that trait, so swapping the source never touches the
//! scheduling code. Sources with jitter or coarse phase feed a PLL before
//! they reach the sequencer; that layer arrives with the first network
//! source (session 3). See ADR-0001.
//!
//! Beats are continuous `f64` values on an unbounded timeline; beat `0` is
//! wherever the source's grid puts it. Samples are the audio device's sample
//! clock, also as `f64` so sub-sample positions survive until the scheduler
//! rounds them.
//!
//! This crate runs on the control thread. Nothing here is called from the
//! render callback.

#![forbid(unsafe_code)]

mod clock;
mod internal;

pub use clock::{AdjustedClock, ClockControls, ClockSource};
pub use internal::InternalClock;
