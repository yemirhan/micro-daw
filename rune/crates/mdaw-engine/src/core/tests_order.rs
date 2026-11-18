//! Ordering and delivery: the control-side outbox (nothing dropped, coalescing, atomic
//! count-in), per-producer FIFO stamping, and the same-time order of pattern notes, repeat lines
//! and live hits (on the real SamplerKit, with a choke group).

use std::sync::Arc;

use mdaw_dsp::buffer::AudioBuffer;
use mdaw_model::PAD_COUNT;

use super::test_support::*;
use super::*;
use crate::sampler::{PadBuffer, PadParams, SamplerKit};

fn caps(r: &Rig) -> Vec<PadCapture> {
    r.captures()
}

/// Review finding 1: with the core stalled, more commands than the ring holds, then Stop: every
/// command arrives, in order, and the core ends stopped (before: the ring dropped the excess and
/// Stop with it, so the engine said Stopped while the core kept playing).
#[test]
fn a_full_ring_queues_instead_of_dropping() {
    let mut r = Rig::new();
    r.h.start(None, 0.0);
    r.run(0.01);
    assert_eq!(r.h.shared().phase(), Phase::Started);
    for i in 0..550 {
        r.h.pad_down(i % PAD_COUNT, 0.5, None);
        r.h.pad_up(i % PAD_COUNT, None);
    }
    let g = r.h.stop();
    assert!(r.h.queued() > 0, "the ring (1024) is full: the rest waits");
    // one UI frame per quantum: flush, render
    for _ in 0..10 {
        r.h.flush();
        r.run(0.001);
    }
    assert_eq!(r.h.queued(), 0);
    let snap = r.h.shared().snapshot();
    assert_eq!((snap.phase, snap.generation), (Phase::Stopped, g));
    let c = caps(&r);
    assert_eq!(c.iter().filter(|c| c.kind == CaptureKind::Hit).count(), 550);
    assert_eq!(
        c.iter().filter(|c| c.kind == CaptureKind::Release).count(),
        550
    );
    assert!(
        r.events
            .iter()
            .any(|e| matches!(e, CoreEvent::Stopped { generation, .. } if *generation == g)),
        "Stop applied after every queued command"
    );
}

/// Latest-value setters coalesce while queued (a fader drag during a stall is one command),
/// but never across a transport command.
#[test]
fn queued_setters_coalesce_but_not_across_transport() {
    let mut r = Rig::new();
    for i in 0..1100 {
        r.h.set_track_gain(0, i as f32);
    }
    let queued_after_gain = r.h.queued();
    r.h.set_bpm(100.0);
    r.h.start(None, 0.0);
    r.h.set_bpm(90.0); // after the start: must not replace the 100 BPM before it
    for _ in 0..10 {
        r.h.flush();
        r.run(0.001);
    }
    assert_eq!(
        queued_after_gain, 1,
        "1023 in the ring, the other 77 gains coalesced"
    );
    assert_eq!(r.h.shared().bpm(), 90.0);
    let started = r.events.iter().find_map(|e| match e {
        CoreEvent::Started { .. } => Some(()),
        _ => None,
    });
    assert!(started.is_some());
}

/// The count-in and its transport start are one command: they can never be split by a full
/// ring.
#[test]
fn count_in_and_start_are_one_submission() {
    let mut r = Rig::new(); // (2 setup commands in the ring)
    for i in 0..1022 {
        r.h.pad_down(i % PAD_COUNT, 0.5, None);
    }
    let t0 = r.now() + 0.05;
    let (g, end) = r.h.start_with_count_in(2.0, t0, 2, 0.5);
    assert_eq!(r.h.queued(), 1, "count-in + start queued together");
    for _ in 0..30 {
        r.h.flush();
        r.run(0.05);
    }
    let started: Vec<(f64, f64, u64)> = r
        .events
        .iter()
        .filter_map(|e| match e {
            CoreEvent::Started {
                time,
                beat,
                generation,
            } => Some((*time, *beat, *generation)),
            _ => None,
        })
        .collect();
    assert_eq!(started, [(r.frame_of(end) as f64 / 48_000.0, 2.0, g)]);
}

/// Review finding 2 (Opus H2): `pad_down(t = now + 1 ms)` then `pad_up(None)` from the same
/// producer: the release must apply after the hit (before: "now" sorted first, so the release
/// came first and the note / repeat stuck).
#[test]
fn a_producer_s_commands_apply_in_send_order() {
    let mut r = Rig::new();
    r.set_kit(ImpulseKit::new(0, Some(r.log.clone())).with_gates(&[0]));
    r.h.set_note_repeat(true, 0.25);
    r.run(0.1);
    let t = r.now() + 0.001;
    r.midi.pad_down(0, 0.9, Some(t));
    r.midi.pad_up(0, None);
    r.run(1.0);
    let c = caps(&r);
    let kinds: Vec<CaptureKind> = c.iter().map(|c| c.kind).collect();
    assert_eq!(
        kinds,
        [CaptureKind::Hit, CaptureKind::Release],
        "no repeats"
    );
    assert!(c[1].time >= c[0].time);
    assert_eq!(r.h.shared().repeating_pads(), 0, "repeat not stuck");
    let rel: Vec<Call> = r
        .calls()
        .into_iter()
        .map(|(_, c)| c)
        .filter(|c| matches!(c, Call::Release { .. }))
        .collect();
    assert_eq!(rel.len(), 1, "the gate got its release");
}

// --- Same-time order on the real SamplerKit ----------------------------------------------

/// Pads 0 and 1 in choke group 1. Pad `audible` plays 2 s of DC 0.3, the other one silence (it
/// still starts a voice, so it still chokes).
fn choke_kit(audible: usize) -> SamplerKit {
    let mut pads = [PadParams::default(); PAD_COUNT];
    pads[0].choke_group = 1;
    pads[1].choke_group = 1;
    let mut k = SamplerKit::new(SR, pads, 0);
    for pad in 0..2 {
        let v = if pad == audible { 0.3 } else { 0.0 };
        k.set_pad_buffer(
            pad,
            Some(PadBuffer {
                forward: Arc::new(AudioBuffer::mono(SR, vec![v; 2 * SR as usize])),
                reversed: None,
            }),
        );
    }
    k
}

fn rig_with(kit: SamplerKit, pattern: &[(usize, f64)]) -> Rig {
    let mut r = Rig::new();
    r.h.set_instrument(0, Some(TrackInstrument::Drum(Box::new(kit))));
    let events = pattern
        .iter()
        .map(|&(pad, tick)| ScheduledNote {
            tick,
            beat: tick / PPQ as f64,
            note: 36.0 + pad as f64,
            velocity: 1.0,
            duration_beats: 0.25,
            pad: Some(pad),
        })
        .collect();
    r.h.set_schedule(0, Some(Arc::new(TrackSchedule::new(true, events))));
    r
}

fn peak(r: &Rig, a: usize, b: usize) -> f32 {
    r.left[a..b].iter().fold(0f32, |m, x| m.max(x.abs()))
}

/// Review finding 4: a repeat line at tick 384 and a choking pattern hit at tick 385 in the
/// same quantum. In time order the pattern hit (later) chokes the repeat voice; before the fix
/// every pattern event of the quantum fired first, so the repeat voice (fired after) survived.
#[test]
fn pattern_and_repeat_fire_in_time_order_within_a_quantum() {
    // 120 BPM at 48 kHz: one tick = 125 frames; ticks 384 / 385 = frames 48000 / 48125, both
    // in the quantum [48000, 48128)
    let mut r = rig_with(choke_kit(0), &[(1, 385.0)]);
    r.h.set_note_repeat(true, 0.25);
    r.h.start(Some(0.0), 0.0);
    r.midi.pad_down(0, 1.0, Some(0.5)); // tick 192: repeats at 240, 288, 336, 384, 432...
    r.run(1.3);
    let reps: Vec<f64> = caps(&r)
        .iter()
        .filter(|c| c.kind == CaptureKind::RepeatHit)
        .map(|c| c.time)
        .collect();
    assert_eq!(reps[..4], [0.625, 0.75, 0.875, 1.0]);
    // choked at 48125 (5 ms fade): silent until the next repeat at tick 432 (frame 54000)
    let p = peak(&r, 48_125 + 300, 54_000 - 10);
    assert!(
        p < 1e-6,
        "the repeat voice survived the later choke: peak {p}"
    );
    assert!(peak(&r, 47_000, 48_100) > 0.1 && peak(&r, 54_000, 55_000) > 0.1);
}

/// A live hit at exactly the time of a pattern note applies after it (Tone invokes transport
/// callbacks ahead of time), so the live hit chokes the pattern voice, not the reverse.
#[test]
fn a_live_hit_applies_after_the_pattern_at_the_same_time() {
    let mut r = rig_with(choke_kit(1), &[(1, 384.0)]); // pattern pad 1 at frame 48000
    r.h.set_live_sink(Some(0));
    r.h.start(Some(0.0), 0.0);
    r.midi.pad_down(0, 1.0, Some(1.0)); // live pad 0 at frame 48000 too
    r.run(1.2);
    assert!(peak(&r, 47_000, 48_000) < 1e-6);
    let p = peak(&r, 48_000 + 300, 56_000);
    assert!(
        p < 1e-6,
        "the pattern voice survived the live choke: peak {p}"
    );
    // the order also holds for the transport events fired by the inclusive advance (no double
    // fire of tick 384 after the live command)
    let hits = r
        .events
        .iter()
        .filter(|e| matches!(e, CoreEvent::PatternHit { .. }))
        .count();
    assert_eq!(hits, 1);
}

/// Review finding 7: each device callback publishes its wall-clock anchor with the clock
/// snapshot: the first frame it hands out (not the frames rendered ahead), the callback instant
/// and the output latency.
#[test]
fn process_timed_publishes_the_callback_anchor() {
    use std::time::{Duration, Instant};
    let mut r = Rig::new();
    assert_eq!(r.h.shared().snapshot().anchor, None);
    let i0 = Instant::now();
    let (mut l, mut rr) = ([0f32; 200], [0f32; 200]);
    for k in 0..3u32 {
        let clock = BlockClock {
            instant: i0 + Duration::from_micros(4167 * k as u64),
            output_latency: Duration::from_millis(3),
        };
        r.core.process_timed(&mut l, &mut rr, clock);
    }
    let s = r.h.shared().snapshot();
    let a = s.anchor.unwrap();
    assert_eq!(
        (a.frame, a.block_frames),
        (400, 200),
        "third callback starts at frame 400"
    );
    assert_eq!(s.frames, 640, "rendered ahead: whole quanta");
    assert_eq!(a.output_latency, Duration::from_millis(3));
    let at = a.instant + Duration::from_millis(1);
    assert!((s.now_extrapolated(at) - (400.0 / 48_000.0 + 0.001)).abs() < 1e-9);
    assert!((s.heard_time(at) - (400.0 / 48_000.0 + 0.001 - 0.003)).abs() < 1e-9);
}
