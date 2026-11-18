//! The host side of the device callback (`RenderPipe::fill` / `fill_at`, including the
//! resampler, the chunk timing and the caught-panic path) never allocates, for any buffer size,
//! format or rate pair.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use mdaw_audio_host::{HostStats, RenderPipe, Resampler, SharedRender, shared_render};

struct Counting;

thread_local! {
    static ARMED: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if ARMED.with(|a| a.get()) {
            COUNT.with(|c| c.set(c.get() + 1));
        }
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static A: Counting = Counting;

fn armed<R>(f: impl FnOnce() -> R) -> (R, usize) {
    COUNT.with(|c| c.set(0));
    ARMED.with(|a| a.set(true));
    let r = f();
    ARMED.with(|a| a.set(false));
    (r, COUNT.with(|c| c.get()))
}

fn sine_render() -> SharedRender {
    let mut phase = 0.0f32;
    shared_render(move |l, r, t| {
        assert!(t.context_rate > 0);
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            *a = phase.sin();
            *b = -*a;
            phase += 0.01;
        }
    })
}

fn exercise(pipe: &mut RenderPipe) -> f32 {
    let mut f32s = vec![0f32; 8192 * 2];
    let mut i16s = vec![0i16; 3000 * 6];
    let mut mono = vec![0u16; 777];
    let ((), n) = armed(|| {
        for frames in [1usize, 128, 511, 512, 513, 4096, 8192] {
            pipe.fill(&mut f32s[..frames * 2], 2);
        }
        pipe.fill_at(&mut i16s, 6, Instant::now(), Duration::from_millis(5));
        pipe.fill(&mut mono, 1);
    });
    assert_eq!(n, 0, "allocations in fill");
    f32s[2]
}

#[test]
fn fill_does_not_allocate() {
    // Native rate, then resampled (down, up, 2×, an awkward ratio).
    for (ctx, dev) in [
        (48_000u32, 48_000u32),
        (48_000, 44_100),
        (44_100, 48_000),
        (48_000, 96_000),
        (44_100, 47_999),
    ] {
        let mut pipe = RenderPipe::with_max_block(
            sine_render(),
            ctx,
            dev,
            512,
            Arc::new(HostStats::default()),
        );
        assert_eq!(pipe.resampling(), ctx != dev);
        assert!(exercise(&mut pipe) != 0.0, "{ctx} -> {dev}");
        let mut pipe = RenderPipe::new(sine_render(), ctx, dev, Arc::new(HostStats::default()));
        exercise(&mut pipe);
    }
}

#[test]
fn resampler_process_does_not_allocate() {
    let mut rs = Resampler::new(48_000, 44_100, 256);
    let (mut l, mut r) = (vec![0f32; 4096], vec![0f32; 4096]);
    let mut pull = |a: &mut [f32], b: &mut [f32]| {
        a.fill(0.25);
        b.fill(-0.25);
    };
    let ((), n) = armed(|| {
        for frames in [1usize, 64, 1000, 4096] {
            rs.process(&mut l[..frames], &mut r[..frames], &mut pull);
        }
    });
    assert_eq!(n, 0);
}

#[test]
fn silenced_after_panic_does_not_allocate() {
    // The panic itself allocates (payload, hook); the latched-silence fills afterwards must not.
    let panicked = Arc::new(AtomicBool::new(false));
    let p = panicked.clone();
    let render = shared_render(move |_, _, _| {
        if !p.swap(true, Ordering::Relaxed) {
            panic!("render bug (expected by the test)");
        }
    });
    let stats = Arc::new(HostStats::default());
    for dev in [48_000u32, 44_100] {
        let mut pipe = RenderPipe::new(render.clone(), 48_000, dev, stats.clone());
        let mut out = vec![1f32; 1024 * 2];
        pipe.fill(&mut out, 2);
        assert!(stats.render_panicked());
        let ((), n) = armed(|| {
            for _ in 0..4 {
                pipe.fill(&mut out, 2);
            }
        });
        assert_eq!(n, 0);
        assert!(out.iter().all(|&x| x == 0.0));
    }
}
