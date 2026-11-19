//! Opens the default output device (or `--device <name>`), plays a short click every 500 ms of
//! context time (120 BPM) for 3 s and prints the device, context / device rate (and whether it
//! resamples), requested / actual buffer size and latency (device + resampler, from
//! [`BlockTiming`]).
//!
//!   cargo run -p mdaw-audio-host --example latency_probe -- [--buffer 256] [--device NAME]
//!     [--context-rate 48000] [--list]

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use mdaw_audio_host::{
    BlockTiming, HostEvent, OutputHost, OutputOptions, list_output_devices, shared_render,
};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1).cloned())
    };
    let hint: u32 = arg("--buffer").and_then(|s| s.parse().ok()).unwrap_or(256);

    match list_output_devices() {
        Ok(devices) if devices.is_empty() => println!("no output devices"),
        Ok(devices) => {
            println!("output devices:");
            for d in devices {
                println!(
                    "  {}{} — {} Hz, {} ch, {}, buffer range {:?}",
                    if d.is_default { "* " } else { "  " },
                    d.name,
                    d.default_sample_rate,
                    d.channels,
                    d.sample_format,
                    d.buffer_size_range
                );
            }
        }
        Err(e) => println!("cannot list devices: {e}"),
    }
    if args.iter().any(|a| a == "--list") {
        return;
    }

    // The render callback learns the (fixed) context rate from its timing.
    let latency_ns = Arc::new(AtomicU64::new(0));
    let lat = latency_ns.clone();
    let mut pos: u64 = 0;
    let render = shared_render(move |left, right, timing: &BlockTiming| {
        lat.store(timing.output_latency.as_nanos() as u64, Ordering::Relaxed);
        let sr = timing.context_rate as u64;
        let period = sr / 2; // a click every 500 ms
        let click_len = sr / 200; // 5 ms
        for (l, rr) in left.iter_mut().zip(right.iter_mut()) {
            let k = pos % period;
            if k < click_len {
                let t = k as f32 / sr as f32;
                let env = 1.0 - k as f32 / click_len as f32;
                let s = 0.25 * env * (2.0 * std::f32::consts::PI * 1000.0 * t).sin();
                *l = s;
                *rr = s;
            }
            pos += 1;
        }
    });

    let options = OutputOptions {
        device_name: arg("--device"),
        buffer_size_hint: Some(hint),
        context_rate: arg("--context-rate").and_then(|s| s.parse().ok()),
        ..Default::default()
    };
    let host = match OutputHost::start(options, render) {
        Ok(h) => h,
        Err(e) => {
            println!("could not open an output device: {e}");
            std::process::exit(1);
        }
    };
    let Some(info) = host.info() else {
        println!(
            "no device stream (null fallback at {} Hz)",
            host.context_rate()
        );
        std::process::exit(1);
    };
    println!(
        "opened: {} — context {} Hz, device {} Hz{}, {} ch, {}, requested {:?} (device range {:?}), follows default: {}",
        info.device_name,
        info.context_rate,
        info.device_rate,
        if info.resampling { " (resampling)" } else { "" },
        info.channels,
        info.sample_format,
        info.buffer,
        info.buffer_size_range,
        info.follows_default
    );

    std::thread::sleep(Duration::from_secs(3));
    let s = host.stats();
    let events: Vec<HostEvent> = host.events().try_iter().collect();
    host.stop();

    println!(
        "callbacks: {}, frames: {} ({:.2} s), buffer frames last/min/max: {}/{}/{}",
        s.callbacks,
        s.frames,
        s.frames as f64 / info.device_rate as f64,
        s.last_frames,
        s.min_frames,
        s.max_frames
    );
    println!(
        "buffer latency: {:.2} ms; device output latency (callback → playback): {:.2} ms; \
         render timing latency (device + resampler): {:.2} ms; skipped blocks: {}; render panics: {}",
        s.last_frames as f64 * 1000.0 / info.device_rate as f64,
        s.output_latency_ns as f64 / 1e6,
        latency_ns.load(Ordering::Relaxed) as f64 / 1e6,
        s.skipped,
        s.render_panics
    );
    for e in events {
        println!("event: {e:?}");
    }
}
