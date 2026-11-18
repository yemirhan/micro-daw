//! Live pad timing, producer merge, capture events (what LoopRecorder needs: beat from ticks,
//! loop-wrap counters, generations), live sink rules and pattern-hit suppression.

use mdaw_model::arrangement_helpers::loop_gate_beats;

use super::test_support::*;
use super::*;

fn caps(r: &Rig, kind: CaptureKind) -> Vec<PadCapture> {
    r.captures()
        .into_iter()
        .filter(|c| c.kind == kind)
        .collect()
}

fn gate(on: &PadCapture, off: &PadCapture, loop_beats: f64) -> f64 {
    loop_gate_beats(
        on.beat,
        off.beat,
        off.wrapped_beats - on.wrapped_beats,
        loop_beats,
    )
}

#[test]
fn a_live_hit_plays_at_the_first_frame_at_or_after_its_time() {
    let mut r = Rig::new();
    r.run(0.1);
    let now = r.now();
    r.h.pad_down(0, 1.0, None); // "now": the next quantum
    r.h.pad_down(1, 1.0, Some(now + 0.0101)); // mid-quantum: its exact frame
    r.h.pad_down(2, 1.0, Some(now - 1.0)); // late: as soon as possible
    r.run(0.05);
    let on: Vec<(u64, usize)> = r.onsets().iter().map(|o| (o.frame, o.pad)).collect();
    let f = r.frame_of(now);
    // pads 0 and 2 on the same frame sum (1 + 3 = 4 reads as pad 3)
    assert_eq!(on, [(f, 3), (r.frame_of(now + 0.0101), 1)]);
    assert_eq!(r.left[f as usize], 4.0);
    let t = caps(&r, CaptureKind::Hit);
    assert_eq!(t.iter().map(|c| c.pad).collect::<Vec<_>>(), [0, 2, 1]);
    assert_eq!(t[0].time, now);
    assert_eq!(t[2].time, r.frame_of(now + 0.0101) as f64 / 48000.0);
}

#[test]
fn producers_merge_by_time_then_producer_then_sequence() {
    let mut r = Rig::new();
    r.run(0.1);
    let t = r.now() + 0.02;
    r.midi.pad_down(3, 0.5, Some(t));
    r.h.pad_down(4, 0.5, Some(t));
    r.midi.pad_down(5, 0.5, Some(t - 0.001));
    r.h.pad_down(6, 0.5, Some(t));
    r.run(0.05);
    let order: Vec<(usize, Option<ProducerId>)> = caps(&r, CaptureKind::Hit)
        .iter()
        .map(|c| (c.pad, c.producer))
        .collect();
    use ProducerId::*;
    assert_eq!(
        order,
        [
            (5, Some(Midi)),
            (4, Some(Ui)),
            (6, Some(Ui)),
            (3, Some(Midi))
        ]
    );
}

#[test]
fn gate_held_while_the_tempo_halves_gets_its_tick_length() {
    // beat.test #5: on at beat 1, 120 → 60 BPM at beat 1.5, off at beat 2 (0.75 s later)
    let mut r = Rig::new();
    r.h.start(None, 0.0);
    r.h.pad_down(2, 0.8, Some(0.5));
    r.h.send(Some(0.75), Command::SetBpm { bpm: 60.0 });
    r.h.pad_up(2, Some(1.25));
    r.run(1.5);
    let (on, off) = (
        caps(&r, CaptureKind::Hit)[0],
        caps(&r, CaptureKind::Release)[0],
    );
    assert_eq!((on.beat, off.beat), (1.0, 2.0));
    assert!(
        (gate(&on, &off, 4.0) - 1.0).abs() < 1e-9,
        "seconds × final BPM would say 0.75"
    );
}

#[test]
fn gate_held_across_60_to_300_bpm_keeps_its_musical_length() {
    // beat.test round 3 E1: 0.8 s at 60 BPM + 0.2 s at 300 BPM = 1.8 beats, no invented pass
    let mut r = Rig::new();
    r.h.set_bpm(60.0);
    r.h.set_loop(Some((0.0, 4.0)));
    r.h.start(None, 0.0);
    r.h.pad_down(0, 0.8, Some(0.15));
    r.h.send(Some(0.95), Command::SetBpm { bpm: 300.0 });
    r.h.pad_up(0, Some(1.15));
    r.run(1.3);
    let (on, off) = (
        caps(&r, CaptureKind::Hit)[0],
        caps(&r, CaptureKind::Release)[0],
    );
    assert!((gate(&on, &off, 4.0) - 1.8).abs() < 1e-9);
    assert_eq!(off.wrap_count, on.wrap_count);
}

#[test]
fn loop_wraps_between_note_on_and_off_are_counted() {
    // beat.test E1 + "gate pads get their held length (wrapped, capped)"
    let mut r = Rig::new();
    r.h.set_loop(Some((0.0, 4.0)));
    r.h.start(None, 0.0);
    // wrap at 2 s (before the hit: not counted), hit at beat 3 of pass 2, wrap at 4 s,
    // release at beat 0.5 of pass 3
    r.h.pad_down(0, 0.8, Some(3.5));
    r.h.pad_up(0, Some(4.25));
    // held across two wraps: capped at the loop
    r.h.pad_down(1, 0.8, Some(5.0));
    r.h.pad_up(1, Some(9.5));
    r.run(10.0);
    let on = caps(&r, CaptureKind::Hit);
    let off = caps(&r, CaptureKind::Release);
    assert_eq!((on[0].beat, off[0].beat), (3.0, 0.5));
    assert_eq!((on[0].wrap_count, off[0].wrap_count), (1, 2));
    assert_eq!(gate(&on[0], &off[0], 4.0), 1.5);
    assert_eq!(gate(&on[1], &off[1], 4.0), 4.0);
    let wraps: Vec<f64> = r
        .events
        .iter()
        .filter_map(|e| match e {
            CoreEvent::LoopWrap { time, .. } => Some(*time),
            _ => None,
        })
        .collect();
    assert_eq!(wraps, [2.0, 4.0, 6.0, 8.0]);
}

#[test]
fn a_hit_exactly_at_the_wrap_is_on_the_loop_start_and_after_the_wrap() {
    let mut r = Rig::new();
    r.h.set_loop(Some((0.0, 4.0)));
    r.h.start(None, 0.0);
    r.h.pad_down(0, 0.8, Some(2.0));
    r.h.pad_up(0, Some(2.25));
    r.run(2.5);
    let (on, off) = (
        caps(&r, CaptureKind::Hit)[0],
        caps(&r, CaptureKind::Release)[0],
    );
    assert_eq!((on.beat, on.wrap_count), (0.0, 1));
    assert_eq!(gate(&on, &off, 4.0), 0.5);
}

#[test]
fn wrap_counters_stay_exact_over_many_passes() {
    // L-e: the TS prunes its wrap list; the core keeps two running counters (O(1)).
    let mut r = Rig::new();
    r.h.set_loop(Some((0.0, 0.25)));
    r.h.start(None, 0.0);
    r.run(62.5); // 500 passes of 0.125 s
    assert_eq!(r.h.shared().wrap_count(), 500);
    let t = r.now();
    r.h.pad_down(0, 0.8, Some(t + 0.0625)); // mid-pass
    r.h.pad_up(0, Some(t + 0.0625 + 0.375)); // 3 wraps later, same phase
    r.run(0.5);
    let (on, off) = (
        caps(&r, CaptureKind::Hit)[0],
        caps(&r, CaptureKind::Release)[0],
    );
    assert_eq!(off.wrap_count - on.wrap_count, 3);
    assert_eq!(gate(&on, &off, 0.25), 0.25, "capped at the loop");
}

#[test]
fn a_hit_during_the_count_in_reports_pending_and_the_pattern_start() {
    let mut r = Rig::new();
    r.h.set_loop(Some((4.0, 8.0)));
    r.run(0.05);
    let (g, cie) = r.h.start_with_count_in(4.0, r.now() + 0.05, 4, 0.5);
    r.h.pad_down(0, 0.8, Some(cie - 0.2));
    r.run(2.5);
    let on = caps(&r, CaptureKind::Hit)[0];
    assert_eq!((on.phase, on.beat, on.generation), (Phase::Pending, 4.0, g));
    // LoopRecorder: ahead = countInLeft / spb from the audio time
    assert!(((cie - on.time) / 0.5 - 0.4).abs() < 1e-3);
}

#[test]
fn captures_carry_the_generation_of_their_run() {
    let mut r = Rig::new();
    let g1 = r.h.start(None, 0.0);
    r.h.pad_down(0, 0.8, None);
    r.run(0.1);
    let g2 = r.h.stop();
    let g3 = r.h.start(None, 0.0);
    r.h.pad_down(0, 0.8, None);
    r.run(0.1);
    let gens: Vec<u64> = caps(&r, CaptureKind::Hit)
        .iter()
        .map(|c| c.generation)
        .collect();
    assert_eq!(gens, [g1, g3]);
    assert!(g1 < g2 && g2 < g3);
    assert!(
        r.events
            .iter()
            .any(|e| matches!(e, CoreEvent::Stopped { generation, .. } if *generation == g2))
    );
    // a stale transport command (older generation) is ignored
    r.h.send(None, Command::Stop { generation: g1 });
    r.run(0.05);
    assert_eq!(r.h.shared().phase(), Phase::Started);
}

#[test]
fn switching_the_live_sink_releases_the_pads_held_on_the_old_one() {
    // beat.test #3 (R2-3)
    let mut r = Rig::new();
    let log = r.log.clone();
    r.h.set_instrument(
        1,
        Some(TrackInstrument::Drum(Box::new(ImpulseKit::new(
            1,
            Some(log),
        )))),
    );
    r.h.pad_down(2, 0.8, None);
    r.run(0.05);
    let t = r.now();
    r.h.set_live_sink(Some(1));
    r.h.pad_down(3, 0.8, None);
    r.run(0.05);
    r.h.pad_up(3, None);
    r.h.pad_up(2, None);
    r.run(0.05);
    let rel: Vec<(usize, Call)> = r
        .calls()
        .into_iter()
        .filter(|(_, c)| matches!(c, Call::Release { .. }))
        .collect();
    assert_eq!(rel[0], (0, Call::Release { pad: 2, time: t }));
    assert_eq!(rel.len(), 2);
    assert!(matches!(rel[1], (1, Call::Release { pad: 3, .. })));
    assert_eq!(r.triggers(), [(2, 0.0), (3, t)]);
}

#[test]
fn a_suppressed_pattern_hit_is_skipped_once() {
    // LoopRecorder: a hit snapped forward into this pass is not replayed by the pattern
    let mut r = Rig::new();
    r.pattern(&[(0, 1.0)]);
    r.h.set_loop(Some((0.0, 2.0)));
    r.h.start(None, 0.0);
    r.h.suppress_pattern_hit(36.0, 1.0, 0.5 + 0.15);
    r.run(2.2);
    let t: Vec<f64> = r.triggers().iter().map(|x| x.1).collect();
    assert_eq!(t, [1.5], "skipped at 0.5 s, played on the next pass");
    // expired suppression: consumed, but the hit plays
    r.h.suppress_pattern_hit(36.0, 1.0, 1.0);
    r.run(1.0);
    assert_eq!(r.triggers().len(), 2);
    let flashes = r
        .events
        .iter()
        .filter(|e| matches!(e, CoreEvent::PatternHit { pad: 0, .. }))
        .count();
    assert_eq!(flashes, 2);
}
