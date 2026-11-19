//! Review findings 7 and 8 end to end: the real core (metronome at 120 BPM) at a fixed context
//! rate of 48 kHz, driven through the output host's `RenderPipe` at other device rates (the
//! resampling path a device switch 44.1k ↔ 48k now takes). The tempo measured from the clicks in
//! the device-rate output must stay exactly 120 BPM (the reviewer measured 130.35 BPM after the
//! switch before the fix), and every callback publishes its wall-clock anchor through the glue
//! the app uses (`BlockTiming` → `BlockClock` → `Core::process_timed`).

use std::sync::Arc;
use std::time::{Duration, Instant};

use mdaw_audio_host::{BlockTiming, HostStats, RenderPipe, shared_render};
use mdaw_engine::core::{BlockClock, Core, CoreConfig};

/// Onset times (s) of the clicks in a device-rate mono signal: first sample above `thr` after at
/// least 100 ms of silence.
fn onsets(x: &[f32], rate: u32, thr: f32) -> Vec<f64> {
    let gap = rate as usize / 10;
    let mut quiet = gap;
    let mut out = Vec::new();
    for (i, s) in x.iter().enumerate() {
        if s.abs() > thr {
            if quiet >= gap {
                out.push(i as f64 / rate as f64);
            }
            quiet = 0;
        } else {
            quiet += 1;
        }
    }
    out
}

fn measure(device_rate: u32) -> (f64, usize, Option<mdaw_engine::core::ClockAnchor>) {
    let (mut core, mut h, _live) = Core::new(CoreConfig::new(48_000));
    h.set_metronome(true);
    h.start(None, 0.0);
    let render = shared_render(move |l: &mut [f32], r: &mut [f32], t: &BlockTiming| {
        core.process_timed(
            l,
            r,
            BlockClock {
                instant: t.chunk_instant(),
                output_latency: t.output_latency,
            },
        );
    });
    let mut pipe = RenderPipe::new(render, 48_000, device_rate, Arc::new(HostStats::default()));
    let block = 512usize;
    let mut out = vec![0f32; block * 2];
    let mut mono = Vec::new();
    let i0 = Instant::now();
    let seconds = 4.1;
    let mut n = 0u64;
    while (mono.len() as f64) < seconds * device_rate as f64 {
        let at = i0 + Duration::from_secs_f64(n as f64 * block as f64 / device_rate as f64);
        pipe.fill_at(&mut out, 2, at, Duration::from_millis(5));
        mono.extend(out.chunks(2).map(|f| f[0]));
        n += 1;
    }
    let on = onsets(&mono, device_rate, 0.01);
    let count = on.len();
    // least-squares slope of onset time against click index
    let k = on.len() as f64;
    let mx = (k - 1.0) / 2.0;
    let my = on.iter().sum::<f64>() / k;
    let (mut sxy, mut sxx) = (0.0, 0.0);
    for (i, t) in on.iter().enumerate() {
        sxy += (i as f64 - mx) * (t - my);
        sxx += (i as f64 - mx).powi(2);
    }
    let bpm = 60.0 / (sxy / sxx);
    (bpm, count, h.shared().snapshot().anchor)
}

#[test]
fn tempo_is_exact_at_any_device_rate() {
    for rate in [48_000, 44_100, 96_000] {
        let (bpm, clicks, anchor) = measure(rate);
        eprintln!("device {rate} Hz: {clicks} clicks, {bpm:.6} BPM");
        assert_eq!(clicks, 9, "a click every 0.5 s for 4.1 s");
        assert!((bpm - 120.0).abs() < 0.01, "device {rate}: {bpm} BPM");
        let a = anchor.expect("the callback anchor is published");
        assert!(a.block_frames > 0 && a.frame > 0);
        assert!(
            a.output_latency >= Duration::from_millis(5),
            "device latency (+ resampler lead) carried to the engine clock"
        );
    }
}
