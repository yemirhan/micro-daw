//! Transport, pattern scheduling, loop, tempo, metronome and count-in on the real core,
//! rendered offline; onsets asserted to the frame.

use std::sync::{Arc, Mutex};

use super::test_support::*;
use super::*;

const SPB: f64 = 0.5; // 120 BPM

fn started(r: &Rig) -> Vec<(f64, f64)> {
    r.events
        .iter()
        .filter_map(|e| match e {
            CoreEvent::Started { time, beat, .. } => Some((*time, *beat)),
            _ => None,
        })
        .collect()
}

fn clicks(r: &Rig) -> Vec<(f64, f64, bool)> {
    r.events
        .iter()
        .filter_map(|e| match e {
            CoreEvent::MetronomeClick { time, beat, accent } => Some((*time, *beat, *accent)),
            _ => None,
        })
        .collect()
}

fn wraps(r: &Rig) -> Vec<f64> {
    r.events
        .iter()
        .filter_map(|e| match e {
            CoreEvent::LoopWrap { time, .. } => Some(*time),
            _ => None,
        })
        .collect()
}

#[test]
fn play_starts_at_the_next_quantum_and_steps_fire_on_exact_frames() {
    let mut r = Rig::new();
    r.pattern(&[(0, 0.0), (1, 1.0), (2, 2.5), (3, 0.3)]);
    r.run(0.01);
    let t0 = r.now();
    assert_eq!(t0 * 48000.0, 512.0);
    r.h.start(None, 0.0);
    r.run(2.0);
    assert_eq!(started(&r), [(t0, 0.0)], "no 100 ms look-ahead");
    let on: Vec<(u64, usize)> = r.onsets().iter().map(|o| (o.frame, o.pad)).collect();
    // beat b at 512 + b·24000; beat 0.3 = tick 57.6 (fractional: exact time, like Tone's
    // TransportEvent remainder)
    assert_eq!(on, [(512, 0), (512 + 7200, 3), (24512, 1), (60512, 2)]);
    assert!((r.onsets()[0].vel - 0.8).abs() < 1e-6);
    assert!((r.h.shared().playhead_beat() - (r.now() - t0) / SPB).abs() < 1e-9);
}

#[test]
fn position_comes_from_ticks_so_a_tempo_change_does_not_corrupt_it() {
    // ArrangementEngine.beat.test "position comes from transport ticks"
    let mut r = Rig::new();
    r.pattern(&[(0, 6.5)]);
    r.h.start(None, 0.0);
    r.run(3.0); // 3 s at 120 = beat 6
    let t = r.now();
    assert!((r.h.shared().playhead_beat() - 6.0).abs() < 1e-9);
    r.h.set_bpm(60.0);
    r.run(0.25);
    assert!(
        (r.h.shared().playhead_beat() - (6.0 + (r.now() - t))).abs() < 1e-9,
        "1 beat/s now"
    );
    r.run(1.0);
    // beat 6.5 at 60 BPM: 0.5 s after the change
    assert_eq!(r.onsets()[0].frame, r.frame_of(t + 0.5));
}

#[test]
fn loop_refires_every_pass_and_stays_musical_across_tempo_changes() {
    let mut r = Rig::new();
    r.pattern(&[(0, 0.0), (1, 3.5), (2, 4.0)]);
    r.h.set_loop(Some((0.0, 4.0)));
    r.h.start(None, 0.0);
    let t0 = r.now();
    r.run(2.5); // into pass 2, beat 1
    let t_change = r.now();
    let beat_at_change = (t_change - t0 - 2.0) / SPB;
    r.h.set_bpm(60.0);
    r.run(7.0);
    let on: Vec<(u64, usize)> = r.onsets().iter().map(|o| (o.frame, o.pad)).collect();
    let end2 = t_change + (4.0 - beat_at_change); // 1 s per beat
    assert_eq!(
        on,
        [
            (r.frame_of(t0), 0),
            (r.frame_of(t0 + 1.75), 1),
            (r.frame_of(t0 + 2.0), 0),
            (r.frame_of(end2 - 0.5), 1),
            (r.frame_of(end2), 0),
            (r.frame_of(end2 + 3.5), 1),
            (r.frame_of(end2 + 4.0), 0),
        ],
        "beat 4 (= loop end) never fires; the loop end stays at beat 4"
    );
    let w = wraps(&r);
    assert!((w[0] - (t0 + 2.0)).abs() < 1e-9 && (w[1] - end2).abs() < 1e-9);
}

#[test]
fn gate_release_uses_the_tempo_at_event_time() {
    // ArrangementEngine.beat.test #11 (R2-11) + playDrumNote's max(0.01, dur)
    let mut r = Rig::new();
    let log = r.log.clone();
    r.set_kit(ImpulseKit::new(0, Some(log)).with_gates(&[0, 2]));
    let ev = |pad: usize, beat: f64, dur: f64| ScheduledNote {
        id: 0,
        tick: beat * 192.0,
        beat,
        note: 36.0 + pad as f64,
        velocity: 1.0,
        duration_beats: dur,
        pad: Some(pad),
    };
    let sched = TrackSchedule::new(
        true,
        vec![ev(0, 4.0, 0.5), ev(1, 4.0, 0.5), ev(2, 5.0, 0.0)],
    );
    r.h.set_schedule(0, Some(Arc::new(sched)));
    r.h.start(None, 0.0);
    let t0 = r.now();
    r.run(1.0); // beat 2
    let t = r.now();
    r.h.set_bpm(60.0);
    r.run(4.0);
    let calls = r.calls();
    let rel: Vec<(usize, f64)> = calls
        .iter()
        .filter_map(|(_, c)| match c {
            Call::Release { pad, time } => Some((*pad, *time)),
            _ => None,
        })
        .collect();
    let at4 = t + (4.0 - (t - t0) / SPB);
    assert_eq!(
        rel.len(),
        2,
        "one-shot pads get no pattern release: {rel:?}"
    );
    assert_eq!(rel[0].0, 0);
    assert!((rel[0].1 - (at4 + 0.5)).abs() < 1e-9, "0.5 beat at 60 BPM");
    assert_eq!(rel[1].0, 2);
    assert!((rel[1].1 - (at4 + 1.0 + 0.01)).abs() < 1e-9, "min 10 ms");
}

#[test]
fn synth_notes_get_attack_and_release() {
    let mut r = Rig::new();
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    r.h.set_instrument(
        1,
        Some(TrackInstrument::Synth(Box::new(StubSynth {
            log: log.clone(),
        }))),
    );
    let n = ScheduledNote {
        id: 0,
        tick: 192.0,
        beat: 1.0,
        note: 60.0,
        velocity: 0.5,
        duration_beats: 2.0,
        pad: None,
    };
    r.h.set_schedule(1, Some(Arc::new(TrackSchedule::new(false, vec![n]))));
    r.h.start(None, 0.0);
    let t0 = r.now();
    r.run(1.0);
    r.h.stop();
    r.run(0.1);
    let calls: Vec<Call> = log.lock().unwrap().iter().map(|c| c.1).collect();
    assert_eq!(
        calls[0],
        Call::NoteOn {
            note: 60.0,
            time: t0 + 0.5
        }
    );
    assert_eq!(
        calls[1],
        Call::NoteOff {
            note: 60.0,
            time: t0 + 1.5
        }
    );
    assert!(
        matches!(calls[2], Call::ReleaseAll { .. }),
        "stop releases synths"
    );
}

#[test]
fn live_volume_and_mute() {
    let mut r = Rig::new();
    r.pattern(&[(3, 0.0), (3, 1.0), (3, 2.0)]);
    r.h.start(None, 0.0);
    let t0 = r.now();
    r.run(0.1);
    r.h.set_track_gain(0, 0.5);
    r.run(0.5);
    r.h.set_track_mute(0, true);
    r.run(0.6);
    let f = |b: f64| r.frame_of(t0 + b * SPB) as usize;
    assert_eq!(r.left[f(0.0)], 4.0);
    assert_eq!(r.left[f(1.0)], 2.0, "gain applied live");
    assert_eq!(r.left[f(2.0)], 0.0, "muted live");
    assert_eq!(
        r.triggers().len(),
        3,
        "a muted track still plays its schedule"
    );
}

#[test]
fn metronome_accents_from_transport_ticks_across_loop_wraps() {
    let mut r = Rig::new();
    r.h.set_metronome(true);
    r.h.set_loop(Some((2.0, 6.0)));
    r.run(0.01);
    r.h.start(None, 2.0);
    let t0 = r.now();
    r.run(4.2);
    let c = clicks(&r);
    let beats: Vec<f64> = c.iter().map(|x| x.1).collect();
    assert_eq!(beats, [2.0, 3.0, 4.0, 5.0, 2.0, 3.0, 4.0, 5.0, 2.0]);
    for (i, (time, beat, accent)) in c.iter().enumerate() {
        assert!((time - (t0 + i as f64 * SPB)).abs() < 1e-9);
        assert_eq!(*accent, *beat == 4.0, "accent = bar start from ticks");
    }
    // audio: each click starts on the first frame after its time, sine at C6 / G5, -15 dB
    let f0 = r.frame_of(t0) as usize;
    assert_eq!(r.left[f0 - 1], 0.0);
    assert!(r.left[f0 + 1] != 0.0);
    let crossings = |from: usize| {
        (from + 1..from + 960) // 20 ms
            .filter(|i| (r.left[*i - 1] < 0.0) != (r.left[*i] < 0.0))
            .count()
    };
    let g5 = crossings(f0);
    let c6 = crossings(r.frame_of(t0 + 2.0 * SPB) as usize);
    assert!(
        (30..=33).contains(&g5),
        "G5 784 Hz ≈ 31 crossings / 20 ms: {g5}"
    );
    assert!(
        (41..=43).contains(&c6),
        "C6 1046.5 Hz ≈ 42 crossings / 20 ms: {c6}"
    );
    let peak = r.left[f0..f0 + 2400]
        .iter()
        .fold(0f32, |m, x| m.max(x.abs()));
    assert!((peak as f64 - CLICK_GAIN).abs() < 0.01);
}

#[test]
fn metronome_obeys_its_flag_and_never_clicks_twice_at_a_wrap() {
    // R2-12: the control side turns the click on for pattern playback; the core obeys.
    let mut r = Rig::new();
    r.h.set_loop(Some((0.0, 2.0)));
    r.h.start(None, 0.0);
    r.run(0.6);
    assert!(clicks(&r).is_empty());
    r.h.set_metronome(true);
    r.run(2.0);
    let c = clicks(&r);
    assert_eq!(
        c.iter().map(|x| x.1).collect::<Vec<_>>(),
        [0.0, 1.0, 0.0, 1.0]
    );
    assert!(
        c.windows(2).all(|w| w[1].0 > w[0].0),
        "strictly increasing times"
    );
    r.h.set_metronome(false);
    r.run(1.0);
    assert_eq!(clicks(&r).len(), 4);
    r.h.stop();
    r.h.set_metronome(true);
    r.run(1.0);
    assert_eq!(clicks(&r).len(), 4, "no clicks while stopped");
}

#[test]
fn count_in_clicks_then_the_pattern_starts_on_the_one() {
    let mut r = Rig::new();
    r.pattern(&[(0, 0.0)]);
    r.h.set_loop(Some((0.0, 4.0)));
    r.run(0.1);
    let t0 = r.now() + 0.05;
    let (_, cie) = r.h.start_with_count_in(0.0, t0, 8, SPB);
    assert!((cie - (t0 + 4.0)).abs() < 1e-12);
    r.run(2.0);
    assert_eq!(r.h.shared().phase(), Phase::Pending);
    assert_eq!(
        r.h.shared().playhead_beat(),
        0.0,
        "count-in: the pattern start"
    );
    r.run(2.5);
    let ci: Vec<(f64, u32, u32)> = r
        .events
        .iter()
        .filter_map(|e| match e {
            CoreEvent::CountInClick {
                time,
                index,
                remaining,
                ..
            } => Some((*time, *index, *remaining)),
            _ => None,
        })
        .collect();
    assert_eq!(ci.len(), 8);
    for (i, (time, index, remaining)) in ci.iter().enumerate() {
        assert!((time - (t0 + i as f64 * SPB)).abs() < 1e-12);
        assert_eq!((*index, *remaining), (i as u32, 8 - i as u32));
    }
    // audible clicks: first non-zero frame right after each click time; accents on 0 and 4
    for i in 0..8 {
        let f = r.frame_of(t0 + i as f64 * SPB) as usize;
        assert_eq!(r.left[f - 1], 0.0, "click {i} starts on time");
        assert!(r.left[f + 2] != 0.0);
    }
    assert_eq!(started(&r), [(cie, 0.0)]);
    assert_eq!(
        r.onsets()[0].frame,
        r.frame_of(cie),
        "beat 0 exactly at the count-in end"
    );
}

#[test]
fn stop_fades_every_drum_track_and_silences_the_count_in() {
    let mut r = Rig::new();
    let log = r.log.clone();
    r.h.set_instrument(
        1,
        Some(TrackInstrument::Drum(Box::new(ImpulseKit::new(
            1,
            Some(log),
        )))),
    );
    let t0 = r.now() + 0.05;
    r.h.start_with_count_in(0.0, t0, 4, SPB);
    r.run(0.6); // after the second click
    let t_stop = r.now();
    r.h.stop();
    r.run(2.0);
    let stops: Vec<usize> = r
        .calls()
        .iter()
        .filter(|(_, c)| *c == Call::StopAll { time: t_stop })
        .map(|(id, _)| *id)
        .collect();
    assert_eq!(stops, [0, 1]);
    let f = r.frame_of(t_stop) as usize;
    assert!(
        r.left[f..].iter().all(|x| *x == 0.0),
        "count-in clicks silenced at the stop"
    );
    assert_eq!(r.h.shared().phase(), Phase::Stopped);
    assert!(started(&r).is_empty());
}

#[test]
fn start_while_running_withdraws_hits_handed_out_by_the_old_run() {
    let mut r = Rig::new();
    r.h.start(None, 0.0);
    r.run(0.2);
    let t = r.now();
    r.h.start(None, 8.0);
    r.run(0.01);
    assert!(
        r.calls()
            .iter()
            .any(|(_, c)| *c == Call::Cancel { time: t })
    );
    assert!((r.h.shared().playhead_beat() - (8.0 + (r.now() - t) / SPB)).abs() < 1e-9);
}
