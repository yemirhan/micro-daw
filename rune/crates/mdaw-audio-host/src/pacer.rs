//! Real-time pacing for device-less rendering ([`crate::NullHost`] and the [`crate::OutputHost`]
//! null fallback): a thread renders one block per `block_frames / rate` seconds against
//! absolute deadlines, so the long-run rate is exact and the clock never runs ahead of real time.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::{HostError, HostStats, OutputTap, RenderPipe, SharedRender};

/// When the pacer falls this far behind (machine suspended, thread starved) it re-anchors its
/// schedule instead of rendering the backlog in a burst.
pub(crate) const MAX_LAG: Duration = Duration::from_millis(250);

/// The deadline after `n` blocks of `block_frames` at `rate`: `start + n · block_frames / rate`,
/// computed from `n` (not accumulated), so there is no rounding drift however long it runs.
pub fn block_deadline(start: Instant, rate: u32, block_frames: usize, n: u64) -> Instant {
    let ns = n as u128 * block_frames as u128 * 1_000_000_000 / rate.max(1) as u128;
    start + Duration::from_nanos(ns.min(u64::MAX as u128) as u64)
}

/// Time source for [`run_paced`] (real or simulated in tests).
pub(crate) trait PaceClock {
    fn now(&mut self) -> Instant;
    fn sleep_until(&mut self, deadline: Instant);
}

pub(crate) struct RealClock;

impl PaceClock for RealClock {
    fn now(&mut self) -> Instant {
        Instant::now()
    }
    fn sleep_until(&mut self, deadline: Instant) {
        let now = Instant::now();
        if deadline > now {
            std::thread::sleep(deadline - now);
        }
    }
}

/// Call `tick(now)` once per block while `running`: block `n` starts no earlier than
/// `block_deadline(anchor, rate, block_frames, n)`. Late blocks run back to back (catching up);
/// more than [`MAX_LAG`] late, the schedule is re-anchored at the current time.
pub(crate) fn run_paced<C: PaceClock>(
    clock: &mut C,
    rate: u32,
    block_frames: usize,
    running: &AtomicBool,
    mut tick: impl FnMut(Instant),
) {
    let mut anchor = clock.now();
    let mut n: u64 = 0;
    while running.load(Ordering::Acquire) {
        tick(clock.now());
        n += 1;
        let deadline = block_deadline(anchor, rate, block_frames, n);
        let now = clock.now();
        if deadline > now {
            clock.sleep_until(deadline);
        } else if now - deadline > MAX_LAG {
            anchor = now;
            n = 0;
        }
    }
}

/// A running pacer thread driving a [`RenderPipe`] into a discarded (or tapped) stereo buffer.
pub(crate) struct Pacer {
    running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Pacer {
    /// Render `block_frames` device frames per block at `device_rate` (resampled from
    /// `context_rate` when they differ). Timing: the tick instant and ZERO output latency.
    pub(crate) fn spawn(
        name: &str,
        context_rate: u32,
        device_rate: u32,
        block_frames: usize,
        render: SharedRender,
        stats: Arc<HostStats>,
        mut tap: Option<OutputTap>,
    ) -> Result<Self, HostError> {
        let block_frames = block_frames.max(1);
        let running = Arc::new(AtomicBool::new(true));
        let r = running.clone();
        let thread = std::thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                let mut pipe = RenderPipe::new(render, context_rate, device_rate, stats);
                let mut out = vec![0f32; block_frames * 2];
                run_paced(&mut RealClock, device_rate, block_frames, &r, |now| {
                    pipe.fill_at(&mut out, 2, now, Duration::ZERO);
                    if let Some(tap) = tap.as_mut() {
                        tap(&out);
                    }
                });
            })
            .map_err(|e| HostError::Thread(e.to_string()))?;
        Ok(Self {
            running,
            thread: Some(thread),
        })
    }

    /// Stop and join: no render call happens after this returns.
    pub(crate) fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.running.store(false, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Pacer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared_render;
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::atomic::AtomicU64;

    /// Simulated time: `sleep_until` jumps to the deadline, ticks cost `work` each.
    struct FakeClock {
        t: Rc<Cell<Instant>>,
        sleeps: usize,
    }

    impl PaceClock for FakeClock {
        fn now(&mut self) -> Instant {
            self.t.get()
        }
        fn sleep_until(&mut self, deadline: Instant) {
            assert!(deadline > self.t.get());
            self.sleeps += 1;
            self.t.set(deadline);
        }
    }

    /// Run `blocks` ticks with `work(n)` simulated cost each; returns the tick instants.
    fn simulate(
        rate: u32,
        block: usize,
        blocks: usize,
        work: impl Fn(usize) -> Duration,
    ) -> (Instant, Vec<Instant>, usize) {
        let t0 = Instant::now();
        let t = Rc::new(Cell::new(t0));
        let mut clock = FakeClock {
            t: t.clone(),
            sleeps: 0,
        };
        let running = AtomicBool::new(true);
        let mut ticks = Vec::new();
        run_paced(&mut clock, rate, block, &running, |now| {
            ticks.push(now);
            t.set(now + work(ticks.len() - 1));
            if ticks.len() == blocks {
                running.store(false, Ordering::Release);
            }
        });
        (t0, ticks, clock.sleeps)
    }

    #[test]
    fn deadlines_are_exact_and_drift_free() {
        let t0 = Instant::now();
        assert_eq!(
            block_deadline(t0, 44_100, 441, 100),
            t0 + Duration::from_secs(1)
        );
        assert_eq!(block_deadline(t0, 48_000, 480, 0), t0);
        // 128-frame blocks at 44.1 kHz for 10 hours: still the exact floor of n·128/44100 s.
        let n = 10 * 3600 * 44_100 / 128;
        let ns = n as u128 * 128 * 1_000_000_000 / 44_100;
        assert_eq!(
            block_deadline(t0, 44_100, 128, n),
            t0 + Duration::from_nanos(ns as u64)
        );
    }

    #[test]
    fn ticks_land_on_deadlines_and_never_run_ahead() {
        let (t0, ticks, sleeps) = simulate(48_000, 480, 200, |_| Duration::from_micros(700));
        assert_eq!(ticks.len(), 200);
        assert_eq!(sleeps, 200);
        for (n, at) in ticks.iter().enumerate() {
            // Block n starts exactly at its deadline: frames rendered before it = n·480 =
            // elapsed·rate, never more.
            assert_eq!(*at, block_deadline(t0, 48_000, 480, n as u64), "block {n}");
        }
    }

    #[test]
    fn late_blocks_catch_up_then_reanchor_after_max_lag() {
        // Block 10 stalls 100 ms (< MAX_LAG): the next blocks run back to back until on time.
        let (t0, ticks, _) = simulate(48_000, 480, 40, |n| {
            Duration::from_millis(if n == 10 { 100 } else { 1 })
        });
        let late = ticks[11] - block_deadline(t0, 48_000, 480, 11);
        assert!(late > Duration::from_millis(80));
        for (n, at) in ticks.iter().enumerate().skip(30) {
            assert_eq!(
                *at,
                block_deadline(t0, 48_000, 480, n as u64),
                "caught up by {n}"
            );
        }
        for (n, at) in ticks.iter().enumerate() {
            assert!(
                *at >= block_deadline(t0, 48_000, 480, n as u64),
                "ahead at {n}"
            );
        }
        // A 2 s stall: no 200-block burst, the schedule restarts from the stall's end.
        let (_, ticks, _) = simulate(48_000, 480, 20, |n| {
            Duration::from_millis(if n == 5 { 2000 } else { 1 })
        });
        let restart = ticks[6];
        for (k, at) in ticks.iter().enumerate().skip(6) {
            let expect = block_deadline(restart, 48_000, 480, (k - 6) as u64);
            assert_eq!(*at, expect, "block {k} after re-anchoring");
        }
    }

    #[test]
    fn stop_joins_and_no_render_happens_after() {
        let calls = Arc::new(AtomicU64::new(0));
        let c = calls.clone();
        let render = shared_render(move |_, _, t| {
            assert_eq!(t.output_latency, Duration::ZERO);
            c.fetch_add(1, Ordering::Relaxed);
        });
        let stats = Arc::new(HostStats::default());
        let p = Pacer::spawn("t", 48_000, 48_000, 64, render, stats, None).unwrap();
        while calls.load(Ordering::Relaxed) == 0 {
            std::thread::yield_now();
        }
        p.stop();
        let after = calls.load(Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(calls.load(Ordering::Relaxed), after);
    }
}
