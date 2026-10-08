//! The one piece of state the render thread publishes for the control
//! thread: which sample it is at, and the host time that sample is heard.
//!
//! A sequence lock over plain atomics: the renderer writes without ever
//! waiting, the control thread retries on a torn read. No locks, no
//! allocation, so it is legal on the render path.

use std::sync::atomic::{fence, AtomicU64, Ordering};

/// Render position paired with platform host time.
#[derive(Debug, Default)]
pub struct SharedTiming {
    seq: AtomicU64,
    position: AtomicU64,
    host_ticks: AtomicU64,
}

/// One consistent snapshot of [`SharedTiming`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimingSnapshot {
    /// Sample position of the start of the most recent block.
    pub position: u64,
    /// Platform host-time ticks at which that sample is output (`0` when the
    /// host did not supply one).
    pub host_ticks: u64,
}

impl SharedTiming {
    /// Publishes a block start. Render thread only (single writer).
    #[inline]
    pub fn publish(&self, position: u64, host_ticks: u64) {
        let s = self.seq.load(Ordering::Relaxed);
        self.seq.store(s.wrapping_add(1), Ordering::Relaxed);
        fence(Ordering::Release);
        self.position.store(position, Ordering::Relaxed);
        self.host_ticks.store(host_ticks, Ordering::Relaxed);
        self.seq.store(s.wrapping_add(2), Ordering::Release);
    }

    /// Reads a consistent snapshot. Never blocks the writer.
    #[must_use]
    pub fn read(&self) -> TimingSnapshot {
        loop {
            let s1 = self.seq.load(Ordering::Acquire);
            if s1 % 2 == 1 {
                std::hint::spin_loop();
                continue;
            }
            let position = self.position.load(Ordering::Relaxed);
            let host_ticks = self.host_ticks.load(Ordering::Relaxed);
            fence(Ordering::Acquire);
            if self.seq.load(Ordering::Relaxed) == s1 {
                return TimingSnapshot {
                    position,
                    host_ticks,
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn snapshots_are_never_torn() {
        let t = Arc::new(SharedTiming::default());
        let w = Arc::clone(&t);
        let writer = std::thread::spawn(move || {
            for i in 1..200_000u64 {
                w.publish(i, i * 10);
            }
        });
        for _ in 0..200_000 {
            let s = t.read();
            assert_eq!(s.host_ticks, s.position * 10);
        }
        writer.join().unwrap();
    }
}
