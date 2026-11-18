//! A device-less host: a thread calls the render callback block by block at real-time pace.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::{HostError, HostStats, RenderPipe, SharedRender, StatsSnapshot};

/// Headless output: renders `block_frames` stereo frames every `block_frames / sample_rate`
/// seconds (deadline-based, so the long-run rate is exact) into a discarded buffer.
pub struct NullHost {
    running: Arc<AtomicBool>,
    stats: Arc<HostStats>,
    sample_rate: u32,
    block_frames: usize,
    thread: Option<JoinHandle<()>>,
}

impl NullHost {
    pub fn start(
        sample_rate: u32,
        block_frames: usize,
        render: SharedRender,
    ) -> Result<Self, HostError> {
        let sample_rate = sample_rate.max(1);
        let block_frames = block_frames.max(1);
        let running = Arc::new(AtomicBool::new(true));
        let stats = Arc::new(HostStats::default());
        let (r, s) = (running.clone(), stats.clone());
        let thread = std::thread::Builder::new()
            .name("mdaw-null-audio".into())
            .spawn(move || {
                let mut pipe = RenderPipe::new(render, block_frames, s);
                let mut out = vec![0f32; block_frames * 2];
                let period = Duration::from_secs_f64(block_frames as f64 / sample_rate as f64);
                let start = Instant::now();
                let mut n: u32 = 0;
                while r.load(Ordering::Relaxed) {
                    pipe.fill(&mut out, 2);
                    n = n.wrapping_add(1);
                    let deadline = start + period * n;
                    let now = Instant::now();
                    if deadline > now {
                        std::thread::sleep(deadline - now);
                    }
                }
            })
            .map_err(|e| HostError::Thread(e.to_string()))?;
        Ok(Self {
            running,
            stats,
            sample_rate,
            block_frames,
            thread: Some(thread),
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn block_frames(&self) -> usize {
        self.block_frames
    }

    pub fn stats(&self) -> StatsSnapshot {
        self.stats.snapshot()
    }

    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for NullHost {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared_render;
    use std::sync::atomic::AtomicU64;

    #[test]
    fn runs_at_real_time_pace() {
        let frames = Arc::new(AtomicU64::new(0));
        let f = frames.clone();
        let render = shared_render(move |l, r| {
            assert_eq!(l.len(), r.len());
            f.fetch_add(l.len() as u64, Ordering::Relaxed);
        });
        let host = NullHost::start(48_000, 480, render).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let s = host.stats();
        host.stop();
        let rendered = frames.load(Ordering::Relaxed);
        // ~300 ms of 10 ms blocks; generous bounds for loaded CI machines.
        assert!(
            (10..=40).contains(&s.callbacks),
            "callbacks {}",
            s.callbacks
        );
        assert_eq!(rendered, s.frames);
        assert_eq!(s.last_frames, 480);
    }
}
