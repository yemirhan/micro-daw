use super::*;
use crate::shared_render;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

fn options(context_rate: Option<u32>) -> OutputOptions {
    OutputOptions {
        poll_interval: Duration::from_millis(20),
        context_rate,
        ..Default::default()
    }
}

fn wait_for(what: &str, cond: impl Fn() -> bool) {
    let t0 = Instant::now();
    while !cond() {
        assert!(t0.elapsed() < Duration::from_secs(10), "timed out: {what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn defaults_enable_the_null_fallback() {
    let o = OutputOptions::default();
    assert!(o.null_fallback);
    assert_eq!(o.context_rate, None);
}

#[test]
fn no_device_keeps_the_core_rendering_through_the_null_fallback() {
    let frames = Arc::new(AtomicU64::new(0));
    let rate = Arc::new(AtomicU32::new(0));
    let (f, r) = (frames.clone(), rate.clone());
    let render = shared_render(move |l, _, t| {
        r.store(t.context_rate, Ordering::Relaxed);
        f.fetch_add(l.len() as u64, Ordering::Relaxed);
    });
    let host = OutputHost::start_failing(options(None), render).unwrap();
    assert_eq!(host.info(), None);
    assert_eq!(host.context_rate(), DEFAULT_CONTEXT_RATE);
    wait_for("fallback renders", || {
        frames.load(Ordering::Relaxed) >= 2048
    });
    assert_eq!(rate.load(Ordering::Relaxed), DEFAULT_CONTEXT_RATE);
    // Retries keep failing (several poll intervals): no event flapping.
    host.set_device(Some("nope".into()));
    std::thread::sleep(Duration::from_millis(100));
    let events: Vec<HostEvent> = host.events().try_iter().collect();
    assert_eq!(
        events,
        [
            HostEvent::OpenFailed(HostError::NoDevice.to_string()),
            HostEvent::NullFallback { active: true }
        ]
    );
    host.stop();
    // Stopped and joined: the render callback is never called again.
    let after = frames.load(Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(frames.load(Ordering::Relaxed), after);
}

#[test]
fn null_fallback_runs_at_the_configured_context_rate() {
    let rate = Arc::new(AtomicU32::new(0));
    let r = rate.clone();
    let render = shared_render(move |_, _, t| r.store(t.context_rate, Ordering::Relaxed));
    let host = OutputHost::start_failing(options(Some(44_100)), render).unwrap();
    assert_eq!(host.context_rate(), 44_100);
    wait_for("fallback renders", || rate.load(Ordering::Relaxed) != 0);
    assert_eq!(rate.load(Ordering::Relaxed), 44_100);
}

#[test]
fn without_the_fallback_a_failed_open_is_an_error() {
    let o = OutputOptions {
        null_fallback: false,
        ..options(None)
    };
    let called = Arc::new(AtomicBool::new(false));
    let c = called.clone();
    let render = shared_render(move |_, _, _| c.store(true, Ordering::Relaxed));
    let err = OutputHost::start_failing(o, render).err();
    assert_eq!(err, Some(HostError::NoDevice));
    std::thread::sleep(Duration::from_millis(20));
    assert!(!called.load(Ordering::Relaxed));
}

#[test]
fn render_panic_is_reported_once_and_can_be_cleared() {
    let calls = Arc::new(AtomicU64::new(0));
    let c = calls.clone();
    let render = shared_render(move |_, _, _| {
        if c.fetch_add(1, Ordering::Relaxed) == 3 {
            panic!("render bug (expected by the test)");
        }
    });
    let host = OutputHost::start_failing(options(None), render).unwrap();
    wait_for("RenderPanicked", || {
        host.events()
            .try_iter()
            .any(|e| e == HostEvent::RenderPanicked)
    });
    let s = host.stats();
    assert_eq!((s.render_panics, s.render_silenced), (1, true));
    let frozen = calls.load(Ordering::Relaxed);
    assert_eq!(frozen, 4, "silenced after the panic: no more render calls");
    std::thread::sleep(Duration::from_millis(60));
    assert_eq!(calls.load(Ordering::Relaxed), frozen);
    assert!(
        !host
            .events()
            .try_iter()
            .any(|e| e == HostEvent::RenderPanicked),
        "reported once"
    );
    host.clear_render_panic();
    wait_for("render resumes", || calls.load(Ordering::Relaxed) > frozen);
    assert!(!host.stats().render_silenced);
}

#[test]
fn stream_info_latency_uses_the_device_rate() {
    let info = StreamInfo {
        device_name: "x".into(),
        device_rate: 44_100,
        context_rate: 48_000,
        resampling: true,
        channels: 2,
        sample_format: "f32".into(),
        buffer: BufferRequest::Fixed(441),
        buffer_size_range: None,
        follows_default: true,
    };
    assert_eq!(info.nominal_buffer_latency_secs(), Some(0.01));
}
