//! Offline-render ports of the real-Tone probes (`scripts/probes/*.mjs`, `npm run test:audio`)
//! and of the transport cases of ArrangementEngine.beat.test.ts / round5.test.ts. Each one
//! replays the probe's sequence on the real core and checks the probe's expectation (or a
//! stricter, frame-exact one).

use mdaw_model::QuantizeGrid;
use mdaw_model::SwingGrid;
use mdaw_model::arrangement_helpers::{loop_gate_beats, quantize_beat};

use super::test_support::*;
use super::*;

const SPB: f64 = 0.5;

fn q16(beat: f64) -> f64 {
    quantize_beat(beat, QuantizeGrid::Q16, 0.0, SwingGrid::S16)
}

fn hits(r: &Rig, kind: CaptureKind) -> Vec<PadCapture> {
    r.captures()
        .into_iter()
        .filter(|c| c.kind == kind)
        .collect()
}

fn poll(r: &mut Rig, every: f64, n: usize) -> Vec<f64> {
    (0..n)
        .map(|_| {
            r.run(every);
            r.h.shared().playhead_beat()
        })
        .collect()
}

/// A drum pattern on the 16th grid (`steps` 0..15 of a 1-bar loop) on pad 0.
fn steps(r: &mut Rig, steps: &[u32]) {
    let notes: Vec<(usize, f64)> = steps.iter().map(|s| (0, *s as f64 * 0.25)).collect();
    r.pattern(&notes);
}

// --- b1-position-glitch.mjs ------------------------------------------------------------------

#[test]
fn b1_no_stale_playhead_after_play_stop_idle_play() {
    for pattern in [false, true] {
        for lookahead in [None, Some(0.1)] {
            let mut r = Rig::new();
            steps(&mut r, &[0, 4]);
            if pattern {
                r.h.set_loop(Some((0.0, 4.0)));
            }
            r.h.start(None, 0.0);
            r.run(1.0);
            r.h.stop();
            r.run(2.0);
            let at = lookahead.map(|d| r.now() + d);
            r.h.start(at, 0.0);
            let seen = poll(&mut r, 0.016, 16);
            assert!(
                seen[..6].iter().all(|b| *b < 0.25),
                "pattern {pattern} {lookahead:?}: {seen:?}"
            );
            assert!(seen.windows(2).all(|w| w[1] >= w[0]));
        }
    }
}

// --- b1-song-autostop.mjs --------------------------------------------------------------------

#[test]
fn b1_song_play_after_a_long_idle_starts_from_zero_and_keeps_playing() {
    let mut r = Rig::new();
    r.h.set_bpm(300.0);
    steps(&mut r, &[0, 4]);
    r.h.start(None, 0.0);
    r.run(0.6);
    r.h.stop();
    r.run(13.0);
    let before = r.triggers().len();
    r.h.start(None, 0.0);
    let seen = poll(&mut r, 0.05, 8);
    r.run(0.6);
    assert!(seen.iter().all(|b| *b < 4.0), "stale start {seen:?}");
    assert_eq!(r.h.shared().phase(), Phase::Started);
    // The Song auto-stop (control side) fires at beat ≥ lengthBeats (64): never reached here.
    assert!(r.h.shared().playhead_beat() < 64.0);
    assert_eq!(r.triggers().len() - before, 2, "hits heard");
}

// --- b1-record-restart.mjs -------------------------------------------------------------------

/// A 1-bar loop take with count-in off: stop at beat 2.5, `gap`, record again, hit 30 ms later.
fn restart_take(gap: f64, start_delay: Option<f64>) -> (Rig, u64, PadCapture, PadCapture) {
    let mut r = Rig::new();
    r.set_kit(ImpulseKit::new(0, Some(r.log.clone())).with_gates(&[6]));
    r.h.set_loop(Some((0.0, 4.0)));
    r.h.start(None, 0.0);
    r.run(2.5 * SPB);
    r.h.stop();
    r.run(gap);
    let at = start_delay.map(|d| r.now() + d);
    let g = r.h.start(at, 0.0);
    r.run(0.03);
    r.midi.pad_down(6, 0.9, None);
    r.run(0.5);
    r.midi.pad_up(6, None);
    r.run(0.2);
    let on = hits(&r, CaptureKind::Hit)[0];
    let off = hits(&r, CaptureKind::Release)[0];
    (r, g, on, off)
}

#[test]
fn b1_hits_right_after_restarting_record_land_on_beat_zero() {
    for gap in [0.02, 0.06, 0.15, 0.4] {
        let (_, g, on, _) = restart_take(gap, None);
        assert_eq!(on.generation, g);
        assert_eq!(on.phase, Phase::Started);
        assert!(on.beat > 0.0 && on.beat < 0.07, "gap {gap}: {}", on.beat);
        assert_eq!(q16(on.beat), 0.0);
        // With a Tone-like 100 ms start delay the hit is before the start: the start beat.
        let (_, _, on, _) = restart_take(gap, Some(0.1));
        assert_eq!((on.phase, on.beat), (Phase::Pending, 0.0));
    }
}

#[test]
fn b1_playing_then_record_hit_lands_at_the_pattern_start() {
    let mut r = Rig::new();
    steps(&mut r, &[3]);
    r.h.set_loop(Some((0.0, 4.0)));
    r.h.start(None, 0.0);
    r.run(2.5 * SPB);
    // startLoopRecording stops the playback first, then starts the take
    r.h.stop();
    let g = r.h.start(None, 0.0);
    r.run(0.03);
    r.h.pad_down(0, 0.9, None);
    r.run(0.04);
    r.h.pad_up(0, None);
    r.run(0.2);
    let on = hits(&r, CaptureKind::Hit)[0];
    assert_eq!((on.generation, q16(on.beat)), (g, 0.0));
    assert!(r.h.shared().playhead_beat() < 1.0);
}

#[test]
fn b1_gate_held_while_a_take_is_restarted_quickly() {
    let (_, _, on, off) = restart_take(0.02, None);
    assert_eq!(q16(on.beat), 0.0);
    let gate = loop_gate_beats(on.beat, off.beat, off.wrapped_beats - on.wrapped_beats, 4.0);
    assert!((gate - 1.0).abs() < 0.01, "≈ 1 beat: {gate}");
}

// --- n1-countin-abort.mjs --------------------------------------------------------------------

fn countin_rig() -> Rig {
    let mut r = Rig::new();
    steps(&mut r, &[0, 4, 8, 12]);
    r.h.set_loop(Some((0.0, 4.0)));
    r.run(0.05);
    r
}

fn count_in(r: &mut Rig) -> (u64, f64) {
    let t0 = r.now() + 0.05;
    r.h.start_with_count_in(0.0, t0, 4, SPB)
}

#[test]
fn n1_stop_during_the_count_in_cancels_the_pending_start() {
    let mut r = countin_rig();
    let (_, cie) = count_in(&mut r);
    r.run(0.5);
    r.h.stop();
    r.run_until(cie + 0.6);
    assert_eq!(r.h.shared().phase(), Phase::Stopped);
    assert_eq!(r.h.shared().playhead_beat(), 0.0, "transport never ran");
    assert!(r.triggers().is_empty(), "no pattern hits while stopped");
    assert!(
        !r.events
            .iter()
            .any(|e| matches!(e, CoreEvent::Started { .. }))
    );
}

#[test]
fn n1_record_again_starts_the_pattern_at_the_new_count_in_end() {
    let mut r = countin_rig();
    count_in(&mut r);
    r.run(0.6);
    r.h.stop();
    r.run(0.15);
    let (g2, cie2) = count_in(&mut r);
    r.run_until(cie2 + 0.02);
    r.midi.pad_down(1, 0.9, None);
    r.run(0.04);
    r.midi.pad_up(1, None);
    r.run(0.3);
    let first = r.triggers()[0];
    assert_eq!(
        first,
        (0, cie2),
        "pattern beat 0 exactly at the new count-in end"
    );
    assert_eq!(r.onsets()[0].frame, r.frame_of(cie2));
    let on = hits(&r, CaptureKind::Hit)[0];
    assert_eq!((on.generation, q16(on.beat)), (g2, 0.0));
}

#[test]
fn n1_play_after_an_aborted_count_in_never_jumps_back() {
    let mut r = countin_rig();
    let (_, cie1) = count_in(&mut r);
    r.run(0.4);
    r.h.stop();
    r.run(0.1);
    r.h.start(None, 0.0);
    let mut pos = vec![];
    while r.now() < cie1 + 0.4 {
        r.run(0.03);
        pos.push(r.h.shared().playhead_beat());
    }
    assert!(pos.windows(2).all(|w| w[1] >= w[0]), "{pos:?}");
    assert_eq!(r.h.shared().phase(), Phase::Started);
}

#[test]
fn r1_abort_then_immediate_restart_plays_no_stray_hit() {
    let mut r = countin_rig();
    r.h.start(None, 0.0);
    r.run(0.7);
    r.h.stop();
    r.run(0.3);
    let mut strays = vec![];
    for gap in [0.0, 0.005, 0.015, 0.025, 0.035, 0.045, 0.0, 0.01] {
        count_in(&mut r);
        r.run(0.25);
        r.h.stop();
        if gap > 0.0 {
            r.run(gap);
        }
        let mark = r.triggers().len();
        let (_, cie) = count_in(&mut r);
        r.run_until(cie - 0.02);
        strays.extend(
            r.triggers()[mark..]
                .iter()
                .filter(|(_, t)| *t < cie - 0.005)
                .copied(),
        );
        r.h.stop();
        r.run(0.12);
    }
    assert!(strays.is_empty(), "{strays:?}");
}

// --- n4-seek-pending.mjs ---------------------------------------------------------------------

#[test]
fn n4_seek_before_the_start_takes_effect_starts_from_the_new_beat() {
    for delay in [0.0, 0.03, 0.07] {
        let mut r = Rig::new();
        r.pattern(&[(0, 0.0), (1, 3.0)]);
        r.run(0.05);
        let at = r.now() + 0.1; // a Tone-like look-ahead start
        r.h.start(Some(at), 0.0);
        if delay > 0.0 {
            r.run(delay);
        }
        r.h.seek(3.0);
        let pos = poll(&mut r, 0.025, 16);
        assert!(pos.iter().all(|b| *b >= 3.0), "delay {delay}: {pos:?}");
        assert_eq!(r.h.shared().phase(), Phase::Started);
        assert_eq!(
            r.triggers(),
            [(1, at)],
            "beat 3 at the start; no beat-0 step"
        );
        assert!(r.onsets().iter().all(|o| o.pad == 1), "no stale voice");
    }
    // Same frame as an immediate start: also from the new beat.
    let mut r = Rig::new();
    r.pattern(&[(0, 0.0), (1, 3.0)]);
    r.h.start(None, 0.0);
    r.h.seek(3.0);
    r.run(0.2);
    assert_eq!(r.triggers(), [(1, 0.0)]);
}

#[test]
fn n4_seek_while_pending_keeps_the_pattern_loop() {
    let mut r = Rig::new();
    r.pattern(&[(0, 0.0), (1, 3.0)]);
    r.h.set_loop(Some((0.0, 4.0)));
    r.run(0.05);
    let at = r.now() + 0.1;
    r.h.start(Some(at), 0.0);
    r.h.seek(3.0);
    r.run(1.0);
    let tr = r.triggers();
    assert_eq!(tr.iter().map(|x| x.0).collect::<Vec<_>>(), [1, 0]);
    assert_eq!(tr[0].1, at);
    assert!(
        (tr[1].1 - (at + SPB)).abs() < 1e-12,
        "wraps at beat 4 to the loop start"
    );
}

#[test]
fn n4_a_seek_after_the_start_just_moves_the_transport() {
    let mut r = Rig::new();
    r.pattern(&[(0, 0.0), (1, 8.0)]);
    r.h.start(None, 0.0);
    r.run(0.2);
    let t = r.now();
    r.h.seek(8.0);
    r.run(0.2);
    assert_eq!(r.triggers(), [(0, 0.0), (1, t)]);
    assert!((r.h.shared().playhead_beat() - (8.0 + (r.now() - t) / SPB)).abs() < 1e-9);
}

// --- ArrangementEngine.beat.test "review round 4 regressions" -------------------------------

#[test]
fn b1_stopping_a_stopped_transport_changes_nothing() {
    let mut r = Rig::new();
    r.h.start(None, 0.0);
    r.run(1.0);
    r.h.stop();
    r.run(5.0);
    r.h.stop(); // Stop pressed twice: never extrapolates the old run
    r.run(1.0);
    assert_eq!(r.h.shared().playhead_beat(), 0.0);
    let t = r.now();
    r.h.start(None, 0.0);
    r.run(0.01);
    let last_start = r.events.iter().rev().find_map(|e| match e {
        CoreEvent::Started { time, beat, .. } => Some((*time, *beat)),
        _ => None,
    });
    assert_eq!(last_start, Some((t, 0.0)));
}

#[test]
fn b1_play_from_reports_the_cue_beat_before_the_start() {
    let mut r = Rig::new();
    r.h.start(None, 0.0);
    r.run(0.5);
    r.h.stop();
    r.run(0.5);
    let at = r.now() + 0.1;
    r.h.start(Some(at), 6.0);
    r.run(0.05);
    assert_eq!(r.h.shared().playhead_beat(), 6.0, "not 0, not the old run");
    r.run_until(at + 0.25);
    assert!((r.h.shared().playhead_beat() - (6.0 + (r.now() - at) / SPB)).abs() < 1e-9);
}

#[test]
fn b1_song_record_hit_before_the_scheduled_start_is_at_the_record_start() {
    let mut r = Rig::new();
    r.h.start(None, 0.0);
    r.run(0.5);
    r.h.stop();
    r.run(0.1);
    let at = r.now() + 0.1;
    r.h.start(Some(at), 0.0);
    r.midi.pad_down(1, 0.8, Some(at - 0.03));
    r.run(0.2);
    let on = hits(&r, CaptureKind::Hit)[0];
    assert_eq!((on.phase, on.beat), (Phase::Pending, 0.0));
    assert!((on.time - (at - 0.03)).abs() < 1.0 / 48000.0);
}
