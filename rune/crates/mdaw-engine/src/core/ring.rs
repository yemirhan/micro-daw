//! A bounded, lock-free single-producer / single-consumer ring.
//!
//! Used for every queue that touches the audio thread (UI → core, MIDI → core, core → control
//! events, core → control garbage). `push` and `pop` never allocate, lock or make syscalls; the
//! storage is allocated once in [`ring`]. Each side is `Send` and owned by exactly one thread, so
//! the only synchronisation is the acquire/release pair on the two indices.

use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Inner<T> {
    slots: Box<[UnsafeCell<MaybeUninit<T>>]>,
    /// Next slot to read (owned by the consumer, read by the producer).
    head: AtomicUsize,
    /// Next slot to write (owned by the producer, read by the consumer).
    tail: AtomicUsize,
}

// SAFETY: a slot is written only by the producer while it is outside [head, tail) and read only by
// the consumer while it is inside; the index stores (release) / loads (acquire) order the accesses.
unsafe impl<T: Send> Sync for Inner<T> {}
unsafe impl<T: Send> Send for Inner<T> {}

impl<T> Inner<T> {
    fn cap(&self) -> usize {
        self.slots.len()
    }
}

impl<T> Drop for Inner<T> {
    fn drop(&mut self) {
        let mut head = *self.head.get_mut();
        let tail = *self.tail.get_mut();
        while head != tail {
            // SAFETY: slots in [head, tail) are initialised and no longer shared.
            unsafe { (*self.slots[head % self.cap()].get()).assume_init_drop() };
            head = head.wrapping_add(1);
        }
    }
}

/// Writing end.
pub struct Producer<T> {
    inner: Arc<Inner<T>>,
}

/// Reading end.
pub struct Consumer<T> {
    inner: Arc<Inner<T>>,
}

/// A ring holding up to `capacity` items (at least 1).
pub fn ring<T>(capacity: usize) -> (Producer<T>, Consumer<T>) {
    let capacity = capacity.max(1);
    let slots = (0..capacity)
        .map(|_| UnsafeCell::new(MaybeUninit::uninit()))
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let inner = Arc::new(Inner {
        slots,
        head: AtomicUsize::new(0),
        tail: AtomicUsize::new(0),
    });
    (
        Producer {
            inner: inner.clone(),
        },
        Consumer { inner },
    )
}

impl<T> Producer<T> {
    /// Push `value`; gives it back when the ring is full.
    pub fn push(&mut self, value: T) -> Result<(), T> {
        let inner = &*self.inner;
        let tail = inner.tail.load(Ordering::Relaxed);
        let head = inner.head.load(Ordering::Acquire);
        if tail.wrapping_sub(head) >= inner.cap() {
            return Err(value);
        }
        // SAFETY: the slot is outside [head, tail): the consumer does not touch it.
        unsafe { (*inner.slots[tail % inner.cap()].get()).write(value) };
        inner.tail.store(tail.wrapping_add(1), Ordering::Release);
        Ok(())
    }

    /// Free slots right now.
    pub fn free(&self) -> usize {
        let inner = &*self.inner;
        let used = inner
            .tail
            .load(Ordering::Relaxed)
            .wrapping_sub(inner.head.load(Ordering::Acquire));
        inner.cap() - used
    }
}

impl<T> Consumer<T> {
    /// Pop the oldest item.
    pub fn pop(&mut self) -> Option<T> {
        let inner = &*self.inner;
        let head = inner.head.load(Ordering::Relaxed);
        let tail = inner.tail.load(Ordering::Acquire);
        if head == tail {
            return None;
        }
        // SAFETY: the slot is inside [head, tail): initialised, and the producer does not touch it.
        let value = unsafe { (*inner.slots[head % inner.cap()].get()).assume_init_read() };
        inner.head.store(head.wrapping_add(1), Ordering::Release);
        Some(value)
    }

    /// Items waiting.
    pub fn len(&self) -> usize {
        let inner = &*self.inner;
        inner
            .tail
            .load(Ordering::Acquire)
            .wrapping_sub(inner.head.load(Ordering::Relaxed))
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fifo_full_and_wraparound() {
        let (mut tx, mut rx) = ring::<u32>(3);
        assert!(tx.push(1).is_ok() && tx.push(2).is_ok() && tx.push(3).is_ok());
        assert_eq!(tx.push(4), Err(4));
        assert_eq!(rx.pop(), Some(1));
        assert!(tx.push(4).is_ok());
        assert_eq!(rx.len(), 3);
        assert_eq!(
            (rx.pop(), rx.pop(), rx.pop(), rx.pop()),
            (Some(2), Some(3), Some(4), None)
        );
        for i in 0..1000 {
            tx.push(i).unwrap();
            assert_eq!(rx.pop(), Some(i));
        }
    }

    #[test]
    fn drops_unread_items() {
        let item = Arc::new(());
        let (mut tx, rx) = ring::<Arc<()>>(4);
        tx.push(item.clone()).unwrap();
        tx.push(item.clone()).unwrap();
        drop((tx, rx));
        assert_eq!(Arc::strong_count(&item), 1);
    }

    #[test]
    fn threads() {
        let (mut tx, mut rx) = ring::<u64>(16);
        let t = std::thread::spawn(move || {
            let mut i = 0;
            while i < 100_000 {
                if tx.push(i).is_ok() {
                    i += 1;
                }
            }
        });
        let mut expect = 0;
        while expect < 100_000 {
            if let Some(v) = rx.pop() {
                assert_eq!(v, expect);
                expect += 1;
            }
        }
        t.join().unwrap();
    }
}
