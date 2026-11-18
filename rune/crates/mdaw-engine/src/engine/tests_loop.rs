//! Loop recording on the real engine + core (ArrangementEngine.beat.test.ts "loop recording",
//! the review-round regressions #2 #5 #13 E1 U6 B1 and the count-in), with live pad hits through
//! the MIDI input at exact times.

use serde_json::{Value, json};

use super::TransportState;
use super::test_rig::*;
use mdaw_model::doc::LoopSettingsPatch;
use mdaw_model::{InstrumentType, PadIndex};

fn patch(v: Value) -> LoopSettingsPatch {
    serde_json::from_value(v).unwrap()
}

fn settings(r: &mut Rig, v: Value) {
    let p = patch(v);
    r.e.edit(|d| d.set_loop_settings(&p));
}

fn base() -> Value {
    json!({"bars": 1, "quantize": "1/16", "countInBars": 0, "swing": 0, "metronome": false})
}

fn merged(extra: Value) -> Value {
    let mut b = base();
    for (k, v) in extra.as_object().unwrap() {
        b[k] = v.clone();
    }
    b
}

/// A take on the active drum track (test kit), started and running.
fn recording(extra: Value) -> (Rig, String) {
    let mut r = Rig::new();
    let id = r.drum_track();
    settings(&mut r, merged(extra));
    r.e.start_loop_recording();
    r.run(0.01);
    (r, id)
}

fn gate(r: &mut Rig, id: &str, pad: PadIndex) {
    let p = json!({"mode": "gate"}).as_object().unwrap().clone();
    r.e.edit(|d| d.update_pad(id, pad, &p));
}

fn step(r: &mut Rig, id: &str, pad: PadIndex, step: i64) {
    r.e.edit(|d| d.toggle_step(id, pad, step, 0.25, None));
}

fn dur_of(r: &Rig, id: &str, pad: PadIndex, start: f64) -> f64 {
    r.e.doc()
        .get_pattern_region(id)
        .unwrap()
        .notes
        .iter()
        .find(|n| n.note == 36.0 + pad as f64 && (n.start_beat - start).abs() < 1e-6)
        .unwrap()
        .duration_beats
}

#[test]
fn creates_the_drum_track_and_pattern_region_and_loops_the_pattern_range() {
    let mut r = Rig::new();
    settings(
        &mut r,
        json!({"bars": 2, "countInBars": 0, "metronome": false}),
    );
    r.e.start_loop_recording();
    assert!(r.e.is_loop_recording());
    assert_eq!(r.e.get_state(), TransportState::Recording);
    let id = r.e.doc().get_active_drum_track_id().unwrap();
    let region = r.e.doc().get_pattern_region(&id).unwrap();
    assert_eq!((region.start_beat, region.length_beats), (0.0, 8.0));
    assert!(
        !r.e.doc().is_loop_enabled(),
        "the Song loop toggle is untouched"
    );
    let range = r.e.get_active_loop_range().unwrap();
    assert_eq!((range.start_beat, range.end_beat), (0.0, 8.0));
    r.run(4.5); // 9 beats at 120 BPM: wrapped once
    assert!(
        (r.e.get_position() - 1.0).abs() < 0.02,
        "{}",
        r.e.get_position()
    );
    r.e.stop();
    assert!(!r.e.is_loop_recording());
    assert_eq!(r.e.get_state(), TransportState::Stopped);
    assert_eq!(r.e.get_position(), 0.0);
}

#[test]
fn quantizes_hits_and_wraps_them_into_the_loop() {
    let (mut r, id) = recording(json!({}));
    r.hit_at_beat(0.52, 1, 0.9);
    r.hit_at_beat(3.97, 0, 0.7);
    r.hit_at_beat(6.1, 2, 0.5); // second pass, position 2.1
    assert_eq!(
        r.pattern(&id),
        [(1, 0.5, 0.9), (0, 0.0, 0.7), (2, 2.0, 0.5)]
    );
}

#[test]
fn quantizes_against_the_swung_grid_but_stores_straight_and_triplet_grids() {
    let (mut r, id) = recording(json!({"swing": 0.6}));
    r.hit_at_beat(0.4, 2, 0.8); // the swung position of step 1 (0.25 + 0.15)
    assert_eq!(r.pattern(&id)[0].1, 0.25);
    settings(&mut r, json!({"quantize": "1/8T"}));
    r.hit_at_beat(0.7, 3, 0.8);
    assert!((r.pattern(&id)[1].1 - 2.0 / 3.0).abs() < 1e-6);
}

#[test]
fn note_repeat_hits_on_a_finer_grid_than_the_quantize_keep_their_own_grid() {
    let (mut r, id) = recording(json!({"quantize": "1/16"}));
    r.e.handle().set_note_repeat(true, 0.125); // a 1/32 roll
    let t = r.time_of_beat(0.87);
    r.pad_down(0, 0.8, Some(t)); // finger hit → 0.75; repeats from the next grid line ≥ +1/64
    r.pad_up(0, Some(r.time_of_beat(1.45)));
    r.run_until(r.time_of_beat(1.6));
    let notes: Vec<(f64, f64)> =
        r.e.doc()
            .get_pattern_region(&id)
            .unwrap()
            .notes
            .iter()
            .filter(|n| n.note == 36.0)
            .map(|n| (n.start_beat, n.duration_beats))
            .collect();
    assert_eq!(
        notes,
        [
            (0.75, 0.25),
            (1.0, 0.125),
            (1.125, 0.125),
            (1.25, 0.125),
            (1.375, 0.125)
        ]
    );
    // a coarser hint than the quantize changes nothing: the quantize wins
    r.e.handle().set_note_repeat(true, 0.5);
    let t = r.time_of_beat(2.2);
    r.pad_down(1, 0.8, Some(t));
    r.pad_up(1, Some(t + 0.05));
    r.run_until(t + 0.1);
    assert_eq!(r.pattern(&id).iter().find(|n| n.0 == 1).unwrap().1, 2.25);
}

#[test]
fn dedupes_hits_on_the_same_step_and_pad_louder_wins() {
    let (mut r, id) = recording(json!({}));
    r.hit_at_beat(1.02, 0, 0.4);
    r.hit_at_beat(4.98, 0, 0.9); // next pass, same step
    r.hit_at_beat(9.0, 0, 0.6);
    assert_eq!(r.pattern(&id), [(0, 1.0, 0.9)]);
}

#[test]
fn overdub_keeps_the_existing_pattern() {
    let mut r = Rig::new();
    let id = r.drum_track();
    settings(&mut r, merged(json!({"overdub": true})));
    step(&mut r, &id, 5, 0);
    r.e.start_loop_recording();
    r.run(0.01);
    r.hit_at_beat(2.0, 1, 0.8);
    let mut pads: Vec<_> = r.pattern(&id).iter().map(|n| n.0).collect();
    pads.sort();
    assert_eq!(pads, [1, 5]);
}

#[test]
fn replace_clears_the_pattern_on_the_first_hit_only() {
    let mut r = Rig::new();
    let id = r.drum_track();
    settings(&mut r, merged(json!({"overdub": false})));
    step(&mut r, &id, 5, 0);
    step(&mut r, &id, 6, 4);
    r.e.start_loop_recording();
    r.run(0.01);
    assert_eq!(
        r.pattern(&id).len(),
        2,
        "nothing cleared until the first hit"
    );
    r.hit_at_beat(2.0, 1, 0.8);
    r.hit_at_beat(5.0, 2, 0.8); // second pass: kept
    assert_eq!(r.pattern(&id), [(1, 2.0, 0.8), (2, 1.0, 0.8)]);
}

#[test]
fn the_whole_take_is_one_undo_step() {
    let mut r = Rig::new();
    let id = r.drum_track();
    settings(&mut r, base());
    step(&mut r, &id, 5, 0);
    r.e.start_loop_recording();
    r.run(0.01);
    r.hit_at_beat(1.0, 1, 0.8);
    r.hit_at_beat(2.0, 2, 0.8);
    r.e.stop_loop_recording();
    assert_eq!(r.e.doc().undo_manager().undo_label(), Some("Loop Record"));
    r.e.edit(|d| d.perform_undo());
    assert_eq!(r.pattern(&id), [(5, 0.0, 0.8)]);
}

#[test]
fn ignores_synth_notes_and_note_offs() {
    let (mut r, id) = recording(json!({}));
    r.run(0.5);
    r.e.capture_note_on(60.0, 0.8, None);
    r.e.capture_note_off(60.0, None);
    r.pad_up(0, None);
    r.run(0.1);
    assert!(r.pattern(&id).is_empty());
}

#[test]
fn merges_hits_live_and_does_not_replay_a_hit_snapped_forward() {
    let (mut r, id) = recording(json!({}));
    let t0 = r.time_of_beat(0.0);
    r.hit_at_beat(0.95, 4, 0.8); // snapped forward to beat 1
    assert_eq!(r.pattern(&id), [(4, 1.0, 0.8)]);
    r.run_until(t0 + 4.2 * SPB * 2.0);
    let hits: Vec<f64> = r
        .pattern_hits()
        .iter()
        .filter(|h| h.1 == 4)
        .map(|h| (h.2 - t0) / SPB)
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "not replayed in the pass it was hit in: {hits:?}"
    );
    assert!(
        (hits[0] - 5.0).abs() < 1e-3,
        "plays on the next pass: {hits:?}"
    );
}

#[test]
fn count_in_remaining_beats_early_hits_ignored_pre_roll_hits_on_beat_1() {
    let mut r = Rig::new();
    let id = r.drum_track();
    settings(&mut r, merged(json!({"countInBars": 1, "metronome": true})));
    let start = r.now();
    r.e.start_loop_recording();
    assert_eq!(r.e.get_count_in_remaining(), 4);
    let end = r.e.get_count_in_end().unwrap();
    assert!((end - (start + 0.05 + 4.0 * SPB)).abs() < 1e-9);
    assert_eq!(r.e.get_position(), 0.0, "count-in: the pattern start");
    r.pad_down(1, 0.8, None); // ~2 s before the downbeat: ignored
    r.run(0.01);
    assert!(r.pattern(&id).is_empty());
    let v0 = r.e.doc().state_version();
    r.run_until(start + 1.1);
    assert_eq!(r.e.get_count_in_remaining(), 2);
    assert!(
        r.e.doc().state_version() > v0,
        "listeners hear each count-in beat"
    );
    // 0.15 s (0.3 beat) before the downbeat: beat 1
    r.pad_down(2, 0.8, Some(end - 0.15));
    r.run_until(end - 0.1);
    assert_eq!(r.pattern(&id), [(2, 0.0, 0.8)]);
    r.run_until(end + 2.1);
    assert_eq!(r.e.get_count_in_remaining(), 0);
    let count_in = r
        .events
        .iter()
        .filter(|e| matches!(e, super::EngineEvent::CountInClick { .. }))
        .count();
    assert_eq!(count_in, 4);
    // the metronome keeps clicking during the loop (loop setting)
    assert!(r.clicks().iter().filter(|c| c.0 >= end - 1e-9).count() >= 4);
    // the pre-roll hit is suppressed on the first downbeat (heard live), plays on the next pass
    let first: Vec<_> = r
        .pattern_hits()
        .iter()
        .filter(|h| h.1 == 2)
        .map(|h| h.2)
        .collect();
    assert_eq!(first.len(), 1);
    assert!((first[0] - (end + 4.0 * SPB)).abs() < 1e-6);
    r.e.stop_loop_recording();
}

#[test]
fn metronome_during_loop_play_only_when_the_setting_is_on() {
    let (mut r, _) = recording(json!({"metronome": false}));
    r.run(1.0);
    assert!(r.clicks().is_empty());
    settings(&mut r, json!({"metronome": true}));
    r.run(1.0);
    assert!(!r.clicks().is_empty());
    r.e.stop_loop_recording();
    let n = r.clicks().len();
    r.run(1.0);
    assert_eq!(r.clicks().len(), n, "stopped with the take");
}

#[test]
fn a_failing_start_rolls_back_to_stopped_and_notifies() {
    let mut r = Rig::new();
    r.drum_track();
    settings(&mut r, json!({"bars": 1, "countInBars": 1}));
    let markers = r.e.doc().get_loop_markers();
    let v0 = r.e.doc().state_version();
    r.e.inject_start_failure();
    r.e.start_loop_recording();
    assert_eq!(r.e.get_state(), TransportState::Stopped);
    assert!(!r.e.is_loop_recording());
    assert!(!r.e.doc().is_loop_enabled());
    assert_eq!(r.e.doc().get_loop_markers(), markers);
    assert!(r.e.doc().state_version() > v0);
    r.run(3.0);
    assert!(r.pattern_hits().is_empty() && r.clicks().is_empty());
    assert_eq!(r.e.get_position(), 0.0);
    // and it can start again
    r.e.start_loop_recording();
    assert_eq!(r.e.get_state(), TransportState::Recording);
    r.e.stop();
}

#[test]
fn gate_pads_get_their_held_length_wrapped_and_capped_one_shots_a_fixed_one() {
    let mut r = Rig::new();
    let id = r.drum_track();
    gate(&mut r, &id, 2);
    settings(&mut r, base());
    r.e.start_loop_recording();
    r.run(0.01);
    // gate pad: hit at 3.5, released 1 beat later (across the loop wrap at 4)
    let t = r.time_of_beat(3.5);
    r.pad_down(2, 0.8, Some(t));
    r.pad_up(2, Some(t + SPB));
    r.run_until(t + SPB + 0.01);
    assert!((dur_of(&r, &id, 2, 3.5) - 1.0).abs() < 1e-6);
    // one-shot: the grid length however long it is held
    let t = r.time_of_beat(5.0);
    r.pad_down(0, 0.8, Some(t));
    r.pad_up(0, Some(t + 3.0 * SPB));
    r.run_until(t + 3.0 * SPB + 0.01);
    assert_eq!(dur_of(&r, &id, 0, 1.0), 0.25);
    // held longer than the loop (two wraps): capped at the loop length
    let t = r.time_of_beat(r.e.get_position().floor() + 6.0);
    r.pad_down(2, 0.8, Some(t));
    r.pad_up(2, Some(t + 9.0 * SPB));
    r.run_until(t + 9.0 * SPB + 0.01);
    assert_eq!(dur_of(&r, &id, 2, 2.0), 4.0);
    r.e.stop_loop_recording();
}

#[test]
fn a_gate_held_while_the_tempo_halves_gets_its_musical_length() {
    // review round 2 #5
    let mut r = Rig::new();
    let id = r.drum_track();
    gate(&mut r, &id, 2);
    settings(&mut r, base());
    r.e.start_loop_recording();
    r.run(0.01);
    let t = r.time_of_beat(1.0);
    r.pad_down(2, 0.8, Some(t));
    r.run_until(t + 0.25); // half a beat at 120
    r.e.edit(|d| d.set_bpm(60.0));
    let t2 = r.now();
    r.pad_up(2, Some(t2 + 0.5 - (t2 - t - 0.25) * 2.0)); // the next half beat takes 0.5 s
    r.run_until(t2 + 0.6);
    assert!(
        (dur_of(&r, &id, 2, 1.0) - 1.0).abs() < 2e-3,
        "{}",
        dur_of(&r, &id, 2, 1.0)
    );
    r.e.stop_loop_recording();
}

#[test]
fn e1_a_gate_held_across_60_to_300_bpm_keeps_its_musical_length() {
    let mut r = Rig::new();
    let id = r.drum_track();
    gate(&mut r, &id, 0);
    r.e.edit(|d| d.set_bpm(60.0));
    settings(&mut r, merged(json!({"quantize": "off"})));
    r.e.start_loop_recording();
    r.run(0.15);
    let t = r.now();
    let on = r.e.get_position();
    r.pad_down(0, 0.8, Some(t));
    r.run_until(t + 0.8);
    r.e.edit(|d| d.set_bpm(300.0));
    let t2 = r.now();
    r.pad_up(0, Some(t2 + 0.2));
    r.run_until(t2 + 0.3);
    let start = r.e.doc().get_pattern_region(&id).unwrap().notes[0].start_beat;
    assert!((start - on).abs() < 1e-3);
    // 0.8 beat at 60 BPM (+ the frames until the tempo change) + 1 beat at 300 BPM
    let want = (t2 - t) + 1.0;
    assert!((dur_of(&r, &id, 0, start) - want).abs() < 1e-3);
    r.e.stop_loop_recording();
}

#[test]
fn r2_2_undo_during_the_count_in_of_an_empty_take() {
    let mut r = Rig::new();
    let id = r.drum_track();
    step(&mut r, &id, 0, 0);
    settings(
        &mut r,
        json!({"bars": 4, "countInBars": 1, "metronome": false}),
    ); // the take grows the region
    r.e.start_loop_recording();
    r.run(0.3);
    assert!(r.e.get_count_in_remaining() > 0);
    r.e.edit(|d| d.perform_undo());
    assert!(!r.e.is_loop_recording());
    assert_eq!(r.e.get_state(), TransportState::Stopped);
    assert!(r.pattern(&id).is_empty());
    let um = r.e.doc().undo_manager();
    assert!(um.can_redo());
    assert_ne!(um.undo_label(), Some("Loop Record"));
    r.run(3.0);
    assert!(
        r.pattern_hits().is_empty(),
        "the pending start was cancelled"
    );
    r.e.edit(|d| d.perform_redo());
    assert_eq!(r.pattern(&id), [(0, 0.0, 0.8)]);
}

#[test]
fn r2_2_undo_mid_take_with_hits_undoes_the_take_and_redo_brings_it_back() {
    let mut r = Rig::new();
    let id = r.drum_track();
    settings(&mut r, base());
    step(&mut r, &id, 5, 0);
    r.e.start_loop_recording();
    r.run(0.01);
    r.hit_at_beat(1.0, 1, 0.8);
    r.hit_at_beat(2.0, 2, 0.8);
    r.e.edit(|d| d.perform_undo());
    assert!(!r.e.is_loop_recording());
    assert_eq!(r.pattern(&id), [(5, 0.0, 0.8)]);
    assert_eq!(r.e.doc().undo_manager().undo_label(), Some("Toggle Step"));
    r.e.edit(|d| d.perform_redo());
    assert_eq!(r.pattern(&id).len(), 3);
}

#[test]
fn r2_2_undo_inside_an_open_gesture_undoes_the_gesture_only() {
    let mut r = Rig::new();
    let id = r.drum_track();
    step(&mut r, &id, 0, 0);
    step(&mut r, &id, 0, 4);
    let cut = |v: f64| json!({"filterCutoff": v}).as_object().unwrap().clone();
    r.e.edit(|d| {
        d.begin_undo_group("Pad Cutoff");
        d.update_pad(&id, 0, &cut(2000.0));
        d.update_pad(&id, 0, &cut(1500.0));
        d.perform_undo();
    });
    assert_eq!(r.e.doc().get_kit(&id).pads[0].filter_cutoff, 20000.0);
    assert_eq!(r.pattern(&id).len(), 2);
    r.e.edit(|d| d.end_undo_group());
    assert_eq!(r.e.doc().undo_manager().undo_label(), Some("Toggle Step"));
    assert!(r.e.doc().undo_manager().can_redo());
    r.e.edit(|d| d.perform_redo());
    assert_eq!(r.e.doc().get_kit(&id).pads[0].filter_cutoff, 1500.0);
}

#[test]
fn r2_2_restoring_a_snapshot_never_writes_history() {
    let mut r = Rig::new();
    let id = r.drum_track();
    step(&mut r, &id, 0, 0);
    settings(
        &mut r,
        json!({"bars": 4, "countInBars": 0, "metronome": false}),
    );
    let snap = r.e.doc().get_arrangement().clone();
    r.e.start_loop_recording(); // grows the pattern (structure change)
    let depth = r.e.doc().undo_manager().undo_len();
    r.e.edit(|d| d.restore_from_snapshot(&snap));
    assert_eq!(r.e.doc().undo_manager().undo_len(), depth);
    assert!(!r.e.is_loop_recording());
    assert_eq!(r.e.get_state(), TransportState::Stopped);
}

#[test]
fn u6_loop_recording_loops_the_pattern_without_rewriting_the_markers() {
    let mut r = Rig::new();
    r.drum_track();
    settings(&mut r, base());
    r.e.edit(|d| d.set_loop_markers(0.0, 12.0));
    r.e.start_loop_recording();
    let range = r.e.get_active_loop_range().unwrap();
    assert_eq!((range.start_beat, range.end_beat), (0.0, 4.0));
    r.e.edit(|d| d.set_bpm(60.0));
    r.run(4.5); // 4.5 beats at 60: wrapped at 4
    assert!(
        (r.e.get_position() - 0.5).abs() < 0.02,
        "{}",
        r.e.get_position()
    );
    r.e.stop_loop_recording();
    let m = r.e.doc().get_loop_markers().unwrap();
    assert_eq!((m.start_beat, m.end_beat), (0.0, 12.0));
    assert!(!r.e.doc().is_loop_enabled());
}

#[test]
fn b1_a_hit_right_after_record_lands_on_the_pattern_start() {
    let mut r = Rig::new();
    let id = r.drum_track();
    settings(&mut r, merged(json!({"overdub": true})));
    r.e.start_loop_recording();
    r.run(0.7);
    r.e.stop();
    r.run(0.06);
    r.e.start_loop_recording();
    r.pad_down(5, 0.9, None); // with the start, before the transport moved
    r.run(0.01);
    assert_eq!(r.pattern(&id), [(5, 0.0, 0.9)]);
    r.e.stop();
}

#[test]
fn r2_13_a_new_hit_plays_on_the_next_pass_at_its_swung_time() {
    let mut r = Rig::new();
    let id = r.drum_track();
    settings(&mut r, merged(json!({"swing": 0.5})));
    step(&mut r, &id, 0, 0);
    r.e.start_loop_recording();
    r.run(0.01);
    let t0 = r.time_of_beat(0.0);
    r.hit_at_beat(1.25, 3, 0.8); // stored straight at 1.25, sounds at 1.375 (odd 16th)
    assert_eq!(r.pattern(&id).iter().find(|n| n.0 == 3).unwrap().1, 1.25);
    r.run_until(t0 + 6.0 * SPB);
    let hits: Vec<f64> = r
        .pattern_hits()
        .iter()
        .filter(|h| h.1 == 3)
        .map(|h| (h.2 - t0) / SPB)
        .collect();
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert!((hits[0] - 5.375).abs() < 1e-6, "{hits:?}");
    // a louder hit on the same step merges (velocity 1)
    r.hit_at_beat(9.25, 3, 1.0);
    assert_eq!(r.pattern(&id).iter().find(|n| n.0 == 3).unwrap().2, 1.0);
    r.e.stop_loop_recording();
}

#[test]
fn stop_during_a_take_silences_the_kits_and_a_new_take_starts_clean() {
    let (mut r, id) = recording(json!({}));
    step(&mut r, &id, 0, 0);
    r.run(2.5);
    r.e.stop();
    let t = r.now();
    r.run(0.3);
    assert!(r.rms(t + 0.01, t + 0.3) < 1e-3); // the strip's disabled-FX leaks (−56 dB echoes, fx/xfade.rs) ring on
    let tracks =
        r.e.doc()
            .get_tracks()
            .iter()
            .filter(|t| t.instrument.kind == InstrumentType::Drums)
            .count();
    assert_eq!(tracks, 1);
}

#[test]
fn a_recorded_hit_notifies_state_listeners_once_and_playhead_frames_never() {
    // useArrangement / useBeat (review E4, L-a): engine-facing half
    let (mut r, id) = recording(json!({}));
    r.run(0.3);
    let v = r.e.doc().state_version();
    r.run(0.5); // playhead frames only
    assert_eq!(r.e.doc().state_version(), v);
    r.hit_at_beat(2.0, 3, 0.8);
    assert_eq!(r.e.doc().state_version(), v + 1);
    assert_eq!(r.pattern(&id).len(), 1);
}
