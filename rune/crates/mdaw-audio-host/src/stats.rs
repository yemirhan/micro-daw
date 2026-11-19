//! Lock-free counters the audio thread updates and the control side reads.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering, Ordering::Relaxed};

/// Live stream statistics (atomics; written by the audio thread, except the render-panic latch
/// which the control side clears).
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
    /// Render callback panics caught on the audio thread.
    render_panics: AtomicU64,
    /// Latched by a render panic: the render callback is no longer called (silence) until the
    /// control side clears it.
    render_panicked: AtomicBool,
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
    /// Render callback panics caught so far (each one latches [`Self::render_silenced`]).
    pub render_panics: u64,
    /// A render panic latched the output to silence (cleared by `clear_render_panic`).
    pub render_silenced: bool,
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

    pub(crate) fn record_render_panic(&self) {
        self.render_panics.fetch_add(1, Relaxed);
        self.render_panicked.store(true, Ordering::Release);
    }

    /// True after a render panic until [`Self::clear_render_panic`].
    pub fn render_panicked(&self) -> bool {
        self.render_panicked.load(Ordering::Acquire)
    }

    /// Let the audio thread call the render callback again after a panic (only once the
    /// render state is known to be consistent again, e.g. after rebuilding it).
    pub fn clear_render_panic(&self) {
        self.render_panicked.store(false, Ordering::Release);
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
            render_panics: self.render_panics.load(Relaxed),
            render_silenced: self.render_panicked(),
        }
    }
}
