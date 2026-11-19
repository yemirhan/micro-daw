//! MIDI → engine, end to end, on the real engine core rendered offline:
//! - parsed MIDI bytes through the shared MIDI state (router + `EngineLink`) play pads and the
//!   live synth, and the audio is checked;
//! - (timestamps, stalls, backpressure and recording: `tests/live_timing.rs`)
//! - knob CCs and legacy CCs drive the doc / master volume / live effects through [`MidiUi`];
//! - a **virtual MIDI port** (CoreMIDI / ALSA) feeds a real midir connection: note in → audio
//!   out, then the port disappears (hot-plug) and its held notes are released. It needs a MIDI
//!   backend, so it is `#[ignore]`d (run with `cargo test -p mdaw-midi -- --ignored`) and fails,
//!   rather than passing silently, when MIDI is unavailable. The external-process hot-plug test
//!   is `tests/hotplug_external.rs`.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use mdaw_engine::buffers::PadBufferCache;
use mdaw_engine::core::Core;
use mdaw_engine::engine::{ArrangementEngine, EngineEvent, NoSamples};
use mdaw_midi::{MidiHost, MidiUi, build};
use mdaw_model::controller::{MemoryProfileStore, create_default_profile, serialize_profile};

const SR: u32 = 48_000;

struct Setup {
    engine: ArrangementEngine,
    core: Core,
    host: MidiHost<MemoryProfileStore>,
    ui: MidiUi,
}

fn setup(store: MemoryProfileStore) -> Setup {
    let (engine, core, live) =
        ArrangementEngine::new(SR, PadBufferCache::with_fs(SR, None), Arc::new(NoSamples));
    let epoch = Instant::now();
    let (host, ui) = build(&engine, live, store, move || {
        epoch.elapsed().as_secs_f64() * 1000.0
    });
    Setup {
        engine,
        core,
        host,
        ui,
    }
}

impl Setup {
    /// Render `seconds` offline (one UI tick per 128 frames); returns the left channel.
    fn render(&mut self, seconds: f64) -> Vec<f32> {
        let n = (seconds * f64::from(SR) / 128.0).ceil() as usize;
        let mut out = Vec::with_capacity(n * 128);
        for _ in 0..n {
            let (mut l, mut r) = ([0f32; 128], [0f32; 128]);
            self.core.process(&mut l, &mut r);
            self.engine.tick();
            self.ui.pump(&mut self.engine);
            out.extend_from_slice(&l);
        }
        out
    }

    fn send(&self, bytes: &[u8]) {
        self.host
            .shared()
            .lock()
            .on_message(None, bytes, Instant::now());
    }
}

fn rms(x: &[f32]) -> f64 {
    if x.is_empty() {
        return 0.0;
    }
    (x.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / x.len() as f64).sqrt()
}

#[test]
fn pads_and_the_live_synth_sound_through_the_router() {
    let mut s = setup(MemoryProfileStore::default());
    s.engine.live().start();
    let silent = s.render(0.05);
    assert!(rms(&silent) < 1e-6);
    // pad 3 (note 39, any channel) on channel 10: drumEngine's fallback factory kit
    s.send(&[0x99, 39, 127]);
    let pad = s.render(0.2);
    assert!(rms(&pad) > 1e-3, "pad rms {}", rms(&pad));
    s.send(&[0x89, 39, 0]);
    s.render(1.5);
    let hits: Vec<_> = s
        .engine
        .take_ui_events()
        .into_iter()
        .filter(|e| matches!(e, EngineEvent::PadHit { pad: 3, .. }))
        .collect();
    assert_eq!(hits.len(), 1);
    let tail = s.render(0.2);
    assert!(rms(&tail) < 1e-4, "tail {}", rms(&tail));
    // a key (unmapped note) plays the live synth; note-off releases it
    s.send(&[0x90, 60, 100]);
    let note = s.render(0.3);
    assert!(rms(&note) > 1e-3, "synth rms {}", rms(&note));
    assert_eq!(s.host.shared().lock().router.sounding_notes(), vec![60]);
    s.send(&[0x80, 60, 0]);
    s.render(3.0);
    let after = s.render(0.2);
    assert!(rms(&after) < 1e-4, "released synth {}", rms(&after));
    // panic (CC 123) silences a held + sustained note
    s.send(&[0xB0, 64, 127]);
    s.send(&[0x90, 62, 100]);
    s.send(&[0x80, 62, 0]);
    s.render(0.2);
    assert_eq!(s.host.shared().lock().router.sounding_notes(), vec![62]);
    s.send(&[0xB0, 123, 0]);
    assert!(s.host.shared().lock().router.sounding_notes().is_empty());
    s.render(3.0);
    assert!(rms(&s.render(0.2)) < 1e-4);
}

#[test]
fn pad_mute_records_but_is_silent() {
    let mut s = setup(MemoryProfileStore::default());
    s.host
        .shared()
        .lock()
        .with_router(|r, out| r.set_muted(true, out));
    s.send(&[0x99, 36, 127]);
    let a = s.render(0.2);
    assert!(rms(&a) < 1e-6);
    let hits = s
        .engine
        .take_ui_events()
        .into_iter()
        .filter(|e| matches!(e, EngineEvent::PadHit { pad: 0, .. }))
        .count();
    assert_eq!(hits, 1, "the hit is still captured (flash / recording)");
}

#[test]
fn knobs_and_legacy_ccs_drive_the_engine() {
    let mut s = setup(MemoryProfileStore::default());
    let bpm0 = s.engine.doc().get_bpm();
    assert_eq!(bpm0, 120.0);
    // tempo knob (CC 75): pick up near 120 BPM, then sweep to the top
    s.send(&[0xB0, 75, 127]); // far from 120: soft takeover holds it back
    s.render(0.01);
    assert_eq!(s.engine.doc().get_bpm(), 120.0);
    for v in [64u8, 80, 100, 127] {
        s.send(&[0xB0, 75, v]);
    }
    s.render(0.01);
    assert_eq!(s.engine.doc().get_bpm(), 180.0);
    // master volume knob (CC 77) from the −12 dB default
    for v in [89u8, 100, 127] {
        s.send(&[0xB0, 77, v]);
    }
    s.render(0.01);
    assert_eq!(s.engine.get_master_volume_db(), 0.0);
    // swing knob (CC 74): soft takeover from 0
    for v in [0u8, 50, 127] {
        s.send(&[0xB0, 74, v]);
    }
    s.render(0.01);
    assert_eq!(s.engine.doc().get_loop_settings().swing, 0.75);
    // pad pitch knob (CC 70) on the selected pad of the drum track the first message creates
    // (that changes the pickup context, so the knob is picked up again: TS behaviour)
    for v in [64u8, 64, 100, 127] {
        s.send(&[0xB0, 70, v]);
    }
    s.render(0.01);
    let track = s.engine.doc().get_active_drum_track_id().unwrap();
    assert_eq!(s.engine.doc().get_kit(&track).pads[0].pitch, 24.0);
    // one undo step per gesture: undo restores the pitch
    std::thread::sleep(Duration::from_millis(450));
    s.render(0.01);
    s.engine.edit(|d| d.perform_undo());
    assert_eq!(s.engine.doc().get_kit(&track).pads[0].pitch, 0.0);
}

#[test]
fn legacy_effect_ccs_with_an_empty_knob_map() {
    let mut p = create_default_profile();
    p.cc_mappings.clear();
    let store = MemoryProfileStore {
        value: Some(serialize_profile(&p)),
    };
    let mut s = setup(store);
    // CC 3 = reverb wet (default 0.12): pick up at 15/127, then move
    for v in [15u8, 64] {
        s.send(&[0xB0, 3, v]);
    }
    s.render(0.01);
    assert_eq!(s.engine.live_effect_params().reverb_wet, 64.0 / 127.0);
    // CC 5 = master volume (no pickup)
    s.send(&[0xB0, 5, 0]);
    s.render(0.01);
    assert_eq!(s.engine.get_master_volume_db(), -40.0);
}

/// Virtual port end to end (CoreMIDI on macOS, ALSA on Linux).
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
#[ignore = "needs a MIDI backend (CoreMIDI / ALSA); run with --ignored"]
fn virtual_port_end_to_end() {
    use midir::MidiOutput;
    use midir::os::unix::VirtualOutput;

    let mut s = setup(MemoryProfileStore::default());
    mdaw_midi::notify::ensure_started();
    let name = format!("mdaw-e2e-{}", std::process::id());
    let out = MidiOutput::new("mdaw-e2e").expect("no MIDI backend on this machine");
    let mut port = out
        .create_virtual(&name)
        .expect("virtual ports unsupported");
    let woke = Arc::new(AtomicUsize::new(0));
    let w = woke.clone();
    s.host.set_waker(Arc::new(move || {
        w.fetch_add(1, Ordering::Relaxed);
    }));
    s.host.set_poll_interval(Duration::from_millis(50));
    assert!(s.host.enable(None), "midir unavailable");
    let id = s
        .host
        .devices()
        .iter()
        .find(|d| d.name.contains(&name))
        .map(|d| d.id.clone())
        .expect("virtual port listed");
    s.host.select_device(&id);
    assert!(s.host.is_connected());
    assert_eq!(s.host.active_device_id(), Some(id.as_str()));
    let seen = Arc::new(AtomicUsize::new(0));
    let c = seen.clone();
    // raw listeners run on the UI thread, in `poll`
    s.host.add_raw_listener(Box::new(move |_, _| {
        c.fetch_add(1, Ordering::Relaxed);
    }));
    let wait = |s: &mut Setup, n: usize| {
        let t = Instant::now();
        while seen.load(Ordering::Relaxed) < n && t.elapsed() < Duration::from_secs(3) {
            s.host.poll();
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(seen.load(Ordering::Relaxed) >= n, "messages not delivered");
    };
    s.render(0.05);
    port.send(&[0x99, 38, 120]).unwrap();
    wait(&mut s, 1);
    let a = s.render(0.2);
    assert!(
        rms(&a) > 1e-3,
        "virtual-port pad hit not heard: {}",
        rms(&a)
    );
    port.send(&[0x90, 64, 100]).unwrap();
    wait(&mut s, 2);
    s.render(0.1);
    assert_eq!(s.host.shared().lock().router.sounding_notes(), vec![64]);
    // unplug while the key is held: hot-plug detects it, the held note is released
    drop(port);
    let t = Instant::now();
    while !s.host.poll() && t.elapsed() < Duration::from_secs(3) {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        woke.load(Ordering::Relaxed) > 0,
        "watcher never woke the UI"
    );
    assert!(s.host.devices().iter().all(|d| d.id != id));
    assert_ne!(s.host.active_device_id(), Some(id.as_str()));
    assert!(s.host.shared().lock().router.sounding_notes().is_empty());
    s.host.dispose();
}
