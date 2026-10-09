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
    /// For tests: Pro DJ Link listens on `base`, `base + 1`, `base + 2`
    /// (announce, beat, status) on 127.0.0.1 instead of 50000–50002 on all
    /// interfaces.
    pub prolink_port_base: Option<u16>,
}

impl Default for SourceOptions {
    fn default() -> Self {
        Self {
            sim_bpm: 120.0,
            device_number: 5,
            interface: None,
            passive: false,
            prolink_port_base: None,
        }
    }
}

/// Starts a source. Errors are human-readable.
pub fn start_source(kind: SourceKind, opts: &SourceOptions) -> Result<SourceHandle, String> {
    match kind {
        SourceKind::Sim => sync::net::start_simulated(opts.sim_bpm, Duration::from_millis(10))
            .map_err(|e| format!("cannot start the simulated clock: {e}")),
        SourceKind::Prolink => {
            let mut config = sync::prolink::ProlinkConfig {
                device_number: opts.device_number,
                interface: opts.interface,
                passive: opts.passive,
                ..sync::prolink::ProlinkConfig::default()
            };
            if let Some(base) = opts.prolink_port_base {
                config.ports = sync::prolink::ProlinkPorts {
                    announce: base,
                    beat: base.saturating_add(1),
                    status: base.saturating_add(2),
                };
                config.listen_address = std::net::Ipv4Addr::LOCALHOST;
                config.broadcast = Some(std::net::Ipv4Addr::LOCALHOST);
            }
            sync::prolink::start(config)
                .map_err(|e| format!("cannot start Pro DJ Link (UDP 50000-50002): {e}"))
        }
        SourceKind::Opus => {
            let config = sync::opus::OpusConfig {
                interface: opts.interface,
                ..sync::opus::OpusConfig::default()
            };
            sync::opus::start(config).map_err(|e| format!("cannot start Opus Quad mode: {e}"))
        }
        SourceKind::Link => start_link(opts),
    }
}

#[cfg(feature = "ableton-link")]
fn start_link(opts: &SourceOptions) -> Result<SourceHandle, String> {
    sync::link::start(sync::link::LinkConfig {
        initial_bpm: opts.sim_bpm,
        ..sync::link::LinkConfig::default()
    })
    .map_err(|e| format!("cannot start Ableton Link: {e}"))
}

#[cfg(not(feature = "ableton-link"))]
fn start_link(_opts: &SourceOptions) -> Result<SourceHandle, String> {
    Err("this bridge was built without Ableton Link; rebuild with \
         `cargo build -p player5-bridge --features ableton-link` (see ADR-0005: GPL)"
        .to_string())
}
