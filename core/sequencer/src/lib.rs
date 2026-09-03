//! The 16-step sequencer.
//!
//! Three pieces, split by which thread they run on:
//!
//! * [`Pattern`] and [`Scheduler`] belong to the **control thread**. The
//!   scheduler walks the pattern ahead of the audio clock and turns steps
//!   into timestamped [`Event`]s ("A Tale of Two Clocks", see
//!   `docs/protocols/lookahead-scheduling.md`).
//! * [`queue`] is the single-producer single-consumer ring that carries
//!   events to the **render thread** without locks or allocation.
//! * [`Event`] is the only thing that crosses the boundary. Nothing audible
//!   is ever triggered directly from the UI or control thread.
//!
//! Only `queue` contains `unsafe` code, with its invariants documented there.

#![deny(unsafe_code)]

mod event;
mod pattern;
pub mod queue;
mod scheduler;

pub use event::{Event, EventKind, MasterParam, ParamTarget, VoiceParam};
pub use pattern::{Pattern, PatternParseError, Step, Track, VoiceId, STEP_COUNT};
pub use scheduler::Scheduler;
