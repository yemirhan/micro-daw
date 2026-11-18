//! What the core publishes for the other threads: a **consistent clock snapshot** (seqlock) and
//! a few independent counters / meters.
//!
//! **Snapshot.** `{frames, transport beat, bpm, phase, generation, wrap count, wrapped beats,
//! wall-clock anchor}` are written together by the audio thread after every render quantum (and
//! after every device callback, for the anchor) under a sequence counter, and read together with
//! [`Shared::snapshot`]: a reader never sees, say, a new generation with the old beat, or frames
//! with a stale wrap count. The writer never waits (single writer: the audio thread); a reader
//! retries while a write is in progress (a few hundred ns at most).
//!
//! **Wall-clock anchor** (plan §3: "Clock shared atomically: frames rendered + the wall-clock
//! instant of that callback"). [`super::Core::process_timed`] records, per device callback, the
//! first frame it hands out, the `Instant` of the callback and the callback → speaker latency.
//! From it:
//! - [`ClockSnapshot::now_extrapolated`]: a continuous "now" in context seconds (the render
//!   clock between callbacks; a smooth playhead base, phase 7);
//! - [`ClockSnapshot::audio_time_for_instant`]: the context time at which an event that happened
//!   at a wall-clock instant (a MIDI timestamp, phase 5) should be rendered so every event gets the
//!   same latency (one callback period), i.e. jitter-free timing;
//! - [`ClockSnapshot::heard_time`]: the context time being heard at an instant (output latency
//!   subtracted).
//!
//! Offline renders (export, tests) have no anchor: those methods fall back to `frames / rate`.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering, fence};
use std::time::{Duration, Instant};

use super::api::Phase;
use crate::contract::RENDER_QUANTUM;

/// No anchor published yet.
const NO_ANCHOR: u64 = u64::MAX;

/// Where the render clock was at a device callback.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClockAnchor {
    /// First frame handed out in that callback.
    pub frame: u64,
    /// When the callback started.
    pub instant: Instant,
    /// Callback → speaker time of `frame` (0 when the backend gives none).
    pub output_latency: Duration,
    /// Frames handed out in that callback.
    pub block_frames: u32,
}

/// A consistent copy of the core's clock (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClockSnapshot {
    pub sample_rate: u32,
    /// Frames rendered: the start of the next quantum ("now" = frames / rate).
    pub frames: u64,
    /// Transport beat at `frames` (getPosition).
    pub beat: f64,
    pub bpm: f64,
    pub phase: Phase,
    pub generation: u64,
    pub wrap_count: u64,
    /// Sum of the loop lengths wrapped so far (beats).
    pub wrapped_beats: f64,
    pub anchor: Option<ClockAnchor>,
}

impl ClockSnapshot {
    fn sr(&self) -> f64 {
        self.sample_rate.max(1) as f64
    }

    /// Core time in context seconds (start of the next render quantum).
    pub fn now(&self) -> f64 {
        self.frames as f64 / self.sr()
    }

    /// Continuous render-clock time at `at`: the anchor frame's time plus the wall time since its
    /// callback, never before the anchor and at most two callback periods past it (a stalled
    /// device does not run the clock away). Without an anchor: [`Self::now`].
    pub fn now_extrapolated(&self, at: Instant) -> f64 {
        let Some(a) = self.anchor else {
            return self.now();
        };
        let base = a.frame as f64 / self.sr();
        let period =
            (a.block_frames.max(1) as f64 / self.sr()).max(RENDER_QUANTUM as f64 / self.sr());
        let elapsed = at.saturating_duration_since(a.instant).as_secs_f64();
        base + elapsed.min(2.0 * period)
    }

    /// The context time to render an event that happened at `instant` (e.g. a MIDI timestamp):
    /// the anchor's mapping plus one callback period and one render quantum, so every event
    /// keeps its relative timing (constant latency, no jitter) and lands at or after the next
    /// quantum the core renders. Without an anchor: [`Self::now`].
    pub fn audio_time_for_instant(&self, instant: Instant) -> f64 {
        let Some(a) = self.anchor else {
            return self.now();
        };
        let sr = self.sr();
        let base = a.frame as f64 / sr;
        let offset = if instant >= a.instant {
            instant.duration_since(a.instant).as_secs_f64()
        } else {
            -a.instant.duration_since(instant).as_secs_f64()
        };
        base + offset + (a.block_frames as f64 + RENDER_QUANTUM as f64) / sr
    }

    /// Output latency of the last callback (s).
    pub fn output_latency(&self) -> f64 {
        self.anchor.map_or(0.0, |a| a.output_latency.as_secs_f64())
    }

    /// The context time being heard at `at` ([`Self::now_extrapolated`] minus the output
    /// latency).
    pub fn heard_time(&self, at: Instant) -> f64 {
        self.now_extrapolated(at) - self.output_latency()
    }
}

/// Published by the core (see the module docs).
pub struct Shared {
    pub sample_rate: u32,
    epoch: Instant,
    seq: AtomicU64,
    frames: AtomicU64,
    beat_bits: AtomicU64,
    bpm_bits: AtomicU64,
    phase: AtomicU64,
    generation: AtomicU64,
    wrap_count: AtomicU64,
    wrapped_beats_bits: AtomicU64,
    anchor_frame: AtomicU64,
    anchor_ns: AtomicU64,
    latency_ns: AtomicU64,
    block_frames: AtomicU64,
    /// Mean square of each slot's output in the last quantum (f32 bits).
    levels: Vec<AtomicU32>,
    repeating_pads: AtomicU64,
    dropped_events: AtomicU64,
    critical_overruns: AtomicU64,
    leaked_garbage: AtomicU64,
    /// Per producer: the sequence number of the last command the core applied.
    applied_seq: [AtomicU64; 2],
}

/// The transport part of a publish.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TransportPub {
    pub frames: u64,
    pub beat: f64,
    pub bpm: f64,
    pub phase: Phase,
    pub generation: u64,
    pub wraps: u64,
    pub wrapped_beats: f64,
}

impl Shared {
    pub(crate) fn new(sample_rate: u32, bpm: f64, slots: usize) -> Self {
        Self {
            sample_rate,
            epoch: Instant::now(),
            seq: AtomicU64::new(0),
            frames: AtomicU64::new(0),
            beat_bits: AtomicU64::new(0f64.to_bits()),
            bpm_bits: AtomicU64::new(bpm.to_bits()),
            phase: AtomicU64::new(Phase::Stopped as u64),
            generation: AtomicU64::new(0),
            wrap_count: AtomicU64::new(0),
            wrapped_beats_bits: AtomicU64::new(0f64.to_bits()),
            anchor_frame: AtomicU64::new(0),
            anchor_ns: AtomicU64::new(NO_ANCHOR),
            latency_ns: AtomicU64::new(0),
            block_frames: AtomicU64::new(0),
            levels: (0..slots).map(|_| AtomicU32::new(0)).collect(),
            repeating_pads: AtomicU64::new(0),
            dropped_events: AtomicU64::new(0),
            critical_overruns: AtomicU64::new(0),
            leaked_garbage: AtomicU64::new(0),
            applied_seq: [AtomicU64::new(0), AtomicU64::new(0)],
        }
    }

    /// Publish the clock (single writer: the audio thread). `anchor`: (frame, instant,
    /// latency, block frames) of the last device callback, if any.
    pub(crate) fn publish(&self, t: TransportPub, anchor: Option<(u64, Instant, Duration, u32)>) {
        let s = self.seq.load(Ordering::Relaxed);
        self.seq.store(s.wrapping_add(1), Ordering::Relaxed);
        fence(Ordering::Release);
        self.frames.store(t.frames, Ordering::Relaxed);
        self.beat_bits.store(t.beat.to_bits(), Ordering::Relaxed);
        self.bpm_bits.store(t.bpm.to_bits(), Ordering::Relaxed);
        self.phase.store(t.phase as u64, Ordering::Relaxed);
        self.generation.store(t.generation, Ordering::Relaxed);
        self.wrap_count.store(t.wraps, Ordering::Relaxed);
        self.wrapped_beats_bits
            .store(t.wrapped_beats.to_bits(), Ordering::Relaxed);
        if let Some((frame, instant, latency, block)) = anchor {
            let ns = instant.saturating_duration_since(self.epoch).as_nanos();
            self.anchor_frame.store(frame, Ordering::Relaxed);
            self.anchor_ns
                .store(ns.min(NO_ANCHOR as u128 - 1) as u64, Ordering::Relaxed);
            self.latency_ns.store(
                latency.as_nanos().min(u64::MAX as u128) as u64,
                Ordering::Relaxed,
            );
            self.block_frames.store(block as u64, Ordering::Relaxed);
        }
        self.seq.store(s.wrapping_add(2), Ordering::Release);
    }

    /// A consistent copy of the clock (retries while the audio thread is writing).
    pub fn snapshot(&self) -> ClockSnapshot {
        loop {
            let s1 = self.seq.load(Ordering::Acquire);
            if s1 & 1 == 1 {
                std::hint::spin_loop();
                continue;
            }
            let frames = self.frames.load(Ordering::Relaxed);
            let beat = f64::from_bits(self.beat_bits.load(Ordering::Relaxed));
            let bpm = f64::from_bits(self.bpm_bits.load(Ordering::Relaxed));
            let phase = Phase::from_u8(self.phase.load(Ordering::Relaxed) as u8);
            let generation = self.generation.load(Ordering::Relaxed);
            let wrap_count = self.wrap_count.load(Ordering::Relaxed);
            let wrapped_beats = f64::from_bits(self.wrapped_beats_bits.load(Ordering::Relaxed));
            let anchor_frame = self.anchor_frame.load(Ordering::Relaxed);
            let anchor_ns = self.anchor_ns.load(Ordering::Relaxed);
            let latency_ns = self.latency_ns.load(Ordering::Relaxed);
            let block_frames = self.block_frames.load(Ordering::Relaxed);
            fence(Ordering::Acquire);
            if self.seq.load(Ordering::Relaxed) != s1 {
                continue;
            }
            let anchor = (anchor_ns != NO_ANCHOR).then(|| ClockAnchor {
                frame: anchor_frame,
                instant: self.epoch + Duration::from_nanos(anchor_ns),
                output_latency: Duration::from_nanos(latency_ns),
                block_frames: block_frames.min(u32::MAX as u64) as u32,
            });
            return ClockSnapshot {
                sample_rate: self.sample_rate,
                frames,
                beat,
                bpm,
                phase,
                generation,
                wrap_count,
                wrapped_beats,
                anchor,
            };
        }
    }

    pub(crate) fn publish_level(&self, slot: usize, mean_square: f32) {
        if let Some(l) = self.levels.get(slot) {
            l.store(mean_square.to_bits(), Ordering::Relaxed);
        }
    }

    pub(crate) fn publish_repeating(&self, mask: u64) {
        self.repeating_pads.store(mask, Ordering::Relaxed);
    }

    pub(crate) fn count_dropped_event(&self) {
        self.dropped_events.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn count_critical_overrun(&self) {
        self.critical_overruns.fetch_add(1, Ordering::Release);
    }

    pub(crate) fn count_leaks(&self, n: u64) {
        if n > 0 {
            self.leaked_garbage.fetch_add(n, Ordering::Relaxed);
        }
    }

    pub(crate) fn set_applied(&self, producer: usize, seq: u64) {
        if let Some(a) = self.applied_seq.get(producer) {
            a.store(seq, Ordering::Release);
        }
    }

    /// Sequence number of the last command of `producer` the core applied.
    pub(crate) fn applied(&self, producer: usize) -> u64 {
        self.applied_seq
            .get(producer)
            .map_or(0, |a| a.load(Ordering::Acquire))
    }

    // --- Convenience reads (each one consistent on its own; use `snapshot` for several) -------

    pub fn frames(&self) -> u64 {
        self.snapshot().frames
    }
    /// Core time in context seconds (start of the next render quantum).
    pub fn now(&self) -> f64 {
        self.snapshot().now()
    }
    /// [`ClockSnapshot::now_extrapolated`] at `Instant::now()`.
    pub fn now_extrapolated(&self) -> f64 {
        self.snapshot().now_extrapolated(Instant::now())
    }
    /// [`ClockSnapshot::audio_time_for_instant`].
    pub fn audio_time_for_instant(&self, instant: Instant) -> f64 {
        self.snapshot().audio_time_for_instant(instant)
    }
    /// Playhead (`getPosition`): the transport beat at [`Shared::now`].
    pub fn playhead_beat(&self) -> f64 {
        self.snapshot().beat
    }
    pub fn bpm(&self) -> f64 {
        self.snapshot().bpm
    }
    pub fn phase(&self) -> Phase {
        self.snapshot().phase
    }
    pub fn generation(&self) -> u64 {
        self.snapshot().generation
    }
    pub fn wrap_count(&self) -> u64 {
        self.snapshot().wrap_count
    }
    /// Sum of the loop lengths (beats) wrapped so far (see `PadCapture::wrapped_beats`).
    pub fn wrapped_beats(&self) -> f64 {
        self.snapshot().wrapped_beats
    }
    /// Mean square of a slot's output (after gain) over the last render quantum.
    pub fn slot_mean_square(&self, slot: usize) -> f32 {
        self.levels
            .get(slot)
            .map_or(0.0, |l| f32::from_bits(l.load(Ordering::Relaxed)))
    }
    /// Bit per pad: held and repeating (`NoteRepeat.isRepeating`).
    pub fn repeating_pads(&self) -> u64 {
        self.repeating_pads.load(Ordering::Relaxed)
    }
    /// Visual events (pad flashes, clicks) lost because their ring was full.
    pub fn dropped_events(&self) -> u64 {
        self.dropped_events.load(Ordering::Relaxed)
    }
    /// Critical events (captures, loop wraps, punch closes, transport) lost because their ring
    /// was full. Latched: the control side compares it with the value it last handled.
    pub fn critical_overruns(&self) -> u64 {
        self.critical_overruns.load(Ordering::Acquire)
    }
    /// Garbage the audio thread had to leak instead of freeing (every retirement path full).
    pub fn leaked_garbage(&self) -> u64 {
        self.leaked_garbage.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    fn tp(i: u64) -> TransportPub {
        TransportPub {
            frames: i * 128,
            beat: i as f64 * 0.5,
            bpm: 120.0,
            phase: if i.is_multiple_of(2) {
                Phase::Started
            } else {
                Phase::Pending
            },
            generation: i,
            wraps: i / 3,
            wrapped_beats: (i / 3) as f64 * 4.0,
        }
    }

    /// Review finding 6: a reader must never mix two publishes. A writer thread publishes
    /// tuples whose fields are all functions of one counter; the reader checks them.
    #[test]
    fn snapshot_is_never_torn() {
        let shared = Arc::new(Shared::new(48_000, 120.0, 1));
        let stop = Arc::new(AtomicBool::new(false));
        let (s, st) = (shared.clone(), stop.clone());
        let writer = std::thread::spawn(move || {
            let mut i = 1u64;
            while !st.load(Ordering::Relaxed) {
                s.publish(
                    tp(i),
                    Some((i * 128, s.epoch, Duration::from_nanos(i), i as u32)),
                );
                i += 1;
            }
            i
        });
        let mut checked = 0u64;
        // A control: the same reads done field by field (the pre-fix API) tear.
        let mut torn_fieldwise = 0u64;
        let deadline = Instant::now() + Duration::from_millis(300);
        while Instant::now() < deadline {
            let snap = shared.snapshot();
            let i = snap.generation;
            if i > 0 {
                let want = tp(i);
                assert_eq!(snap.frames, want.frames);
                assert_eq!(snap.beat, want.beat);
                assert_eq!(snap.phase, want.phase);
                assert_eq!(
                    (snap.wrap_count, snap.wrapped_beats),
                    (want.wraps, want.wrapped_beats)
                );
                let a = snap.anchor.unwrap();
                assert_eq!((a.frame, a.block_frames as u64), (want.frames, i));
                checked += 1;
            }
            let g = shared.generation.load(Ordering::Relaxed);
            let f = shared.frames.load(Ordering::Relaxed);
            if g > 0 && f != g * 128 {
                torn_fieldwise += 1;
            }
        }
        stop.store(true, Ordering::Relaxed);
        let writes = writer.join().unwrap();
        assert!(
            checked > 1000 && writes > 1000,
            "{checked} reads, {writes} writes"
        );
        eprintln!(
            "seqlock: {checked} consistent snapshots; field-by-field control tore {torn_fieldwise} times"
        );
    }

    #[test]
    fn anchor_maps_wall_clock_to_context_time() {
        let shared = Shared::new(48_000, 120.0, 1);
        let snap = shared.snapshot();
        assert_eq!(snap.anchor, None);
        let t = Instant::now();
        assert_eq!(snap.now_extrapolated(t), 0.0, "no anchor: frames / rate");
        // callback at `i0` handing out frames 4800.. (256 per callback), 5 ms output latency
        let i0 = Instant::now();
        let mut p = tp(0);
        p.frames = 4800 + 256;
        shared.publish(p, Some((4800, i0, Duration::from_millis(5), 256)));
        let s = shared.snapshot();
        let ms = |x: u64| Duration::from_micros(x * 1000);
        assert!((s.now_extrapolated(i0 + ms(2)) - (0.1 + 0.002)).abs() < 1e-9);
        // clamped at two callback periods (2 × 5.33 ms)
        assert!((s.now_extrapolated(i0 + ms(500)) - (0.1 + 512.0 / 48_000.0)).abs() < 1e-9);
        assert!((s.heard_time(i0 + ms(2)) - (0.1 + 0.002 - 0.005)).abs() < 1e-9);
        // constant latency: two MIDI events 3 ms apart stay 3 ms apart, one period + one quantum
        // after the anchor mapping
        let a = s.audio_time_for_instant(i0 + ms(1));
        let b = s.audio_time_for_instant(i0 + ms(4));
        assert!((b - a - 0.003).abs() < 1e-9);
        assert!((a - (0.1 + 0.001 + 384.0 / 48_000.0)).abs() < 1e-9);
        assert!(a >= s.now(), "never before the next quantum to render");
    }
}
