//! Single-producer, single-consumer event ring.
//!
//! The producer half lives on the control thread, the consumer half on the
//! render thread. [`Consumer::pop`] never blocks, never allocates and never
//! touches anything but two atomics and one slot of the ring, which is what
//! makes it legal inside the audio callback. [`Producer::push`] fails (and
//! hands the event back) instead of waiting when the ring is full.
//!
//! Capacity is rounded up to a power of two so slot indexing is a mask.

#![allow(unsafe_code)]

use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::Event;

struct Shared {
    slots: Box<[UnsafeCell<MaybeUninit<Event>>]>,
    mask: usize,
    /// Next slot the consumer will read. Written only by the consumer.
    head: AtomicUsize,
    /// Next slot the producer will write. Written only by the producer.
    tail: AtomicUsize,
}

// SAFETY: the ring is only ever accessed through exactly one `Producer` and
// exactly one `Consumer` (they are not `Clone`). The producer writes slot
// `tail` and then publishes `tail + 1` with a Release store; the consumer
// Acquire-loads `tail` before reading any slot below it, and publishes `head`
// after it has finished reading. The head/tail indices therefore partition
// the slots into "owned by producer" and "owned by consumer" at every moment,
// and no slot is accessed by both sides at once. `Event` is `Copy`, so no
// destructor runs on the slots.
unsafe impl Sync for Shared {}
unsafe impl Send for Shared {}

/// Control-thread half of the ring.
pub struct Producer {
    shared: Arc<Shared>,
}

/// Render-thread half of the ring.
pub struct Consumer {
    shared: Arc<Shared>,
}

/// Creates a ring holding at least `capacity` events. Allocates; call it at
/// setup time, never from the render thread.
#[must_use]
pub fn event_queue(capacity: usize) -> (Producer, Consumer) {
    let capacity = capacity.max(2).next_power_of_two();
    let slots = (0..capacity)
        .map(|_| UnsafeCell::new(MaybeUninit::uninit()))
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let shared = Arc::new(Shared {
        slots,
        mask: capacity - 1,
        head: AtomicUsize::new(0),
        tail: AtomicUsize::new(0),
    });
    (
        Producer {
            shared: Arc::clone(&shared),
        },
        Consumer { shared },
    )
}

impl Producer {
    /// Enqueues an event. Returns it back if the ring is full.
    pub fn push(&mut self, event: Event) -> Result<(), Event> {
        let s = &*self.shared;
        let tail = s.tail.load(Ordering::Relaxed);
        let head = s.head.load(Ordering::Acquire);
        if tail.wrapping_sub(head) > s.mask {
            return Err(event);
        }
        // SAFETY: slot `tail` is owned by the producer until `tail` is
        // published (see the `Sync` justification above).
        unsafe {
            (*s.slots[tail & s.mask].get()).write(event);
        }
        s.tail.store(tail.wrapping_add(1), Ordering::Release);
        Ok(())
    }

    /// Number of events that can be pushed before the ring is full.
    #[must_use]
    pub fn vacant(&self) -> usize {
        let s = &*self.shared;
        let tail = s.tail.load(Ordering::Relaxed);
        let head = s.head.load(Ordering::Acquire);
        s.mask + 1 - tail.wrapping_sub(head)
    }

    /// Total capacity.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.shared.mask + 1
    }
}

impl Consumer {
    /// Dequeues the oldest event, if any. Real-time safe.
    #[inline]
    pub fn pop(&mut self) -> Option<Event> {
        let s = &*self.shared;
        let head = s.head.load(Ordering::Relaxed);
        let tail = s.tail.load(Ordering::Acquire);
        if head == tail {
            return None;
        }
        // SAFETY: `head < tail`, so the producer has published this slot and
        // will not touch it again until we advance `head`.
        let event = unsafe { (*s.slots[head & s.mask].get()).assume_init_read() };
        s.head.store(head.wrapping_add(1), Ordering::Release);
        Some(event)
    }

    /// Number of events waiting. Real-time safe.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        let s = &*self.shared;
        s.tail
            .load(Ordering::Acquire)
            .wrapping_sub(s.head.load(Ordering::Relaxed))
    }

    /// Whether no events are waiting.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VoiceId;

    fn ev(sample: u64) -> Event {
        Event::trigger(sample, VoiceId::Kick, 1.0)
    }

    #[test]
    fn fifo_order() {
        let (mut p, mut c) = event_queue(4);
        assert!(c.pop().is_none());
        for i in 0..4 {
            p.push(ev(i)).unwrap();
        }
        assert_eq!(p.vacant(), 0);
        assert_eq!(p.push(ev(99)), Err(ev(99)));
        for i in 0..4 {
            assert_eq!(c.pop(), Some(ev(i)));
        }
        assert!(c.pop().is_none());
        assert!(c.is_empty());
    }

    #[test]
    fn wraps_around_many_times() {
        let (mut p, mut c) = event_queue(8);
        let mut next = 0u64;
        // Five in, five out: the indices lap the eight-slot ring every
        // couple of rounds, in every alignment.
        for _ in 0..2_000 {
            for k in 0..5 {
                p.push(ev(next + k)).unwrap();
            }
            assert_eq!(c.len(), 5);
            for k in 0..5 {
                assert_eq!(c.pop(), Some(ev(next + k)));
            }
            assert!(c.is_empty());
            next += 5;
        }
    }

    #[test]
    fn capacity_rounds_up() {
        let (p, _c) = event_queue(100);
        assert_eq!(p.capacity(), 128);
    }

    #[test]
    fn halves_are_send() {
        fn assert_send<T: Send>() {}
        assert_send::<Producer>();
        assert_send::<Consumer>();
    }

    #[test]
    fn cross_thread_stream() {
        let (mut p, mut c) = event_queue(64);
        let n = 50_000u64;
        let producer = std::thread::spawn(move || {
            let mut i = 0;
            while i < n {
                if p.push(ev(i)).is_ok() {
                    i += 1;
                } else {
                    std::thread::yield_now();
                }
            }
        });
        let mut expect = 0;
        while expect < n {
            if let Some(e) = c.pop() {
                assert_eq!(e.sample, expect);
                expect += 1;
            } else {
                std::thread::yield_now();
            }
        }
        producer.join().unwrap();
    }
}
