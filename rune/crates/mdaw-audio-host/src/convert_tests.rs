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
    let render = shared_render(move |l, r, _| {
        c.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            *a = n;
            *b = -n;
            n += 1.0;
        }
    });
    let stats = Arc::new(HostStats::default());
    let mut pipe = RenderPipe::with_max_block(render, 48_000, 48_000, 4, stats.clone());
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
    let render = shared_render(|l, _, _| l.fill(1.0));
    let stats = Arc::new(HostStats::default());
    let mut pipe = RenderPipe::with_max_block(render.clone(), 48_000, 48_000, 64, stats.clone());
    let mut out = [7i16; 6];
    let guard = render.lock();
    pipe.fill(&mut out, 2);
    drop(guard);
    assert_eq!(out, [0; 6]);
    assert_eq!(stats.snapshot().skipped, 1);
    pipe.fill(&mut out, 2);
    assert_eq!(out[0], i16::MAX);
}

use crate::test_util::{bpm, click_render, left, onsets};
use std::sync::Mutex as StdMutex;
use std::sync::atomic::Ordering;

/// Drive `total` device frames through a pipe in callbacks of varying sizes; returns the
/// device-rate left channel and the pipe.
fn drive(render: SharedRender, ctx: u32, dev: u32, total: usize) -> (Vec<f32>, RenderPipe) {
    let mut pipe = RenderPipe::new(render, ctx, dev, Arc::new(HostStats::default()));
    let mut out = Vec::with_capacity(total);
    let sizes = [512usize, 441, 128, 1000, 4096, 5000, 1];
    let mut buf = vec![0f32; 5000 * 2];
    let mut i = 0;
    while out.len() < total {
        let n = sizes[i % sizes.len()].min(total - out.len());
        i += 1;
        pipe.fill(&mut buf[..n * 2], 2);
        out.extend(left(&buf[..n * 2]));
    }
    (out, pipe)
}

#[test]
fn resampled_click_track_keeps_exact_tempo() {
    // A click every 0.5 s of context time (48 kHz) = 120 BPM, whatever the device rate.
    for dev in [44_100u32, 96_000, 48_000, 47_999] {
        let (render, _) = click_render(24_000);
        // 10.25 s: clicks at 0, 0.5, …, 10 s.
        let (out, pipe) = drive(render, 48_000, dev, (dev * 41 / 4) as usize);
        assert_eq!(pipe.resampling(), dev != 48_000);
        let clicks = onsets(&out, 0.3, (dev / 10) as usize);
        assert_eq!(
            clicks.len(),
            21,
            "{dev}: {} clicks {clicks:?}",
            clicks.len()
        );
        let tempo = bpm(&clicks, dev);
        eprintln!(
            "device {dev} Hz: {} clicks, {tempo:.6} BPM, first at {:.3}",
            clicks.len(),
            clicks[0]
        );
        assert!((tempo - 120.0).abs() <= 0.01, "{dev}: {tempo} BPM");
        // Output n is centred on context time n·ctx/dev: the first click sits at device frame 0.
        assert!(
            clicks[0].abs() < 0.05,
            "{dev}: first click at {}",
            clicks[0]
        );
    }
}

#[test]
fn context_frames_track_device_frames_exactly() {
    for (ctx, dev) in [
        (48_000u32, 44_100u32),
        (44_100, 48_000),
        (48_000, 96_000),
        (44_100, 47_999),
    ] {
        let (render, rendered) = click_render(24_000);
        let (out, pipe) = drive(render, ctx, dev, dev as usize * 7);
        let n = out.len() as u64;
        assert_eq!(pipe.device_frames(), n);
        assert_eq!(pipe.context_frames(), rendered.load(Ordering::Relaxed));
        let ideal = n as u128 * ctx as u128 / dev as u128; // floor(N·ctx/dev)
        let lat = pipe.resampler_latency_frames() as i128;
        let diff = pipe.context_frames() as i128 - ideal as i128;
        eprintln!(
            "{ctx}->{dev}: N={n} consumed={} ideal={ideal} diff={diff} latency={lat}",
            pipe.context_frames()
        );
        assert!(
            (lat - 1..=lat + 1).contains(&diff),
            "{ctx}->{dev}: diff {diff} vs latency {lat}"
        );
    }
}

#[test]
fn render_gets_chunk_timing() {
    let seen = Arc::new(StdMutex::new(Vec::new()));
    let s = seen.clone();
    let render = shared_render(move |l, _, t| s.lock().unwrap().push((l.len(), *t)));
    let stats = Arc::new(HostStats::default());
    let mut pipe = RenderPipe::with_max_block(render.clone(), 48_000, 48_000, 100, stats.clone());
    let at = Instant::now();
    let mut out = [0f32; 250 * 2];
    pipe.fill_at(&mut out, 2, at, Duration::from_millis(3));
    let native = std::mem::take(&mut *seen.lock().unwrap());
    let offsets: Vec<(usize, u64)> = native.iter().map(|(n, t)| (*n, t.frame_offset)).collect();
    assert_eq!(offsets, [(100, 0), (100, 100), (50, 200)]);
    for (_, t) in &native {
        assert_eq!(t.callback, at);
        assert_eq!(t.context_rate, 48_000);
        assert_eq!(t.output_latency, Duration::from_millis(3));
    }
    let expect = at + Duration::from_nanos(200 * 1_000_000_000 / 48_000);
    assert_eq!(native[2].1.chunk_instant(), expect);

    // Resampling: the latency adds how far the input runs ahead of the output.
    let mut pipe = RenderPipe::new(render, 48_000, 44_100, stats);
    pipe.fill_at(&mut out, 2, at, Duration::ZERO);
    seen.lock().unwrap().clear();
    pipe.fill_at(&mut out, 2, at, Duration::from_millis(3));
    let t = seen.lock().unwrap()[0].1;
    let lead_frames = (t.output_latency - Duration::from_millis(3)).as_secs_f64() * 48_000.0;
    let lat = pipe.resampler_latency_frames() as f64;
    assert!(
        (lat - 1.0..=lat + 2.0).contains(&lead_frames),
        "lead {lead_frames} frames, latency {lat}"
    );
}

#[test]
fn render_panic_is_caught_and_latches_silence() {
    for dev in [48_000u32, 44_100] {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let c = calls.clone();
        let render = shared_render(move |l, r, _| {
            let n = c.fetch_add(1, Ordering::Relaxed);
            l.fill(0.5);
            r.fill(0.5);
            if n == 1 {
                panic!("render bug (expected by the test)");
            }
        });
        let stats = Arc::new(HostStats::default());
        let mut pipe = RenderPipe::with_max_block(render.clone(), 48_000, dev, 64, stats.clone());
        let mut out = [0f32; 1000 * 2];
        pipe.fill(&mut out, 2); // calls #0 and #1 (panics): no unwinding into the caller
        let s = stats.snapshot();
        assert_eq!((s.render_panics, s.render_silenced), (1, true));
        let calls_after_panic = calls.load(Ordering::Relaxed);
        assert_eq!(
            calls_after_panic, 2,
            "the rest of the callback skips the render"
        );
        pipe.fill(&mut out, 2);
        assert!(
            out.iter().all(|&x| x == 0.0),
            "silent after the panic ({dev})"
        );
        assert_eq!(calls.load(Ordering::Relaxed), calls_after_panic);
        assert!(render.try_lock().is_some(), "the lock was released");
        stats.clear_render_panic();
        pipe.fill(&mut out, 2);
        assert!(calls.load(Ordering::Relaxed) > calls_after_panic);
        assert!(
            out[out.len() - 2] > 0.4,
            "audible again after clear ({dev})"
        );
        assert_eq!(stats.snapshot().skipped, 0);
    }
}

#[test]
fn the_stereo_pair_can_sit_on_other_channels() {
    let (l, r) = ([0.5f32, -0.5], [0.25f32, -0.25]);
    let mut six = [9f32; 12];
    interleave_at(&mut six, 6, 2, &l, &r);
    assert_eq!(
        six,
        [
            0.0, 0.0, 0.5, 0.25, 0.0, 0.0, 0.0, 0.0, -0.5, -0.25, 0.0, 0.0
        ]
    );
    // an offset past the last pair is clamped to it
    let mut st = [9f32; 4];
    interleave_at(&mut st, 2, 4, &l, &r);
    assert_eq!(st, [0.5, 0.25, -0.5, -0.25]);
}
