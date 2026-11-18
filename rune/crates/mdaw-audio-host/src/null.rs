//! A device-less host: a thread calls the render callback block by block at real-time pace.

use std::fmt;
use std::sync::Arc;

use crate::pacer::Pacer;
use crate::{HostError, HostStats, SharedRender, StatsSnapshot};

/// Receives interleaved stereo device-rate output blocks (see [`NullOptions::tap`]).
pub type OutputTap = Box<dyn FnMut(&[f32]) + Send>;

/// How to run a [`NullHost`].
pub struct NullOptions {
    /// The simulated device's rate: blocks are paced at it.
    pub device_rate: u32,
    /// The render callback's rate; resampled to `device_rate` (like a real device) when it
    /// differs.
    pub context_rate: u32,
    /// Device frames per block.
    pub block_frames: usize,
    /// Receives each block's interleaved stereo device-rate output (tests, metering); runs on
    /// the null audio thread.
    pub tap: Option<OutputTap>,
}

impl Default for NullOptions {
    fn default() -> Self {
        Self {
            device_rate: 48_000,
            context_rate: 48_000,
            block_frames: 128,
            tap: None,
        }
    }
}

impl fmt::Debug for NullOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NullOptions")
            .field("device_rate", &self.device_rate)
            .field("context_rate", &self.context_rate)
            .field("block_frames", &self.block_frames)
            .field("tap", &self.tap.is_some())
            .finish()
    }
}

/// Headless output: renders `block_frames` stereo frames every `block_frames / device_rate`
/// seconds (deadline-based, so the long-run rate is exact; never ahead of real time; more than
/// 250 ms behind it re-anchors instead of bursting) into a discarded buffer (or the tap).
/// [`crate::BlockTiming`]: `callback` is the tick instant, `output_latency` is ZERO.
pub struct NullHost {
    stats: Arc<HostStats>,
    device_rate: u32,
    context_rate: u32,
    block_frames: usize,
    pacer: Option<Pacer>,
}

impl NullHost {
    /// Context rate = device rate = `sample_rate`, no tap.
    pub fn start(
        sample_rate: u32,
        block_frames: usize,
        render: SharedRender,
    ) -> Result<Self, HostError> {
        Self::start_with(
            NullOptions {
                device_rate: sample_rate,
                context_rate: sample_rate,
                block_frames,
                tap: None,
            },
            render,
        )
    }

    pub fn start_with(options: NullOptions, render: SharedRender) -> Result<Self, HostError> {
        let device_rate = options.device_rate.max(1);
        let context_rate = options.context_rate.max(1);
        let block_frames = options.block_frames.max(1);
        let stats = Arc::new(HostStats::default());
        let pacer = Pacer::spawn(
            "mdaw-null-audio",
            context_rate,
            device_rate,
            block_frames,
            render,
            stats.clone(),
            options.tap,
        )?;
        Ok(Self {
            stats,
            device_rate,
            context_rate,
            block_frames,
            pacer: Some(pacer),
        })
    }

    /// The device rate (kept for callers of the single-rate API).
    pub fn sample_rate(&self) -> u32 {
        self.device_rate
    }

    pub fn device_rate(&self) -> u32 {
        self.device_rate
    }

    pub fn context_rate(&self) -> u32 {
        self.context_rate
    }

    pub fn block_frames(&self) -> usize {
        self.block_frames
    }

    pub fn stats(&self) -> StatsSnapshot {
        self.stats.snapshot()
    }

    pub fn stats_handle(&self) -> &Arc<HostStats> {
        &self.stats
    }

    /// Resume calling the render callback after a caught render panic.
    pub fn clear_render_panic(&self) {
        self.stats.clear_render_panic();
    }

    pub fn stop(mut self) {
        if let Some(p) = self.pacer.take() {
            p.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared_render;
    use crate::test_util::{bpm, click_render, left, onsets};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    #[test]
    fn never_runs_ahead_of_real_time() {
        let frames = Arc::new(AtomicU64::new(0));
        let f = frames.clone();
        let render = shared_render(move |l, r, t| {
            assert_eq!(l.len(), r.len());
            assert_eq!((t.context_rate, t.output_latency), (48_000, Duration::ZERO));
            f.fetch_add(l.len() as u64, Ordering::Relaxed);
        });
        let t0 = Instant::now();
        let host = NullHost::start(48_000, 480, render).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        let s = host.stats();
        host.stop();
        let elapsed = t0.elapsed().as_secs_f64();
        let rendered = frames.load(Ordering::Relaxed);
        // Robust bounds only: at least one block, and never more than real time allows (block
        // n starts no earlier than n·10 ms after the start).
        assert!(s.callbacks >= 1);
        assert!(
            rendered as f64 <= elapsed * 48_000.0 + 480.0,
            "{rendered} frames in {elapsed:.3} s"
        );
        assert_eq!(rendered, s.frames);
        assert_eq!(s.last_frames, 480);
    }

    #[test]
    fn mismatched_device_rate_keeps_tempo() {
        // Context 48 kHz clicks every 24 000 frames (120 BPM), device 44.1 kHz.
        let (render, _) = click_render(24_000);
        let captured = Arc::new(Mutex::new(Vec::<f32>::new()));
        let c = captured.clone();
        let target = 44_100 * 16 / 10; // 1.6 s: clicks at 0, 0.5, 1.0, 1.5 s
        let (done_tx, done_rx) = crossbeam_channel::bounded(1);
        let tap = Box::new(move |out: &[f32]| {
            let mut v = c.lock().unwrap();
            v.extend(left(out));
            if v.len() >= target {
                let _ = done_tx.try_send(());
            }
        });
        let host = NullHost::start_with(
            NullOptions {
                device_rate: 44_100,
                context_rate: 48_000,
                block_frames: 441,
                tap: Some(tap),
            },
            render,
        )
        .unwrap();
        assert_eq!((host.device_rate(), host.context_rate()), (44_100, 48_000));
        done_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        host.stop();
        let out = captured.lock().unwrap();
        let clicks = onsets(&out[..target], 0.3, 4410);
        let tempo = bpm(&clicks, 44_100);
        eprintln!(
            "null host 48k -> 44.1k: {} clicks, {tempo:.6} BPM",
            clicks.len()
        );
        assert_eq!(clicks.len(), 4);
        assert!((tempo - 120.0).abs() <= 0.01, "{tempo} BPM");
    }
}
