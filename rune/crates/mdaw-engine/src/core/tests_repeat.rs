//! Note repeat in the core (NoteRepeat.test.ts behaviour cases + grid/loop/tempo/release
//! scenarios), asserted on rendered onsets and capture events.

use super::test_support::*;
use super::*;

const TPS: f64 = 384.0; // ticks per second at 120 BPM

fn repeats(r: &Rig) -> Vec<PadCapture> {
    r.captures()
        .into_iter()
        .filter(|c| c.kind == CaptureKind::RepeatHit)
        .collect()
}

/// Repeat on, transport started at `start_tick`, pad `pad` pressed on the same frame.
fn pressed_at_tick(start_tick: f64, pad: usize, vel: f64) -> (Rig, f64) {
    let mut r = Rig::new();
    r.h.set_note_repeat(true, 0.25);
    r.run(0.01);
    let t0 = r.now();
    r.h.start(None, start_tick / 192.0);
    r.h.pad_down(pad, vel, None);
    (r, t0)
}

#[test]
fn retriggers_on_the_transport_grid_with_the_initial_velocity_until_release() {
    let (mut r, t0) = pressed_at_tick(50.0, 2, 0.7);
    r.h.pad_up(2, Some(t0 + 115.0 / TPS)); // between lines 144 and 192
    r.run(1.0);
    let on: Vec<(u64, usize)> = r.onsets().iter().map(|o| (o.frame, o.pad)).collect();
    assert_eq!(
        on,
        [
            (r.frame_of(t0), 2),
            (r.frame_of(t0 + 46.0 / TPS), 2),
            (r.frame_of(t0 + 94.0 / TPS), 2)
        ]
    );
    assert!(r.onsets().iter().all(|o| (o.vel - 0.7).abs() < 1e-6));
    let rep = repeats(&r);
    assert_eq!(rep.iter().map(|c| c.beat).collect::<Vec<_>>(), [0.5, 0.75]);
    assert!(
        rep.iter()
            .all(|c| c.grid_beats == Some(0.25) && c.velocity == 0.7 && c.producer.is_none())
    );
    assert_eq!(rep[0].time, t0 + 46.0 / TPS, "exact grid time");
    assert_eq!(r.h.shared().repeating_pads() & (1 << 2), 0, "released");
}

#[test]
fn skips_grid_lines_closer_than_half_a_step() {
    let (mut r, t0) = pressed_at_tick(90.0, 1, 1.0);
    r.run(0.2);
    let first = repeats(&r)[0];
    assert_eq!(
        first.beat, 0.75,
        "tick 96 is the initial hit's step; 144 is the first repeat"
    );
    assert_eq!(first.time, t0 + 54.0 / TPS);
}

#[test]
fn the_grid_survives_loop_wraps() {
    let mut r = Rig::new();
    r.h.set_note_repeat(true, 0.25);
    r.h.set_loop(Some((0.0, 1.0)));
    r.run(0.01);
    let t0 = r.now();
    r.h.start(None, 10.0 / 192.0);
    r.h.pad_down(4, 0.9, None);
    r.run(0.9);
    let rep = repeats(&r);
    let beats: Vec<f64> = rep.iter().map(|c| c.beat).collect();
    assert_eq!(&beats[..6], [0.25, 0.5, 0.75, 0.0, 0.25, 0.5]);
    let wrap = t0 + 182.0 / TPS;
    assert_eq!(rep[3].time, wrap, "line 0 fires at the wrap");
    assert_eq!((rep[2].wrap_count, rep[3].wrap_count), (0, 1));
    for w in rep.windows(2) {
        assert!(
            (w[1].time - w[0].time - 0.125).abs() < 1e-9,
            "steady 1/16 across the wrap"
        );
    }
}

#[test]
fn repeats_follow_a_tempo_change_on_the_tick_grid() {
    let (mut r, t0) = pressed_at_tick(0.0, 0, 1.0);
    let t_change = t0 + 0.2; // tick 76.8, after the first repeat (tick 48)
    r.h.send(Some(t_change), Command::SetBpm { bpm: 60.0 });
    r.run(1.0);
    let rep = repeats(&r);
    let tc = r.frame_of(t_change) as f64 / 48000.0;
    let k_at = (tc - t0) * TPS; // ticks at the (frame-aligned) change
    assert_eq!(rep[0].time, t0 + 48.0 / TPS);
    assert_eq!(rep[1].beat, 0.5);
    assert!(
        (rep[1].time - (tc + (96.0 - k_at) / 192.0)).abs() < 1e-9,
        "60 BPM: 192 ticks/s"
    );
    assert!((rep[2].time - rep[1].time - 0.25).abs() < 1e-9);
}

#[test]
fn release_cancels_every_later_repeat_at_once() {
    // R2-4: no overshoot after the pad comes up
    let mut r = Rig::new();
    r.h.set_note_repeat(true, 0.125); // 1/32: 62.5 ms
    r.run(0.1);
    let t0 = r.now();
    r.h.pad_down(7, 0.6, None);
    let t_up = t0 + 0.3;
    r.h.pad_up(7, Some(t_up));
    r.run(1.0);
    let f_up = r.frame_of(t_up);
    assert!(r.onsets().iter().all(|o| o.frame < f_up));
    let rep = repeats(&r);
    assert_eq!(rep.len(), 4, "62.5, 125, 187.5, 250 ms");
    assert!(rep.iter().all(|c| c.time < t_up));
}

#[test]
fn free_runs_with_the_transport_stopped_aligned_on_the_first_hit() {
    let mut r = Rig::new();
    r.h.set_note_repeat(true, 0.25);
    r.run(0.1);
    let t0 = r.now();
    r.h.pad_down(3, 0.9, None);
    r.h.pad_down(5, 0.4, Some(t0 + 0.2)); // joins the grid of pad 3
    r.run(0.55);
    r.h.pad_up(3, None);
    r.h.pad_up(5, None);
    r.run(0.5);
    let rep = repeats(&r);
    let p3: Vec<f64> = rep
        .iter()
        .filter(|c| c.pad == 3)
        .map(|c| c.time - t0)
        .collect();
    let p5: Vec<f64> = rep
        .iter()
        .filter(|c| c.pad == 5)
        .map(|c| c.time - t0)
        .collect();
    for (i, t) in p3.iter().enumerate() {
        assert!((t - 0.125 * (i + 1) as f64).abs() < 1e-9);
    }
    assert_eq!(p3.len(), 4);
    assert!(
        (p5[0] - 0.375).abs() < 1e-9,
        "next line ≥ half a step after its press"
    );
    assert!(
        p5.iter()
            .all(|t| ((t / 0.125).round() * 0.125 - t).abs() < 1e-9)
    );
    assert!(
        rep.iter()
            .all(|c| c.phase == Phase::Stopped && c.beat == 0.0)
    );
    assert_eq!(r.onsets().last().unwrap().frame, r.frame_of(t0 + 0.5));
}

#[test]
fn moves_held_pads_between_the_free_clock_and_the_transport() {
    let mut r = Rig::new();
    r.run(0.1);
    r.h.pad_down(5, 0.9, None);
    r.run(0.05);
    let t_en = r.now();
    r.h.set_note_repeat(true, 0.25); // enabling while held starts the free clock
    r.run(0.2);
    assert_eq!(repeats(&r)[0].time, t_en + 0.125);
    let t_start = r.now();
    r.h.start(None, 0.0); // running: restart on the transport grid (tick 0 press → line 48)
    r.run(0.2);
    let on_tr: Vec<PadCapture> = repeats(&r)
        .into_iter()
        .filter(|c| c.time >= t_start)
        .collect();
    assert_eq!(
        (on_tr[0].phase, on_tr[0].beat, on_tr[0].time),
        (Phase::Started, 0.25, t_start + 0.125)
    );
    r.run(0.1);
    let t_stop = r.now();
    r.h.stop(); // stopped: back to a fresh free grid from the stop
    r.run(0.2);
    let after: Vec<PadCapture> = repeats(&r)
        .into_iter()
        .filter(|c| c.time >= t_stop)
        .collect();
    assert_eq!(
        (after[0].phase, after[0].time),
        (Phase::Stopped, t_stop + 0.125)
    );
    assert!(r.h.shared().repeating_pads() & (1 << 5) != 0);
}

#[test]
fn rate_change_reschedules_and_disable_stops() {
    let (mut r, t0) = pressed_at_tick(0.0, 0, 1.0);
    r.h.set_note_repeat(true, 1.0 / 3.0); // 1/8T: 64 ticks
    r.run(0.4);
    let rep = repeats(&r);
    assert_eq!(rep[0].time, t0 + 64.0 / TPS);
    assert!((rep[0].grid_beats.unwrap() - 1.0 / 3.0).abs() < 1e-12);
    assert!((rep[1].time - rep[0].time - 64.0 / TPS).abs() < 1e-9);
    let n = rep.len();
    r.h.set_note_repeat(false, 1.0 / 3.0);
    r.run(0.5);
    assert_eq!(repeats(&r).len(), n);
    assert_eq!(r.h.shared().repeating_pads(), 0);
}

#[test]
fn does_nothing_while_disabled() {
    let mut r = Rig::new();
    r.h.start(None, 0.0);
    r.h.pad_down(0, 1.0, None);
    r.run(1.0);
    assert!(repeats(&r).is_empty());
    assert_eq!(r.onsets().len(), 1);
}

#[test]
fn held_through_the_count_in_repeats_from_the_downbeat() {
    let mut r = Rig::new();
    r.h.set_note_repeat(true, 0.25);
    r.h.set_loop(Some((0.0, 4.0)));
    r.run(0.05);
    let (_, cie) = r.h.start_with_count_in(0.0, r.now() + 0.05, 4, 0.5);
    r.h.pad_down(9, 0.5, Some(cie - 0.3));
    r.run_until(cie + 0.3);
    let rep = repeats(&r);
    assert_eq!(
        (rep[0].time, rep[0].beat, rep[0].phase),
        (cie, 0.0, Phase::Started)
    );
    assert_eq!(rep[1].time, cie + 0.125);
}
