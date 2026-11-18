//! Lock-free counters the audio thread updates and the control side reads.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering::Relaxed};

/// Live stream statistics (all relaxed atomics; written only by the audio thread).
#[derive(Debug, Default)]
pub struct HostStats {
    callbacks: AtomicU64,
    frames: AtomicU64,
    last_frames: AtomicU32,
    min_frames: AtomicU32,
    max_frames: AtomicU32,
    /// Callback → playback time of the last callback (ns), 0 when the backend gives none.
    output_latency_ns: AtomicU64,
    /// Blocks rendered silent because the render callback was locked elsewhere.
    skipped: AtomicU64,
}

/// A copy of [`HostStats`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StatsSnapshot {
    pub callbacks: u64,
    pub frames: u64,
    /// Frames in the last device callback (the actual buffer size).
    pub last_frames: u32,
    /// Smallest / largest callback seen (0 before the first callback).
    pub min_frames: u32,
    pub max_frames: u32,
    /// Device-reported output latency of the last callback (callback → playback), in ns.
    pub output_latency_ns: u64,
    pub skipped: u64,
}

impl HostStats {
    pub(crate) fn record_callback(&self, frames: usize) {
        let f = frames.min(u32::MAX as usize) as u32;
        self.callbacks.fetch_add(1, Relaxed);
        self.frames.fetch_add(frames as u64, Relaxed);
        self.last_frames.store(f, Relaxed);
        let min = self.min_frames.load(Relaxed);
        if min == 0 || f < min {
            self.min_frames.store(f, Relaxed);
        }
        if f > self.max_frames.load(Relaxed) {
            self.max_frames.store(f, Relaxed);
        }
    }

    pub(crate) fn record_latency_ns(&self, ns: u64) {
        self.output_latency_ns.store(ns, Relaxed);
    }

    pub(crate) fn record_skip(&self) {
        self.skipped.fetch_add(1, Relaxed);
    }

    /// Reset the per-stream buffer-size range (on reopen).
    pub(crate) fn reset_range(&self) {
        self.min_frames.store(0, Relaxed);
        self.max_frames.store(0, Relaxed);
    }

    pub fn snapshot(&self) -> StatsSnapshot {
        StatsSnapshot {
            callbacks: self.callbacks.load(Relaxed),
            frames: self.frames.load(Relaxed),
            last_frames: self.last_frames.load(Relaxed),
            min_frames: self.min_frames.load(Relaxed),
            max_frames: self.max_frames.load(Relaxed),
            output_latency_ns: self.output_latency_ns.load(Relaxed),
            skipped: self.skipped.load(Relaxed),
        }
    }
}
