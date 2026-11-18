//! Opens the default output device (or `--device <name>`), plays a short click every 500 ms for
//! 3 s and prints the device, sample rate, requested / actual buffer size and latency.
//!
//!   cargo run -p mdaw-audio-host --example latency_probe -- [--buffer 256] [--device NAME] [--list]

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use mdaw_audio_host::{HostEvent, OutputHost, OutputOptions, list_output_devices, shared_render};

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

    // The sample rate is only known once the stream is open: the callback reads it from here.
    let rate = Arc::new(AtomicU32::new(0));
    let r = rate.clone();
    let mut pos: u64 = 0;
    let render = shared_render(move |left, right| {
        let sr = r.load(Ordering::Relaxed) as u64;
        if sr == 0 {
            return;
        }
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
        ..Default::default()
    };
    let host = match OutputHost::start(options, render) {
        Ok(h) => h,
        Err(e) => {
            println!("could not open an output device: {e}");
            std::process::exit(1);
        }
    };
    let info = host.info().expect("stream info");
    rate.store(info.sample_rate, Ordering::Relaxed);
    println!(
        "opened: {} — {} Hz, {} ch, {}, requested {:?} (device range {:?}), follows default: {}",
        info.device_name,
        info.sample_rate,
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
        s.frames as f64 / info.sample_rate as f64,
        s.last_frames,
        s.min_frames,
        s.max_frames
    );
    println!(
        "buffer latency: {:.2} ms; device output latency (callback → playback): {:.2} ms; skipped blocks: {}",
        s.last_frames as f64 * 1000.0 / info.sample_rate as f64,
        s.output_latency_ns as f64 / 1e6,
        s.skipped
    );
    for e in events {
        println!("event: {e:?}");
    }
}
