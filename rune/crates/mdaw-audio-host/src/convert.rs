//! Planar stereo f32 (context rate) → device buffer (device rate, any channel count, any sample
//! format), allocation-free.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::time::{Duration, Instant};

use cpal::{FromSample, SizedSample};

use crate::{BlockTiming, HostStats, Resampler, SharedRender};

/// Frames of planar scratch allocated per stream: device buffers larger than this are rendered
/// in several chunks (the engine runs 128-frame quanta, so chunking changes nothing audible).
/// Also the largest context-rate chunk handed to the render callback when resampling.
pub const MAX_BLOCK_FRAMES: usize = 4096;

/// The host side of a device callback: owns the planar scratch (and the resampler when the
/// device rate differs from the context rate), calls the shared render callback with a
/// [`BlockTiming`], and interleaves / converts into the device buffer. Never allocates, locks
/// (beyond `try_lock` on the render callback) or blocks after `new`.
///
/// A panic in the render callback is caught: the rest of that callback is silent, the panic is
/// counted and latched in [`HostStats`], and every later fill outputs silence without calling
/// the render callback until [`HostStats::clear_render_panic`].
pub struct RenderPipe {
    render: SharedRender,
    left: Box<[f32]>,
    right: Box<[f32]>,
    resampler: Option<Resampler>,
    context_rate: u32,
    device_rate: u32,
    stats: Arc<HostStats>,
    context_frames: u64,
    device_frames: u64,
}

impl RenderPipe {
    /// A pipe rendering at `context_rate` into a device running at `device_rate` (resampled
    /// when they differ), with [`MAX_BLOCK_FRAMES`] of scratch.
    pub fn new(
        render: SharedRender,
        context_rate: u32,
        device_rate: u32,
        stats: Arc<HostStats>,
    ) -> Self {
        Self::with_max_block(render, context_rate, device_rate, MAX_BLOCK_FRAMES, stats)
    }

    /// Like [`Self::new`] with `max_block_frames` of scratch (the largest chunk per render call).
    pub fn with_max_block(
        render: SharedRender,
        context_rate: u32,
        device_rate: u32,
        max_block_frames: usize,
        stats: Arc<HostStats>,
    ) -> Self {
        let n = max_block_frames.max(1);
        let (context_rate, device_rate) = (context_rate.max(1), device_rate.max(1));
        Self {
            render,
            left: vec![0.0; n].into_boxed_slice(),
            right: vec![0.0; n].into_boxed_slice(),
            resampler: (context_rate != device_rate)
                .then(|| Resampler::new(context_rate, device_rate, n)),
            context_rate,
            device_rate,
            stats,
            context_frames: 0,
            device_frames: 0,
        }
    }

    /// Fill an interleaved device buffer of `channels` channels, timed at `Instant::now()` with
    /// an unknown device latency.
    pub fn fill<T>(&mut self, out: &mut [T], channels: usize)
    where
        T: SizedSample + FromSample<f32>,
    {
        self.fill_at(out, channels, Instant::now(), Duration::ZERO);
    }

    /// Fill an interleaved device buffer of `channels` channels. `callback` is the instant the
    /// device callback started, `device_latency` the backend's callback → playback time of the
    /// buffer's first frame (ZERO when unknown).
    pub fn fill_at<T>(
        &mut self,
        out: &mut [T],
        channels: usize,
        callback: Instant,
        device_latency: Duration,
    ) where
        T: SizedSample + FromSample<f32>,
    {
        let channels = channels.max(1);
        let frames = out.len() / channels;
        let Self {
            render,
            left,
            right,
            resampler,
            context_rate,
            stats,
            context_frames,
            device_frames,
            ..
        } = self;
        stats.record_callback(frames);
        let silenced = stats.render_panicked();
        let mut guard = if silenced { None } else { render.try_lock() };
        if guard.is_none() && !silenced {
            stats.record_skip();
        }
        let lead = resampler.as_ref().map_or(0, |r| r.lead_ns());
        let output_latency = device_latency + Duration::from_nanos(lead);
        let context_rate = *context_rate;
        let mut frame_offset = 0u64;
        let mut chunk = |l: &mut [f32], r: &mut [f32]| {
            l.fill(0.0);
            r.fill(0.0);
            if let Some(render) = guard.as_mut() {
                let timing = BlockTiming {
                    callback,
                    frame_offset,
                    context_rate,
                    output_latency,
                };
                let call = catch_unwind(AssertUnwindSafe(|| (render)(&mut *l, &mut *r, &timing)));
                if call.is_err() {
                    stats.record_render_panic();
                    guard = None;
                    l.fill(0.0);
                    r.fill(0.0);
                }
            }
            frame_offset += l.len() as u64;
        };
        let mut done = 0;
        while done < frames {
            let n = (frames - done).min(left.len());
            let (l, r) = (&mut left[..n], &mut right[..n]);
            match resampler.as_mut() {
                Some(rs) => rs.process(l, r, &mut chunk),
                None => chunk(l, r),
            }
            interleave(
                &mut out[done * channels..(done + n) * channels],
                channels,
                l,
                r,
            );
            done += n;
        }
        *context_frames += frame_offset;
        *device_frames += frames as u64;
        // A trailing partial frame (never produced by real backends): silence.
        for s in &mut out[frames * channels..] {
            *s = T::EQUILIBRIUM;
        }
    }

    /// Context rate != device rate.
    pub fn resampling(&self) -> bool {
        self.resampler.is_some()
    }

    pub fn context_rate(&self) -> u32 {
        self.context_rate
    }

    pub fn device_rate(&self) -> u32 {
        self.device_rate
    }

    /// Context-rate frames requested from the render callback so far (including silent ones).
    pub fn context_frames(&self) -> u64 {
        self.context_frames
    }

    /// Device frames produced so far.
    pub fn device_frames(&self) -> u64 {
        self.device_frames
    }

    /// The resampler's look-ahead in context frames (0 at the native rate).
    pub fn resampler_latency_frames(&self) -> usize {
        self.resampler.as_ref().map_or(0, |r| r.latency_frames())
    }

    pub fn stats(&self) -> &Arc<HostStats> {
        &self.stats
    }
}

/// Interleave planar stereo into `out` (`channels` per frame, `left.len()` frames):
/// mono = (L + R) / 2, stereo = L R, more channels = L R then silence. Integer formats get the
/// signal clamped to [-1, 1] first (cpal's conversions overflow outside it); NaN becomes 0.
pub fn interleave<T>(out: &mut [T], channels: usize, left: &[f32], right: &[f32])
where
    T: SizedSample + FromSample<f32>,
{
    let float = T::FORMAT.is_float();
    let conv = |x: f32| -> T {
        if float {
            T::from_sample(x)
        } else if x.is_nan() {
            T::EQUILIBRIUM
        } else {
            T::from_sample(x.clamp(-1.0, 1.0))
        }
    };
    match channels {
        0 => {}
        1 => {
            for (o, (l, r)) in out.iter_mut().zip(left.iter().zip(right)) {
                *o = conv((l + r) * 0.5);
            }
        }
        _ => {
            for (frame, (l, r)) in out.chunks_exact_mut(channels).zip(left.iter().zip(right)) {
                frame[0] = conv(*l);
                frame[1] = conv(*r);
                for s in &mut frame[2..] {
                    *s = T::EQUILIBRIUM;
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "convert_tests.rs"]
mod tests;
