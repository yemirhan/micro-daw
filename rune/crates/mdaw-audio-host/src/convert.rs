//! Planar stereo f32 → device buffer (any channel count, any sample format), allocation-free.

use std::sync::Arc;

use cpal::{FromSample, SizedSample};

use crate::{HostStats, SharedRender};

/// Frames of planar scratch allocated per stream: device buffers larger than this are rendered
/// in several chunks (the engine runs 128-frame quanta, so chunking changes nothing audible).
pub const MAX_BLOCK_FRAMES: usize = 4096;

/// The host side of a device callback: owns the planar scratch, calls the shared render
/// callback, and interleaves / converts into the device buffer. Never allocates after `new`.
pub struct RenderPipe {
    render: SharedRender,
    left: Box<[f32]>,
    right: Box<[f32]>,
    stats: Arc<HostStats>,
}

impl RenderPipe {
    pub fn new(render: SharedRender, max_block_frames: usize, stats: Arc<HostStats>) -> Self {
        let n = max_block_frames.max(1);
        Self {
            render,
            left: vec![0.0; n].into_boxed_slice(),
            right: vec![0.0; n].into_boxed_slice(),
            stats,
        }
    }

    /// Fill an interleaved device buffer of `channels` channels.
    pub fn fill<T>(&mut self, out: &mut [T], channels: usize)
    where
        T: SizedSample + FromSample<f32>,
    {
        let channels = channels.max(1);
        let frames = out.len() / channels;
        self.stats.record_callback(frames);
        let mut guard = self.render.try_lock();
        if guard.is_none() {
            self.stats.record_skip();
        }
        let mut done = 0;
        while done < frames {
            let n = (frames - done).min(self.left.len());
            let (l, r) = (&mut self.left[..n], &mut self.right[..n]);
            l.fill(0.0);
            r.fill(0.0);
            if let Some(render) = guard.as_mut() {
                (render)(l, r);
            }
            interleave(
                &mut out[done * channels..(done + n) * channels],
                channels,
                l,
                r,
            );
            done += n;
        }
        // A trailing partial frame (never produced by real backends): silence.
        for s in &mut out[frames * channels..] {
            *s = T::EQUILIBRIUM;
        }
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
mod tests {
    use super::*;
    use crate::shared_render;

    #[test]
    fn interleave_stereo_mono_and_multichannel() {
        let l = [0.5f32, -0.25];
        let r = [0.1f32, 0.3];
        let mut st = [0f32; 4];
        interleave(&mut st, 2, &l, &r);
        assert_eq!(st, [0.5, 0.1, -0.25, 0.3]);
        let mut mono = [0f32; 2];
        interleave(&mut mono, 1, &l, &r);
        assert_eq!(mono, [(0.5f32 + 0.1) * 0.5, (-0.25f32 + 0.3) * 0.5]);
        let mut quad = [9f32; 8];
        interleave(&mut quad, 4, &l, &r);
        assert_eq!(quad, [0.5, 0.1, 0.0, 0.0, -0.25, 0.3, 0.0, 0.0]);
    }

    #[test]
    fn integer_formats_clamp_and_center() {
        let l = [0.5f32, 2.0, f32::NAN, -1.0];
        let r = [0.0f32, -2.0, 0.0, 0.0];
        let mut i16s = [0i16; 8];
        interleave(&mut i16s, 2, &l, &r);
        assert_eq!(i16s, [16384, 0, 32767, -32768, 0, 0, -32768, 0]);
        let mut u16s = [0u16; 8];
        interleave(&mut u16s, 2, &l, &r);
        assert_eq!(u16s[1], 32768); // 0.0 → the unsigned midpoint
        assert_eq!(u16s[0], 32768 + 16384);
        assert_eq!(u16s[4], 32768); // NaN → equilibrium
        let mut i32s = [0i32; 2];
        interleave(&mut i32s, 2, &[1.0], &[-0.5]);
        assert_eq!(i32s, [i32::MAX, -(1 << 30)]);
        let mut f64s = [0f64; 2];
        interleave(&mut f64s, 2, &[1.5], &[-0.5]);
        assert_eq!(f64s, [1.5, -0.5]); // floats are not clamped
        let mut u8s = [0u8; 2];
        interleave(&mut u8s, 2, &[0.0], &[-1.0]);
        assert_eq!(u8s, [128, 0]);
    }

    #[test]
    fn pipe_chunks_large_buffers_and_counts() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let c = calls.clone();
        let mut n = 0.0f32;
        let render = shared_render(move |l, r| {
            c.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            for (a, b) in l.iter_mut().zip(r.iter_mut()) {
                *a = n;
                *b = -n;
                n += 1.0;
            }
        });
        let stats = Arc::new(HostStats::default());
        let mut pipe = RenderPipe::new(render, 4, stats.clone());
        let mut out = [0f32; 20]; // 10 frames → chunks of 4, 4, 2
        pipe.fill(&mut out, 2);
        assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 3);
        let left: Vec<f32> = out.chunks(2).map(|f| f[0]).collect();
        assert_eq!(left, (0..10).map(|x| x as f32).collect::<Vec<_>>());
        assert_eq!(out[1], -0.0);
        assert_eq!(out[3], -1.0);
        let s = stats.snapshot();
        assert_eq!(
            (
                s.callbacks,
                s.frames,
                s.last_frames,
                s.min_frames,
                s.max_frames
            ),
            (1, 10, 10, 10, 10)
        );
    }

    #[test]
    fn pipe_is_silent_while_render_is_locked() {
        let render = shared_render(|l, _| l.fill(1.0));
        let stats = Arc::new(HostStats::default());
        let mut pipe = RenderPipe::new(render.clone(), 64, stats.clone());
        let mut out = [7i16; 6];
        let guard = render.lock();
        pipe.fill(&mut out, 2);
        drop(guard);
        assert_eq!(out, [0; 6]);
        assert_eq!(stats.snapshot().skipped, 1);
        pipe.fill(&mut out, 2);
        assert_eq!(out[0], i16::MAX);
    }
}
