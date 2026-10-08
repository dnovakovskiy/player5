//! Running clock sources on background threads.
//!
//! Network sources (Pro DJ Link, Opus Quad, Ableton Link) and the built-in
//! simulator all run on their own thread and report [`SourceEvent`]s through
//! a channel, timestamped with [`crate::host_time::now_ns`]. Consumers (the
//! bridge, the native engine's control thread) drain the channel, map host
//! time onto their own clock and feed the observations to a
//! [`crate::FollowerClock`].
//!
//! Nothing here runs on an audio thread.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use crate::follower::{Phase, Precision};
use crate::host_time;

/// What kind of device a network peer is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceKind {
    /// A CDJ/XDJ player.
    Player,
    /// A DJM mixer.
    Mixer,
    /// rekordbox (laptop or lighting).
    Rekordbox,
    /// An all-in-one unit such as the Opus Quad.
    AllInOne,
    /// Anything else, including other virtual devices.
    Other,
}

impl DeviceKind {
    /// Stable lowercase name used in the bridge protocol.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Player => "player",
            Self::Mixer => "mixer",
            Self::Rekordbox => "rekordbox",
            Self::AllInOne => "all-in-one",
            Self::Other => "other",
        }
    }
}

/// A device seen on the network.
#[derive(Clone, Debug, PartialEq)]
pub struct DeviceInfo {
    /// Device (player) number.
    pub number: u8,
    /// Model name as announced.
    pub name: String,
    /// IPv4 address, dotted.
    pub address: String,
    /// What it is.
    pub kind: DeviceKind,
    /// Effective tempo (track BPM × pitch), if known.
    pub bpm: Option<f64>,
    /// Whether it is playing, if known.
    pub playing: Option<bool>,
    /// Whether it is the tempo master, if known.
    pub master: Option<bool>,
    /// Whether its channel is on air at the mixer, if known.
    pub on_air: Option<bool>,
}

/// Something a running source reports.
#[derive(Clone, Debug, PartialEq)]
pub enum SourceEvent {
    /// A timing observation at host time `host_ns`.
    Observation {
        /// When the observed phase was true, host nanoseconds.
        host_ns: u64,
        /// Phase at that moment.
        phase: Phase,
        /// Tempo, if reported.
        bpm: Option<f64>,
        /// How much to trust it.
        precision: Precision,
        /// The device it came from, if any.
        device: Option<u8>,
    },
    /// The set of devices changed.
    Devices(Vec<DeviceInfo>),
    /// Human-readable status for logs and the UI.
    Status {
        /// `true` for problems the user should see.
        warning: bool,
        /// The message.
        message: String,
    },
}

/// Which device a network source should follow.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FollowTarget {
    /// Whoever is tempo master.
    #[default]
    Master,
    /// A specific device number.
    Device(u8),
}

/// A running source. Dropping it stops the thread.
pub struct SourceHandle {
    events: Receiver<SourceEvent>,
    commands: Sender<SourceCommand>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    name: &'static str,
}

/// Instructions to a running source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceCommand {
    /// Change which device to follow.
    Follow(FollowTarget),
}

/// What a source thread gets to work with.
pub struct SourceContext {
    /// Where to send events.
    pub events: Sender<SourceEvent>,
    /// Commands from the owner.
    pub commands: Receiver<SourceCommand>,
    /// Set when the owner wants the thread to exit; check it at least every
    /// 100 ms.
    pub stop: Arc<AtomicBool>,
}

impl SourceContext {
    /// Whether the thread should exit.
    #[must_use]
    pub fn should_stop(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    /// Sends an event; returns `false` if the owner hung up.
    pub fn send(&self, event: SourceEvent) -> bool {
        self.events.send(event).is_ok()
    }
}

impl SourceHandle {
    /// Spawns `body` on a named thread with a fresh context.
    pub fn spawn<F>(name: &'static str, body: F) -> std::io::Result<Self>
    where
        F: FnOnce(SourceContext) + Send + 'static,
    {
        let (etx, erx) = mpsc::channel();
        let (ctx_tx, crx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let ctx = SourceContext {
            events: etx,
            commands: crx,
            stop: Arc::clone(&stop),
        };
        let thread = std::thread::Builder::new()
            .name(format!("player5-{name}"))
            .spawn(move || body(ctx))?;
        Ok(Self {
            events: erx,
            commands: ctx_tx,
            stop,
            thread: Some(thread),
            name,
        })
    }

    /// Short name of the source ("prolink", "opus", "link", "sim").
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Next pending event, without blocking.
    pub fn try_recv(&self) -> Option<SourceEvent> {
        self.events.try_recv().ok()
    }

    /// Waits up to `timeout` for the next event.
    pub fn recv_timeout(&self, timeout: Duration) -> Option<SourceEvent> {
        self.events.recv_timeout(timeout).ok()
    }

    /// Sends a command to the source thread.
    pub fn command(&self, command: SourceCommand) {
        let _ = self.commands.send(command);
    }

    /// Stops the thread and waits for it.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for SourceHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// A built-in source that emits a perfect clock at `bpm`, for demos and
/// end-to-end tests without hardware. Reports a bar-phase observation every
/// `interval`.
pub fn start_simulated(bpm: f64, interval: Duration) -> std::io::Result<SourceHandle> {
    let bpm = bpm.clamp(20.0, 400.0);
    SourceHandle::spawn("sim", move |ctx| {
        let origin = host_time::now_ns();
        let _ = ctx.send(SourceEvent::Status {
            warning: false,
            message: format!("simulated clock at {bpm:.2} BPM"),
        });
        while !ctx.should_stop() {
            let now = host_time::now_ns();
            let beats = (now - origin) as f64 / 1e9 * bpm / 60.0;
            let sent = ctx.send(SourceEvent::Observation {
                host_ns: now,
                phase: Phase::Bar(beats.rem_euclid(4.0)),
                bpm: Some(bpm),
                precision: Precision::Exact,
                device: None,
            });
            if !sent {
                break;
            }
            std::thread::sleep(interval);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simulated_source_reports_and_stops() {
        let src = start_simulated(120.0, Duration::from_millis(5)).unwrap();
        let mut observations = 0;
        for _ in 0..20 {
            if let Some(SourceEvent::Observation { bpm, .. }) =
                src.recv_timeout(Duration::from_millis(200))
            {
                assert_eq!(bpm, Some(120.0));
                observations += 1;
            }
        }
        assert!(observations >= 10);
        src.stop();
    }
}
