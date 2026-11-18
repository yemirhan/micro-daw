//! Live MIDI timing, robustness and recording on the real engine core (phase-5 review fixes,
//! `docs/rune-port/review-phase-5.md`):
//! - timestamps with **continuously advancing audio** (one device callback every 256 frames at
//!   its wall-clock instant, messages delivered between callbacks): on-time events keep their
//!   spacing with a constant latency; a burst delivered after its budget plays as soon as
//!   possible and is counted late; a burst within the budget keeps its spacing;
//! - a device **stall / outage**: events during it, and before the first resumed callback, are
//!   not stamped into the future, and later events are not held back;
//! - the core's **live lookahead clamp** and the producer's rebase of a far-future stamp;
//! - **backpressure**: a burst that overflows the MIDI ring is drained by the outbox retry
//!   thread with no further input, and its note-offs release the voices;
//! - **Song recording** of synth notes stamped by the core (pre-record key, a capture whose UI
//!   frame comes after the punch loop wrapped, a stop before the UI pumped);
//! - a lost input lifts its **sustain pedal** (Electron bug fixed in both apps);
//! - how long the UI holds the MIDI lock, and the MIDI thread's work per message.

use std::sync::Arc;
use std::time::{Duration, Instant};

use mdaw_engine::buffers::PadBufferCache;
use mdaw_engine::core::{BlockClock, Core};
use mdaw_engine::engine::{ArrangementEngine, EngineEvent, NoSamples};
use mdaw_midi::{MidiHost, MidiUi, build};
use mdaw_model::controller::MemoryProfileStore;

const SR: u32 = 48_000;
const BLOCK: usize = 256;

fn block_dur() -> Duration {
    Duration::from_secs_f64(BLOCK as f64 / f64::from(SR))
}

/// One device block + one render quantum.
fn latency() -> f64 {
    (BLOCK + 128) as f64 / f64::from(SR)
}

const FRAME: f64 = 1.0 / SR as f64;

struct Rig {
    engine: ArrangementEngine,
    core: Core,
    host: MidiHost<MemoryProfileStore>,
    ui: MidiUi,
    t0: Instant,
    /// Wall-clock position of the next callback (moves on during a stall).
    wall: Duration,
}

impl Rig {
    fn new() -> Self {
        let (engine, core, live) =
            ArrangementEngine::new(SR, PadBufferCache::with_fs(SR, None), Arc::new(NoSamples));
        let epoch = Instant::now();
        let (host, ui) = build(&engine, live, MemoryProfileStore::default(), move || {
            epoch.elapsed().as_secs_f64() * 1000.0
        });
        Self {
            engine,
            core,
            host,
            ui,
            t0: Instant::now() + Duration::from_secs(1),
            wall: Duration::ZERO,
        }
    }

    fn now(&self) -> Instant {
        self.t0 + self.wall
    }

    /// One device callback at the current wall instant, then the UI frame.
    fn callback(&mut self) -> Vec<f32> {
        let (mut l, mut r) = ([0f32; BLOCK], [0f32; BLOCK]);
        self.core.process_timed(
            &mut l,
            &mut r,
            BlockClock {
                instant: self.now(),
                output_latency: Duration::ZERO,
            },
        );
        self.wall += block_dur();
        self.engine.tick();
        self.ui.pump(&mut self.engine);
        l.to_vec()
    }

    /// Callbacks until the wall clock reaches `at` (since t0); returns the audio.
    fn run_until(&mut self, at: Duration) -> Vec<f32> {
        let mut out = Vec::new();
        while self.wall < at {
            out.extend(self.callback());
        }
        out
    }

    /// A message that happened at `happened` (since t0), delivered at `arrival` (since t0),
    /// stamped by a backend whose clock reads `happened + 7 s`.
    fn deliver(&self, happened: Duration, arrival: Duration, bytes: &[u8]) {
        let ts = 7_000_000 + happened.as_micros() as u64;
        self.host
            .shared()
            .lock()
            .on_message(Some(ts), bytes, self.t0 + arrival);
    }

    fn pad_hits(&mut self) -> Vec<(usize, f64)> {
        self.engine
            .take_ui_events()
            .into_iter()
            .filter_map(|e| match e {
                EngineEvent::PadHit { pad, time, .. } => Some((pad, time)),
                _ => None,
            })
            .collect()
    }

    /// Context time of a wall instant (the anchors are exact: frame k·256 at t0 + k·block).
    fn ctx(at: Duration) -> f64 {
        at.as_secs_f64()
    }
}

fn ms(x: u64) -> Duration {
    Duration::from_millis(x)
}

fn rms(x: &[f32]) -> f64 {
    if x.is_empty() {
        return 0.0;
    }
    (x.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / x.len() as f64).sqrt()
}

/// Review finding 6: on-time events keep their spacing at a constant latency; a burst
/// delivered past its budget plays at once (counted late); a burst inside the budget keeps
/// its spacing. Audio advances continuously, messages arrive between callbacks.
#[test]
fn timestamps_with_continuously_advancing_audio() {
    let mut r = Rig::new();
    r.run_until(ms(10));
    // on time: pads 0..3 at 20, 30, 40, 50 ms, each delivered 1 ms after it happened
    for (k, note) in [36u8, 37, 38, 39].into_iter().enumerate() {
        let at = ms(20 + 10 * k as u64);
        r.run_until(at + ms(1));
        r.deliver(at, at + ms(1), &[0x99, note, 100]);
        r.deliver(at, at + ms(1), &[0x89, note, 0]);
    }
    r.run_until(ms(90));
    let on_time = r.pad_hits();
    assert_eq!(on_time.len(), 4, "{on_time:?}");
    // the first message calibrates the origin (offset = its delivery delay, 1 ms)
    for (k, (pad, t)) in on_time.iter().enumerate() {
        assert_eq!(*pad, k);
        let want = Rig::ctx(ms(21 + 10 * k as u64)) + latency();
        assert!((t - want).abs() <= 1.5 * FRAME, "pad {pad}: {t} vs {want}");
    }
    // late burst: 100, 110, 120 ms delivered together at 145 ms (budget 8 ms)
    r.run_until(ms(145));
    // as soon as possible = the next quantum the core renders
    let next_quantum = r.core.shared().snapshot().now();
    for (k, note) in [40u8, 41, 42].into_iter().enumerate() {
        r.deliver(ms(100 + 10 * k as u64), ms(145), &[0x99, note, 100]);
    }
    let (stats, _) = r.host.stats();
    assert_eq!(stats.late_events, 3);
    assert!(stats.max_late_ms > 35.0, "{stats:?}");
    // inside the budget: 200, 202, 204 ms delivered together at 205 ms
    r.run_until(ms(205));
    for (k, note) in [43u8, 44, 45].into_iter().enumerate() {
        r.deliver(ms(200 + 2 * k as u64), ms(205), &[0x99, note, 100]);
    }
    r.run_until(ms(300));
    let hits = r.pad_hits();
    let late: Vec<f64> = hits.iter().filter(|(p, _)| *p < 7).map(|h| h.1).collect();
    assert_eq!(late.len(), 3);
    assert!(
        late.iter().all(|t| (t - late[0]).abs() <= FRAME),
        "{late:?}"
    );
    assert!(
        (late[0] - next_quantum).abs() <= FRAME,
        "{late:?} vs {next_quantum}"
    );
    let inside: Vec<f64> = hits.iter().filter(|(p, _)| *p >= 7).map(|h| h.1).collect();
    assert_eq!(inside.len(), 3);
    for (k, t) in inside.iter().enumerate() {
        let want = Rig::ctx(ms(201 + 2 * k as u64)) + latency();
        assert!(
            (t - want).abs() <= 1.5 * FRAME,
            "inside budget {k}: {t} vs {want}"
        );
    }
    assert_eq!(r.host.stats().0.late_events, 3);
    eprintln!(
        "on time: latency {:.2} ms, spacing kept; late burst: 3 late, max {:.1} ms, one quantum",
        latency() * 1e3,
        r.host.stats().0.max_late_ms
    );
}

/// Review finding 1: a 2 s stall and a 5 s outage. An event during the stall (before the first
/// resumed callback) plays right after the device resumes; events after the resume are not held
/// back behind it. HEAD: 2 s stall → ~1.5 s late; 5 s outage → no hits for 0.54 s after resume.
#[test]
fn a_device_stall_never_pushes_live_events_into_the_future() {
    for stall in [ms(2000), ms(5000)] {
        let mut r = Rig::new();
        r.run_until(ms(100));
        r.deliver(ms(99), ms(100), &[0x99, 36, 100]); // calibration
        r.run_until(ms(120));
        let resume_frame = r.core.shared().snapshot().frames;
        // the device stalls: no callbacks; a pad happens in the middle of it
        let stall_start = r.wall;
        r.wall += stall;
        r.deliver(
            stall_start + stall / 2,
            stall_start + stall / 2,
            &[0x99, 37, 100],
        );
        // resumed: a pad 20 ms later, delivered at once
        let resumed = r.wall;
        r.run_until(resumed + ms(20));
        r.deliver(resumed + ms(20), resumed + ms(20), &[0x99, 38, 100]);
        r.run_until(resumed + ms(1000));
        let hits = r.pad_hits();
        let t = |pad: usize| hits.iter().find(|h| h.0 == pad).map(|h| h.1).unwrap();
        let resume_t = resume_frame as f64 / f64::from(SR);
        let during = t(1) - resume_t;
        let after = t(2) - resume_t;
        eprintln!(
            "stall {:?}: event during the stall plays {:.1} ms after resume, the next one at \
             {:.1} ms (sent at 20 ms)",
            stall,
            during * 1e3,
            after * 1e3
        );
        // capped at two periods past the last anchor + the latency
        assert!(
            during <= 3.0 * BLOCK as f64 / f64::from(SR) + latency(),
            "{during}"
        );
        assert!(
            (after - (0.020 + latency())).abs() <= 2.0 * FRAME,
            "{after}"
        );
    }
}

/// Review finding 1: on a device, a far-future live stamp (from any source) is applied no later
/// than the core's lookahead, and the producer rebases it so later commands are not held back
/// longer than that.
#[test]
fn the_core_clamps_far_future_live_stamps() {
    let (mut engine, mut core, mut live) =
        ArrangementEngine::new(SR, PadBufferCache::with_fs(SR, None), Arc::new(NoSamples));
    let t0 = Instant::now();
    let mut k = 0u32;
    let mut callback = |core: &mut Core, engine: &mut ArrangementEngine| {
        let (mut l, mut r) = ([0f32; BLOCK], [0f32; BLOCK]);
        core.process_timed(
            &mut l,
            &mut r,
            BlockClock {
                instant: t0 + block_dur() * k,
                output_latency: Duration::ZERO,
            },
        );
        k += 1;
        engine.tick();
    };
    callback(&mut core, &mut engine);
    live.pad_down(0, 0.8, Some(10.0));
    live.pad_up(0, Some(10.0));
    live.pad_down(1, 0.8, None);
    for _ in 0..(SR as usize / BLOCK) {
        callback(&mut core, &mut engine);
        live.flush();
    }
    let hits: Vec<(usize, f64)> = engine
        .take_ui_events()
        .into_iter()
        .filter_map(|e| match e {
            EngineEvent::PadHit { pad, time, .. } => Some((pad, time)),
            _ => None,
        })
        .collect();
    assert_eq!(hits.len(), 2, "{hits:?}");
    for (_, t) in &hits {
        assert!(
            *t <= mdaw_engine::core::MAX_LIVE_LOOKAHEAD + 0.01,
            "{hits:?}"
        );
    }
    eprintln!("far-future stamps (10 s) applied at {hits:?} (HEAD: 10 s)");
}

/// Review finding 2: a burst that overflows the MIDI ring (1024 commands) while the audio
/// thread is not running, then silence: the outbox retry thread delivers the rest, so the
/// final note-offs reach the core and the voices are released. HEAD: 1,378 commands stayed
/// queued and notes 60 / 61 kept sounding.
#[test]
fn a_saturating_burst_is_drained_without_further_input() {
    let mut r = Rig::new();
    r.engine.live().start();
    {
        let shared = r.host.shared().clone();
        let mut s = shared.lock();
        for _ in 0..250 {
            for b in [
                [0x90, 60, 100],
                [0x90, 61, 100],
                [0x80, 60, 0],
                [0x80, 61, 0],
            ] {
                s.on_message(None, &b, Instant::now());
            }
        }
    }
    let queued_after_burst = r.host.shared().lock().link.queued();
    assert!(queued_after_burst > 500, "{queued_after_burst}");
    // no more input; the audio thread runs
    let deadline = Instant::now() + Duration::from_secs(5);
    while r.host.shared().lock().link.queued() > 0 && Instant::now() < deadline {
        for _ in 0..4 {
            let (mut l, mut rr) = ([0f32; 128], [0f32; 128]);
            r.core.process(&mut l, &mut rr);
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(r.host.shared().lock().link.queued(), 0);
    let mut tail = Vec::new();
    for _ in 0..(4 * SR as usize / 128) {
        let (mut l, mut rr) = ([0f32; 128], [0f32; 128]);
        r.core.process(&mut l, &mut rr);
        tail.extend_from_slice(&l);
    }
    let last = &tail[tail.len() - 9600..];
    assert!(rms(last) < 1e-5, "voices still sounding: {}", rms(last));
    assert!(r.host.shared().lock().router.sounding_notes().is_empty());
    eprintln!("burst: {queued_after_burst} commands queued → 0, synth silent");
}

/// Render quanta on the audio thread only (the UI does not run).
fn audio_only(r: &mut Rig, quanta: usize) {
    for _ in 0..quanta {
        let (mut l, mut rr) = ([0f32; 128], [0f32; 128]);
        r.core.process(&mut l, &mut rr);
    }
}

fn ui_frame(r: &mut Rig) {
    r.engine.tick();
    r.ui.pump(&mut r.engine);
}

/// Review finding 3: synth captures are stamped by the core at the note's frame.
#[test]
fn synth_captures_are_stamped_by_the_core() {
    let send = |r: &Rig, b: [u8; 3]| {
        r.host.shared().lock().on_message(None, &b, Instant::now());
    };
    // (a) punch 4–8: a key at beat ≈ 7.89 whose UI frame comes after the loop wrapped
    let mut r = Rig::new();
    r.engine.edit(|d| {
        d.set_loop_enabled(true);
        d.set_loop_markers(4.0, 8.0);
    });
    r.engine.start_recording_to_track(None);
    assert!(r.engine.is_punch_recording_active());
    while r.core.shared().snapshot().beat < 7.89 {
        audio_only(&mut r, 1);
        ui_frame(&mut r);
    }
    send(&r, [0x90, 60, 100]);
    let at_beat = r.core.shared().snapshot().beat;
    // the UI stalls across the wrap
    while r.core.shared().snapshot().beat >= 7.0 {
        audio_only(&mut r, 1);
    }
    audio_only(&mut r, 40);
    ui_frame(&mut r);
    send(&r, [0x80, 60, 0]);
    audio_only(&mut r, 4);
    // (stop before the UI pumped the note-off: the stop pumps the core events first)
    let region = r.engine.stop_recording_to_track().unwrap();
    let n = region
        .notes
        .iter()
        .find(|n| n.note == 60.0)
        .expect("note recorded");
    eprintln!(
        "punch capture: start {:.4} (key at transport beat {at_beat:.4})",
        n.start_beat
    );
    assert!(
        (n.start_beat - (at_beat - 4.0)).abs() < 0.01,
        "start {} (HEAD: −0.1067)",
        n.start_beat
    );
    // (b) a key pressed before the recording started is not recorded; one after is, and a
    // stop with captures not yet pumped keeps them
    let mut r = Rig::new();
    send(&r, [0x90, 62, 100]);
    audio_only(&mut r, 8);
    r.engine.start_recording_to_track(None); // the UI did not pump the key's capture
    audio_only(&mut r, 100);
    ui_frame(&mut r);
    send(&r, [0x80, 62, 0]);
    send(&r, [0x90, 64, 100]);
    audio_only(&mut r, 50);
    send(&r, [0x80, 64, 0]);
    audio_only(&mut r, 50);
    let region = r.engine.stop_recording_to_track().unwrap();
    let notes: Vec<f64> = region.notes.iter().map(|n| n.note).collect();
    assert_eq!(notes, vec![64.0], "{:?}", region.notes);
    let n = &region.notes[0];
    assert!(n.duration_beats > 0.12, "{n:?}");
}

/// Review finding 10 (Electron bug fixed in both apps): losing the input lifts its pedal and
/// releases the notes it sustained, on the real synth.
#[test]
fn losing_the_input_releases_sustained_notes() {
    let mut r = Rig::new();
    r.engine.live().start();
    let send = |r: &Rig, b: [u8; 3]| {
        r.host.shared().lock().on_message(None, &b, Instant::now());
    };
    send(&r, [0xB0, 64, 127]);
    send(&r, [0x90, 60, 100]);
    send(&r, [0x80, 60, 0]); // sustained
    send(&r, [0x90, 64, 100]); // held
    let a = r.run_until(r.wall + ms(300));
    assert!(rms(&a) > 1e-3);
    assert_eq!(r.host.shared().lock().router.sounding_notes(), vec![60, 64]);
    // what `MidiHost` does when the input is lost (switch, unplug, dispose)
    r.host.shared().lock().release_held_notes();
    assert!(r.host.shared().lock().router.sounding_notes().is_empty());
    assert!(!r.host.shared().lock().router.sustain_on());
    r.run_until(r.wall + ms(4000));
    let tail = r.run_until(r.wall + ms(200));
    assert!(
        rms(&tail) < 1e-5,
        "sustained voices still ring: {}",
        rms(&tail)
    );
}

/// Review finding 4: what the UI does under the MIDI lock is short, and so is the MIDI
/// thread's work per message (no disk, no formatting, no listeners).
#[test]
fn ui_lock_hold_is_short() {
    let mut r = Rig::new();
    r.engine.live().start();
    let shared = r.host.shared().clone();
    // MIDI-thread work per message (router + engine link), max over 4000 mixed messages
    let mut per_msg = Vec::with_capacity(4000);
    for k in 0..1000u32 {
        for b in [
            [0x99, 36 + (k % 16) as u8, 100],
            [0x90, 48 + (k % 24) as u8, 90],
            [0xB0, 70 + (k % 8) as u8, (k % 128) as u8],
            [0x80, 48 + (k % 24) as u8, 0],
        ] {
            let t = Instant::now();
            shared.lock().on_message(None, &b, Instant::now());
            per_msg.push(t.elapsed());
        }
        if k % 16 == 0 {
            audio_only(&mut r, 8);
            ui_frame(&mut r);
        }
    }
    // UI operations under the lock: panic with 40 voices + 16 pads held, a pad learn that
    // saves the profile (queued, not written), mode / mute / split changes, on-screen keys
    {
        let mut s = shared.lock();
        for n in 30..70u8 {
            s.on_message(None, &[0x90, n, 100], Instant::now());
        }
    }
    audio_only(&mut r, 16);
    let mut worst_ui = Duration::ZERO;
    let mut timed = |f: &mut dyn FnMut()| {
        let t = Instant::now();
        f();
        worst_ui = worst_ui.max(t.elapsed());
    };
    timed(&mut || {
        shared.lock().with_router(|r, out| r.all_notes_off(out));
    });
    timed(&mut || {
        let mut s = shared.lock();
        s.router
            .mapping
            .start_pad_learn(mdaw_model::controller::PadLearn::Pad(3));
        s.on_message(None, &[0x90, 99, 100], Instant::now());
    });
    timed(&mut || {
        shared.lock().with_router(|r, out| {
            r.set_routing_mode(mdaw_midi::RoutingMode::Split);
            r.set_split_point(48);
            r.set_muted(true, out);
            r.set_muted(false, out);
            r.note_on(72, 0.5, out);
            r.note_off(72, out);
        });
    });
    per_msg.sort_unstable();
    let pct = |p: f64| per_msg[((per_msg.len() - 1) as f64 * p) as usize];
    let (median, p99, worst_msg) = (pct(0.5), pct(0.99), *per_msg.last().unwrap());
    eprintln!(
        "MIDI thread per message: median {:.1} µs, p99 {:.1} µs, max {:.1} µs; UI under the \
         lock: worst {:.1} µs",
        median.as_secs_f64() * 1e6,
        p99.as_secs_f64() * 1e6,
        worst_msg.as_secs_f64() * 1e6,
        worst_ui.as_secs_f64() * 1e6
    );
    // generous bounds (debug build, other tests running in parallel: the max includes
    // preemption, so the bound is on the 99th percentile)
    assert!(p99 < Duration::from_millis(2), "{p99:?}");
    assert!(worst_ui < Duration::from_millis(10), "{worst_ui:?}");
}
