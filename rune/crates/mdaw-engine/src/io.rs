//! Live audio input and recording taps: the sample queues between the audio device's input
//! callback, the core and the control side.
//!
//! - [`sample_ring`]: a lock-free SPSC queue of `f32` samples with bulk reads / writes (no
//!   allocation, lock or syscall on either side).
//! - [`input_pair`]: the device input → core path. The input callback pushes interleaved frames
//!   at the context rate into an [`InputFeed`]; the core reads one render quantum per quantum
//!   from the [`InputSource`] ([`crate::core::Command::SetInput`]). The two callbacks are not
//!   synchronised (cpal opens input and output as separate streams), so the source keeps a
//!   cushion of about one input block: it starts reading once two blocks are queued, plays
//!   silence on an underrun (and waits for the cushion again), and drops the oldest frames when
//!   the queue grows past four blocks (the two devices' clocks drift apart: one click then,
//!   instead of an ever-growing delay). On one audio interface both streams share a clock and
//!   neither happens.
//! - [`record_pair`]: the core → control path of one recording take. The core writes the dry
//!   input of an armed track each quantum into a [`RecordTap`]; the control side drains the
//!   [`TakeReader`] every UI frame. [`TakeReader::first_frame`] is the absolute frame of the
//!   first sample, which places the take on the transport.

use std::cell::UnsafeCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};

use crate::contract::RENDER_QUANTUM;

const Q: usize = RENDER_QUANTUM;

/// Most input channels the core reads (channels beyond are ignored).
pub const MAX_INPUT_CHANNELS: usize = 32;

struct RingBuf {
    data: Box<[UnsafeCell<f32>]>,
    /// Next sample to read (owned by the consumer).
    head: AtomicUsize,
    /// Next sample to write (owned by the producer).
    tail: AtomicUsize,
}

// SAFETY: samples in [head, tail) are read only by the consumer, the rest written only by the
// producer; the release / acquire pairs on the two indices order the accesses.
unsafe impl Sync for RingBuf {}
unsafe impl Send for RingBuf {}

impl RingBuf {
    fn ptr(&self) -> *mut f32 {
        UnsafeCell::raw_get(self.data.as_ptr())
    }
}

/// Writing end of a [`sample_ring`].
pub struct SampleProducer {
    ring: Arc<RingBuf>,
}

/// Reading end of a [`sample_ring`].
pub struct SampleConsumer {
    ring: Arc<RingBuf>,
}

/// A queue of up to `capacity` samples (at least 1).
pub fn sample_ring(capacity: usize) -> (SampleProducer, SampleConsumer) {
    let capacity = capacity.max(1);
    let ring = Arc::new(RingBuf {
        data: (0..capacity).map(|_| UnsafeCell::new(0.0)).collect(),
        head: AtomicUsize::new(0),
        tail: AtomicUsize::new(0),
    });
    (
        SampleProducer { ring: ring.clone() },
        SampleConsumer { ring },
    )
}

impl SampleProducer {
    /// Samples that fit right now.
    pub fn free(&self) -> usize {
        let r = &*self.ring;
        let used = r
            .tail
            .load(Ordering::Relaxed)
            .wrapping_sub(r.head.load(Ordering::Acquire));
        r.data.len() - used
    }

    /// Append as much of `samples` as fits; returns how many were written.
    pub fn write(&mut self, samples: &[f32]) -> usize {
        let r = &*self.ring;
        let cap = r.data.len();
        let tail = r.tail.load(Ordering::Relaxed);
        let head = r.head.load(Ordering::Acquire);
        let n = samples.len().min(cap - tail.wrapping_sub(head));
        if n == 0 {
            return 0;
        }
        let at = tail % cap;
        let first = n.min(cap - at);
        // SAFETY: [tail, tail + n) is outside [head, tail): the consumer does not touch it;
        // both copies stay inside the buffer.
        unsafe {
            std::ptr::copy_nonoverlapping(samples.as_ptr(), r.ptr().add(at), first);
            std::ptr::copy_nonoverlapping(samples.as_ptr().add(first), r.ptr(), n - first);
        }
        r.tail.store(tail.wrapping_add(n), Ordering::Release);
        n
    }
}

impl SampleConsumer {
    /// Samples waiting.
    pub fn len(&self) -> usize {
        let r = &*self.ring;
        r.tail
            .load(Ordering::Acquire)
            .wrapping_sub(r.head.load(Ordering::Relaxed))
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Move up to `out.len()` samples into `out`; returns how many.
    pub fn read(&mut self, out: &mut [f32]) -> usize {
        let r = &*self.ring;
        let cap = r.data.len();
        let head = r.head.load(Ordering::Relaxed);
        let tail = r.tail.load(Ordering::Acquire);
        let n = out.len().min(tail.wrapping_sub(head));
        if n == 0 {
            return 0;
        }
        let at = head % cap;
        let first = n.min(cap - at);
        // SAFETY: [head, head + n) is inside [head, tail): written, and the producer does not
        // touch it; both copies stay inside the buffer.
        unsafe {
            std::ptr::copy_nonoverlapping(r.ptr().add(at), out.as_mut_ptr(), first);
            std::ptr::copy_nonoverlapping(r.ptr(), out.as_mut_ptr().add(first), n - first);
        }
        r.head.store(head.wrapping_add(n), Ordering::Release);
        n
    }

    /// Drop up to `n` of the oldest samples; returns how many.
    pub fn skip(&mut self, n: usize) -> usize {
        let r = &*self.ring;
        let head = r.head.load(Ordering::Relaxed);
        let tail = r.tail.load(Ordering::Acquire);
        let n = n.min(tail.wrapping_sub(head));
        r.head.store(head.wrapping_add(n), Ordering::Release);
        n
    }
}

/// What the input path counts, and the level of every input channel.
pub struct InputStats {
    /// Quanta the core rendered without input because the queue was short.
    underruns: AtomicU64,
    /// Frames dropped: by the feed (queue full) or by the source (latency bound).
    dropped: AtomicU64,
    /// The largest push so far, in frames: the input's block size.
    block: AtomicUsize,
    /// Peak (f32 bits) per channel since it was last read.
    peaks: [AtomicU32; MAX_INPUT_CHANNELS],
}

impl InputStats {
    fn new() -> Self {
        Self {
            underruns: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            block: AtomicUsize::new(0),
            peaks: std::array::from_fn(|_| AtomicU32::new(0)),
        }
    }

    pub fn underruns(&self) -> u64 {
        self.underruns.load(Ordering::Relaxed)
    }

    pub fn dropped_frames(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// The input's block size in frames (0 until the first block arrived).
    pub fn block_frames(&self) -> usize {
        self.block.load(Ordering::Relaxed)
    }

    /// The largest absolute sample of `channel` since the last call (a meter read).
    pub fn take_peak(&self, channel: usize) -> f32 {
        self.peaks
            .get(channel)
            .map_or(0.0, |p| f32::from_bits(p.swap(0, Ordering::Relaxed)))
    }
}

/// The device input callback's end (see the module docs).
pub struct InputFeed {
    tx: SampleProducer,
    channels: usize,
    stats: Arc<InputStats>,
}

impl InputFeed {
    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn stats(&self) -> &Arc<InputStats> {
        &self.stats
    }

    /// Queue interleaved frames (`frames.len()` a multiple of the channel count) at the context
    /// rate. Whole frames that do not fit are dropped (counted).
    pub fn push_interleaved(&mut self, frames: &[f32]) {
        let ch = self.channels;
        let n = frames.len() / ch;
        self.stats.block.fetch_max(n, Ordering::Relaxed);
        let fit = (self.tx.free() / ch).min(n);
        self.tx.write(&frames[..fit * ch]);
        if fit < n {
            self.stats
                .dropped
                .fetch_add((n - fit) as u64, Ordering::Relaxed);
        }
    }
}

/// The core's end (see the module docs).
pub struct InputSource {
    rx: SampleConsumer,
    channels: usize,
    /// The block size assumed until the feed reported one.
    default_block: usize,
    primed: bool,
    scratch: Box<[f32]>,
    stats: Arc<InputStats>,
}

/// An input path for `channels` interleaved channels (1..=[`MAX_INPUT_CHANNELS`]; more are
/// clamped) whose device delivers about `block_frames` frames per callback (a hint: the real
/// size is measured), at `sample_rate`.
pub fn input_pair(
    channels: usize,
    block_frames: usize,
    sample_rate: u32,
) -> (InputFeed, InputSource) {
    let channels = channels.clamp(1, MAX_INPUT_CHANNELS);
    // Two seconds: far beyond the latency bound, so the feed only drops when the core stalls.
    let (tx, rx) = sample_ring(channels * (2 * sample_rate as usize).max(8 * Q));
    let stats = Arc::new(InputStats::new());
    (
        InputFeed {
            tx,
            channels,
            stats: stats.clone(),
        },
        InputSource {
            rx,
            channels,
            default_block: block_frames.max(Q),
            primed: false,
            scratch: vec![0.0; channels * Q].into_boxed_slice(),
            stats,
        },
    )
}

impl InputSource {
    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn stats(&self) -> &Arc<InputStats> {
        &self.stats
    }

    /// The next render quantum of every channel into `out[channel]` (planar; `out` holds at
    /// least [`Self::channels`] rows). Silence when the queue is short. Allocation-free.
    pub fn read_quantum(&mut self, out: &mut [[f32; Q]]) {
        let ch = self.channels.min(out.len());
        let block = match self.stats.block.load(Ordering::Relaxed) {
            0 => self.default_block,
            b => b.max(Q),
        };
        let mut avail = self.rx.len() / self.channels;
        if avail > 4 * block + Q {
            // The clocks drifted (or the core stalled): back to the cushion.
            let drop = avail - 2 * block;
            self.rx.skip(drop * self.channels);
            self.stats.dropped.fetch_add(drop as u64, Ordering::Relaxed);
            avail -= drop;
        }
        if !self.primed && avail >= 2 * block {
            self.primed = true;
        }
        if !self.primed || avail < Q {
            if self.primed {
                self.primed = false;
                self.stats.underruns.fetch_add(1, Ordering::Relaxed);
            }
            for row in out.iter_mut().take(ch) {
                row.fill(0.0);
            }
            return;
        }
        let got = self.rx.read(&mut self.scratch);
        debug_assert_eq!(got, self.scratch.len());
        for (c, row) in out.iter_mut().take(ch).enumerate() {
            let mut peak = 0f32;
            for (i, v) in row.iter_mut().enumerate() {
                let x = self.scratch[i * self.channels + c];
                // A device never sends them, but nothing non-finite may reach a strip.
                *v = if x.is_finite() { x } else { 0.0 };
                peak = peak.max(v.abs());
            }
            self.stats.peaks[c].fetch_max(peak.to_bits(), Ordering::Relaxed);
        }
    }
}

/// No first frame yet.
const UNSET: u64 = u64::MAX;

struct TakeInfo {
    first_frame: AtomicU64,
    dropped: AtomicU64,
}

/// The core's end of a take: one armed track's dry input (see the module docs).
pub struct RecordTap {
    tx: SampleProducer,
    stereo: bool,
    info: Arc<TakeInfo>,
}

/// The control side's end of a take.
pub struct TakeReader {
    rx: SampleConsumer,
    stereo: bool,
    info: Arc<TakeInfo>,
    /// Everything drained so far (interleaved when stereo).
    data: Vec<f32>,
    /// The peak of every [`TAKE_PEAK_FRAMES`] frames drained (the last one may be partial).
    peaks: Vec<f32>,
}

/// Frames per column of a take's live waveform ([`TakeReader::peaks`]).
pub const TAKE_PEAK_FRAMES: usize = 512;

/// A take of mono or stereo input at `sample_rate`. The queue holds `seconds` of audio between
/// two [`TakeReader::pump`] calls (a UI stall longer than that loses samples, counted in
/// [`TakeReader::dropped_frames`]).
pub fn record_pair(stereo: bool, sample_rate: u32, seconds: f64) -> (RecordTap, TakeReader) {
    let channels = if stereo { 2 } else { 1 };
    let frames = ((sample_rate as f64 * seconds) as usize).max(16 * Q);
    let (tx, rx) = sample_ring(frames * channels);
    let info = Arc::new(TakeInfo {
        first_frame: AtomicU64::new(UNSET),
        dropped: AtomicU64::new(0),
    });
    (
        RecordTap {
            tx,
            stereo,
            info: info.clone(),
        },
        TakeReader {
            rx,
            stereo,
            info,
            data: Vec::new(),
            peaks: Vec::new(),
        },
    )
}

impl RecordTap {
    /// The quantum starting at absolute frame `frame`. `right` is ignored by a mono take; a
    /// stereo take without `right` records `left` on both channels. Allocation-free.
    pub fn write(&mut self, frame: u64, left: &[f32; Q], right: Option<&[f32; Q]>) {
        let _ = self.info.first_frame.compare_exchange(
            UNSET,
            frame,
            Ordering::Relaxed,
            Ordering::Relaxed,
        );
        let written = if self.stereo {
            let right = right.unwrap_or(left);
            let mut inter = [0f32; 2 * Q];
            for i in 0..Q {
                inter[2 * i] = left[i];
                inter[2 * i + 1] = right[i];
            }
            if self.tx.free() >= 2 * Q {
                self.tx.write(&inter) / 2
            } else {
                0
            }
        } else if self.tx.free() >= Q {
            self.tx.write(left)
        } else {
            0
        };
        if written < Q {
            // Whole quanta only, so the stream stays frame-aligned.
            self.info
                .dropped
                .fetch_add((Q - written) as u64, Ordering::Relaxed);
        }
    }
}

impl TakeReader {
    pub fn is_stereo(&self) -> bool {
        self.stereo
    }

    /// Move what the core recorded since the last call into the take. Call every UI frame.
    pub fn pump(&mut self) {
        let n = self.rx.len();
        if n == 0 {
            return;
        }
        let at = self.data.len();
        self.data.resize(at + n, 0.0);
        let got = self.rx.read(&mut self.data[at..]);
        self.data.truncate(at + got);
        // Extend the peak columns with the new samples (the last column may have been partial).
        let step = TAKE_PEAK_FRAMES * if self.stereo { 2 } else { 1 };
        let mut i = at;
        while i < self.data.len() {
            let column = i / step;
            let end = ((column + 1) * step).min(self.data.len());
            let peak = self.data[i..end].iter().fold(0f32, |m, v| m.max(v.abs()));
            match self.peaks.get_mut(column) {
                Some(p) => *p = p.max(peak),
                None => self.peaks.push(peak),
            }
            i = end;
        }
    }

    /// The absolute frame of the take's first sample (None: nothing recorded yet).
    pub fn first_frame(&self) -> Option<u64> {
        match self.info.first_frame.load(Ordering::Relaxed) {
            UNSET => None,
            f => Some(f),
        }
    }

    /// Frames lost because the queue was full (see [`record_pair`]).
    pub fn dropped_frames(&self) -> u64 {
        self.info.dropped.load(Ordering::Relaxed)
    }

    /// Frames drained so far.
    pub fn frames(&self) -> usize {
        self.data.len() / if self.stereo { 2 } else { 1 }
    }

    /// The peak of every [`TAKE_PEAK_FRAMES`] frames drained so far (the live waveform of a
    /// running take; kept up to date by [`Self::pump`]).
    pub fn peaks(&self) -> &[f32] {
        &self.peaks
    }

    /// The take as planar channels (after a last [`Self::pump`]).
    pub fn into_channels(mut self) -> Vec<Vec<f32>> {
        self.pump();
        if !self.stereo {
            return vec![self.data];
        }
        let n = self.data.len() / 2;
        let mut l = Vec::with_capacity(n);
        let mut r = Vec::with_capacity(n);
        for f in self.data.as_chunks::<2>().0 {
            l.push(f[0]);
            r.push(f[1]);
        }
        vec![l, r]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sample_ring_keeps_order_across_the_wrap() {
        let (mut tx, mut rx) = sample_ring(8);
        assert_eq!(tx.write(&[1.0, 2.0, 3.0, 4.0, 5.0]), 5);
        let mut out = [0f32; 3];
        assert_eq!(rx.read(&mut out), 3);
        assert_eq!(out, [1.0, 2.0, 3.0]);
        // wraps: 6 free, 6 written across the end
        assert_eq!(tx.free(), 6);
        assert_eq!(tx.write(&[6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0]), 6);
        assert_eq!(rx.len(), 8);
        let mut out = [0f32; 16];
        assert_eq!(rx.read(&mut out), 8);
        assert_eq!(&out[..8], &[4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0]);
        assert!(rx.is_empty());
        assert_eq!(tx.write(&[1.0]), 1);
        assert_eq!(rx.skip(5), 1);
    }

    fn block(ch: usize, frames: usize, start: usize) -> Vec<f32> {
        (0..frames)
            .flat_map(|f| (0..ch).map(move |c| (start + f) as f32 + c as f32 * 0.25))
            .collect()
    }

    #[test]
    fn the_input_source_waits_for_its_cushion_then_reads_in_order() {
        let (mut feed, mut src) = input_pair(2, 256, 48_000);
        let mut out = [[0f32; Q]; 2];
        feed.push_interleaved(&block(2, 256, 0));
        src.read_quantum(&mut out);
        assert!(out[0].iter().all(|v| *v == 0.0), "one block: not primed");
        assert_eq!(src.stats().underruns(), 0);
        feed.push_interleaved(&block(2, 256, 256));
        src.read_quantum(&mut out);
        assert_eq!(out[0][0], 0.0);
        assert_eq!(out[0][127], 127.0);
        assert_eq!(out[1][5], 5.25);
        src.read_quantum(&mut out);
        assert_eq!(out[0][0], 128.0);
        assert_eq!(src.stats().block_frames(), 256);
        assert_eq!(src.stats().take_peak(0), 255.0);
        assert_eq!(src.stats().take_peak(0), 0.0, "the peak read resets it");
    }

    #[test]
    fn an_underrun_is_silent_and_counted_and_the_latency_is_bounded() {
        let (mut feed, mut src) = input_pair(1, 128, 48_000);
        let mut out = [[0f32; Q]; 1];
        feed.push_interleaved(&block(1, 128, 1));
        feed.push_interleaved(&block(1, 128, 129));
        src.read_quantum(&mut out);
        src.read_quantum(&mut out);
        assert_eq!(out[0][127], 256.0);
        src.read_quantum(&mut out);
        assert!(out[0].iter().all(|v| *v == 0.0));
        assert_eq!(src.stats().underruns(), 1);
        // not primed again until two blocks wait
        feed.push_interleaved(&block(1, 128, 1000));
        src.read_quantum(&mut out);
        assert!(out[0].iter().all(|v| *v == 0.0));
        assert_eq!(src.stats().underruns(), 1, "counted once per gap");
        // a stall: 10 blocks queue up; the source jumps to the newest two
        for i in 0..10 {
            feed.push_interleaved(&block(1, 128, 2000 + i * 128));
        }
        src.read_quantum(&mut out);
        assert!(src.stats().dropped_frames() > 0);
        assert!(out[0][0] >= 2000.0 + 8.0 * 128.0, "{}", out[0][0]);
    }

    #[test]
    fn non_finite_input_never_reaches_the_core() {
        let (mut feed, mut src) = input_pair(1, 128, 48_000);
        let mut b = vec![0.5f32; 256];
        b[3] = f32::NAN;
        b[4] = f32::INFINITY;
        feed.push_interleaved(&b[..128]);
        feed.push_interleaved(&b[128..]);
        let mut out = [[0f32; Q]; 1];
        src.read_quantum(&mut out);
        assert!(out[0].iter().all(|v| v.is_finite()));
        assert_eq!(out[0][3], 0.0);
        assert_eq!(out[0][5], 0.5);
    }

    #[test]
    fn a_take_records_whole_quanta_and_reports_its_first_frame() {
        let (mut tap, mut take) = record_pair(true, 48_000, 1.0);
        assert_eq!(take.first_frame(), None);
        let l = [0.5f32; Q];
        let r = [-0.25f32; Q];
        tap.write(1280, &l, Some(&r));
        tap.write(1408, &l, None);
        take.pump();
        assert_eq!(take.first_frame(), Some(1280));
        assert_eq!(take.frames(), 2 * Q);
        assert_eq!(take.peaks(), &[0.5]);
        // more quanta in several pumps: the partial column is completed, new ones follow
        for i in 2..10 {
            tap.write(1280 + i * Q as u64, &r, None);
            take.pump();
        }
        assert_eq!(take.frames(), 10 * Q);
        assert_eq!(take.peaks().len(), (10 * Q).div_ceil(TAKE_PEAK_FRAMES));
        assert_eq!(take.peaks()[0], 0.5);
        assert_eq!(*take.peaks().last().unwrap(), 0.25);
        let ch = take.into_channels();
        assert_eq!(ch[0].len(), 10 * Q);
        assert_eq!(ch[1][0], -0.25);
        assert_eq!(ch[1][Q], 0.5, "no right channel: the left one");

        // a full queue drops whole quanta
        let (mut tap, take) = record_pair(false, 48_000, 0.0);
        for i in 0..20 {
            tap.write(i * Q as u64, &l, None);
        }
        assert_eq!(take.dropped_frames(), 4 * Q as u64);
        assert_eq!(take.into_channels()[0].len(), 16 * Q);
    }
}
