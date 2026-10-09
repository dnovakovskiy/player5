//! Starting a clock source by name. This is the single place where the
//! bridge meets the `sync` crate's network sources.

use std::net::Ipv4Addr;
use std::time::Duration;

use sync::net::SourceHandle;

/// Which clock to follow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    /// Built-in perfect clock (demos, tests).
    Sim,
    /// Pro DJ Link (CDJ/XDJ players, DJM mixers).
    Prolink,
    /// Opus Quad (rekordbox-lighting impersonation).
    Opus,
    /// Ableton Link.
    Link,
}

impl SourceKind {
    /// Parses a CLI name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "sim" => Self::Sim,
            "prolink" => Self::Prolink,
            "opus" => Self::Opus,
            "link" => Self::Link,
            _ => return None,
        })
    }

    /// Protocol name (the `source` field of timeline messages).
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Sim => "sim",
            Self::Prolink => "prolink",
            Self::Opus => "opus",
            Self::Link => "link",
        }
    }
}

/// Options shared by the sources.
#[derive(Clone, Debug, PartialEq)]
pub struct SourceOptions {
    /// Tempo of the simulated clock.
    pub sim_bpm: f64,
    /// Pro DJ Link device number to claim.
    pub device_number: u8,
    /// Interface address to use on the booth network (discovered if `None`).
    pub interface: Option<Ipv4Addr>,
    /// Listen only; do not announce a virtual device.
    pub passive: bool,
}

impl Default for SourceOptions {
    fn default() -> Self {
        Self {
            sim_bpm: 120.0,
            device_number: 5,
            interface: None,
            passive: false,
        }
    }
}

/// Starts a source. Errors are human-readable.
pub fn start_source(kind: SourceKind, opts: &SourceOptions) -> Result<SourceHandle, String> {
    match kind {
        SourceKind::Sim => sync::net::start_simulated(opts.sim_bpm, Duration::from_millis(10))
            .map_err(|e| format!("cannot start the simulated clock: {e}")),
        SourceKind::Prolink | SourceKind::Opus | SourceKind::Link => Err(format!(
            "the {} source is not available in this build yet",
            kind.name()
        )),
    }
}
