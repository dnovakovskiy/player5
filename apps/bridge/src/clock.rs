//! The bridge's own view of the followed clock, in microseconds, and the
//! JSON messages built from it (docs/protocols/bridge-websocket.md).

use serde_json::{json, Value};
use sync::net::{DeviceInfo, SourceEvent};
use sync::{ClockSource, FollowerClock, Observation, Phase, Precision};

/// Microseconds on the bridge's monotonic clock (`server_us`).
#[must_use]
pub fn now_us() -> u64 {
    sync::host_time::now_ns() / 1_000
}

/// Protocol name of a precision.
#[must_use]
pub fn precision_name(p: Precision) -> &'static str {
    match p {
        Precision::Exact => "exact",
        Precision::Fine => "fine",
        Precision::Coarse => "coarse",
        Precision::Jittery => "jittery",
    }
}

/// Follows observations and produces timeline messages.
pub struct BridgeClock {
    follower: FollowerClock,
    source: &'static str,
    bar_aligned: bool,
    device: Option<u8>,
    seen: bool,
}

impl BridgeClock {
    /// A clock that has seen nothing yet.
    #[must_use]
    pub fn new(source: &'static str) -> Self {
        Self {
            // "Samples" are microseconds.
            follower: FollowerClock::new(1_000_000.0, 120.0, Precision::Fine),
            source,
            bar_aligned: false,
            device: None,
            seen: false,
        }
    }

    /// Feeds one source event; returns a message to broadcast for events
    /// that are not observations.
    pub fn handle(&mut self, event: SourceEvent, now: u64) -> Option<String> {
        match event {
            SourceEvent::Observation {
                host_ns,
                phase,
                bpm,
                precision,
                device,
            } => {
                if self.follower.precision() != precision {
                    self.follower.set_precision(precision);
                }
                // Another device (a new follow target, a master handoff)
                // has its own bar: take it at once instead of treating it
                // as a phase jump of the old one to be confirmed, which
                // would label the old device's phase with the new number
                // for a while and then jump.
                if self.seen && device != self.device {
                    self.follower.request_resync();
                }
                let obs = Observation {
                    sample: host_ns as f64 / 1_000.0,
                    phase,
                    bpm,
                };
                self.follower.observe(&obs, now as f64);
                let _ = self.follower.take_discontinuity();
                self.bar_aligned = matches!(phase, Phase::Bar(_));
                self.device = device;
                self.seen = true;
                None
            }
            SourceEvent::Devices(devices) => Some(devices_message(&devices)),
            SourceEvent::Status { warning, message } => Some(status_message(
                if warning { "warn" } else { "info" },
                &message,
            )),
        }
    }

    /// Advances lock tracking to `now`.
    pub fn advance(&mut self, now: u64) {
        self.follower.advance(now as f64);
    }

    /// Whether the follower is tracking a live source.
    #[must_use]
    pub fn locked(&self) -> bool {
        self.seen && self.follower.is_locked()
    }

    /// Current tempo.
    #[must_use]
    pub fn bpm(&self) -> f64 {
        self.follower.tempo_bpm()
    }

    /// Beat at server time `t_us`.
    #[must_use]
    pub fn beat_at(&self, t_us: u64) -> f64 {
        self.follower.beat_at_sample(t_us as f64)
    }

    /// The `timeline` message anchored at `now`.
    #[must_use]
    pub fn timeline_message(&self, now: u64) -> String {
        json!({
            "type": "timeline",
            "source": self.source,
            "locked": self.locked(),
            "bpm": self.bpm(),
            "anchor_us": now,
            "anchor_beat": self.beat_at(now),
            "bar_aligned": self.bar_aligned,
            "precision": precision_name(self.follower.precision()),
            "device": self.device,
        })
        .to_string()
    }
}

/// The `hello` message.
#[must_use]
pub fn hello_message(source: &str, now: u64) -> String {
    json!({
        "type": "hello",
        "protocol": 1,
        "app": "player5-bridge",
        "version": env!("CARGO_PKG_VERSION"),
        "server_us": now,
        "source": source,
    })
    .to_string()
}

/// The `devices` message.
#[must_use]
pub fn devices_message(devices: &[DeviceInfo]) -> String {
    let list: Vec<Value> = devices
        .iter()
        .map(|d| {
            json!({
                "number": d.number,
                "name": d.name,
                "address": d.address,
                "kind": d.kind.name(),
                "bpm": d.bpm,
                "playing": d.playing,
                "master": d.master,
                "on_air": d.on_air,
            })
        })
        .collect();
    json!({ "type": "devices", "devices": list }).to_string()
}

/// A `status` message.
#[must_use]
pub fn status_message(level: &str, message: &str) -> String {
    json!({ "type": "status", "level": level, "message": message }).to_string()
}

/// The `pong` reply to a ping (fields echoed as received).
#[must_use]
pub fn pong_message(id: &Value, client_ms: &Value, now: u64) -> String {
    json!({ "type": "pong", "id": id, "client_ms": client_ms, "server_us": now }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeline_follows_observations() {
        let mut c = BridgeClock::new("sim");
        assert!(!c.locked());
        let t0 = 10_000_000u64; // µs
        c.handle(
            SourceEvent::Observation {
                host_ns: t0 * 1_000,
                phase: Phase::Bar(2.0),
                bpm: Some(124.0),
                precision: Precision::Exact,
                device: Some(3),
            },
            t0,
        );
        assert!(c.locked());
        let msg: Value = serde_json::from_str(&c.timeline_message(t0)).unwrap();
        assert_eq!(msg["bpm"], 124.0);
        assert_eq!(msg["bar_aligned"], true);
        assert_eq!(msg["precision"], "exact");
        assert_eq!(msg["device"], 3);
        let beat = msg["anchor_beat"].as_f64().unwrap();
        assert!((beat.rem_euclid(4.0) - 2.0).abs() < 1e-6, "{beat}");
        // One second later the beat advanced by 124/60.
        let later = c.beat_at(t0 + 1_000_000);
        assert!((later - beat - 124.0 / 60.0).abs() < 1e-6);
        // Lock is lost after silence.
        c.advance(t0 + 5_000_000);
        assert!(!c.locked());
    }

    #[test]
    fn a_new_device_is_taken_at_once() {
        let mut c = BridgeClock::new("prolink");
        let obs = |t_us: u64, bar: f64, bpm: f64, device: u8| SourceEvent::Observation {
            host_ns: t_us * 1_000,
            phase: Phase::Bar(bar),
            bpm: Some(bpm),
            precision: Precision::Fine,
            device: Some(device),
        };
        // Device 2 at 120 BPM: a beat every 500 ms, bar position 0..4.
        let mut t = 10_000_000u64;
        for k in 0..8u32 {
            c.handle(obs(t, f64::from(k % 4), 120.0, 2), t);
            c.advance(t);
            t += 500_000;
        }
        assert!(c.locked());
        assert!((c.beat_at(t).rem_euclid(4.0) - 0.0).abs() < 0.01);
        // Device 3 at 128 BPM, at bar position 1.5 right now.
        c.handle(obs(t, 1.5, 128.0, 3), t);
        c.advance(t);
        let msg: Value = serde_json::from_str(&c.timeline_message(t)).unwrap();
        assert_eq!(msg["device"], 3);
        assert_eq!(msg["bpm"], 128.0);
        let bar = msg["anchor_beat"].as_f64().unwrap().rem_euclid(4.0);
        assert!((bar - 1.5).abs() < 0.01, "still on device 2's bar: {bar}");
        assert!(c.locked());
    }

    #[test]
    fn status_and_devices_become_messages() {
        let mut c = BridgeClock::new("prolink");
        let m = c
            .handle(
                SourceEvent::Status {
                    warning: true,
                    message: "no traffic".into(),
                },
                0,
            )
            .unwrap();
        let v: Value = serde_json::from_str(&m).unwrap();
        assert_eq!(v["level"], "warn");
        let m = c.handle(SourceEvent::Devices(vec![]), 0).unwrap();
        let v: Value = serde_json::from_str(&m).unwrap();
        assert_eq!(v["type"], "devices");
        assert!(v["devices"].as_array().unwrap().is_empty());
    }
}
