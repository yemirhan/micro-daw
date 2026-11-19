//! Transport on the real engine + core: play / playFrom / seek / setPosition / stop / auto-stop /
//! playPattern (U6, #1, #12), N1 / N4 (ArrangementEngine.round5.test.ts), B1, tempo, swing at
//! playback (#7, R2 swing), #11 note lengths, solo / mute at schedule time, stop silencing.

use serde_json::{Value, json};

use super::TransportState;
use super::test_rig::*;
use mdaw_model::doc::LoopSettingsPatch;
use mdaw_model::{Arrangement, InstrumentType, PadIndex};

fn settings(r: &mut Rig, v: Value) {
    let p: LoopSettingsPatch = serde_json::from_value(v).unwrap();
    r.e.edit(|d| d.set_loop_settings(&p));
}

fn steps(r: &mut Rig, id: &str, pad: PadIndex, steps: &[i64], step_beats: f64) {
    for s in steps {
        r.e.edit(|d| d.toggle_step(id, pad, *s, step_beats, None));
    }
}

/// Pattern-hit beats of `pad` relative to `t0`.
fn hit_beats(r: &Rig, pad: PadIndex, t0: f64, spb: f64) -> Vec<f64> {
    r.pattern_hits()
        .iter()
        .filter(|h| h.1 == pad)
        .map(|h| ((h.2 - t0) / spb * 1e6).round() / 1e6)
        .collect()
}

#[test]
fn play_starts_at_0_position_from_the_core_and_stop_resets() {
    let mut r = Rig::new();
    r.drum_track();
    r.e.set_position(5.0); // stopped: play() still starts at 0
    r.e.play();
    assert_eq!(r.e.get_state(), TransportState::Playing);
    assert_eq!(r.e.get_position(), 0.0);
    r.run(1.0);
    assert!((r.e.get_position() - 2.0).abs() < 0.01);
    r.e.stop();
    assert_eq!(r.e.get_state(), TransportState::Stopped);
    assert_eq!(r.e.get_position(), 0.0);
}

#[test]
fn b1_play_from_reports_the_cue_beat_before_the_start_then_the_clock() {
    let mut r = Rig::new();
    r.drum_track();
    r.e.play();
    r.run(1.0);
    r.e.stop();
    r.run(2.0); // idle
    r.e.play_from(6.0);
    assert_eq!(r.e.get_position(), 6.0, "not 0, not the old run");
    r.run(0.25);
    assert!((r.e.get_position() - 6.5).abs() < 0.01);
    r.e.stop();
    r.e.play();
    assert_eq!(r.e.get_position(), 0.0);
    r.run(0.003);
    assert!(r.e.get_position() < 0.02, "no jump from the previous run");
}

#[test]
fn the_position_comes_from_ticks_so_a_tempo_change_does_not_corrupt_it() {
    let mut r = Rig::new();
    r.drum_track();
    r.e.play();
    r.run(3.0);
    let p = r.e.get_position();
    assert!((p - 6.0).abs() < 0.01);
    r.e.edit(|d| d.set_bpm(60.0));
    r.run(0.003);
    assert!(
        (r.e.get_position() - p).abs() < 0.02,
        "seconds × new BPM would say 3"
    );
    r.run(1.0);
    assert!((r.e.get_position() - p - 1.0).abs() < 0.02);
}

#[test]
fn song_playback_without_a_loop_stops_past_the_song_end() {
    let mut r = Rig::new();
    r.drum_track();
    r.e.edit(|d| d.set_bpm(480.0)); // 64 beats in 8 s
    let len = r.e.doc().get_arrangement().length_beats;
    assert_eq!(len, 64.0);
    r.e.play();
    r.run(7.9);
    assert_eq!(r.e.get_state(), TransportState::Playing);
    r.run(0.2);
    assert_eq!(r.e.get_state(), TransportState::Stopped);
    // with the Song loop on it keeps looping
    r.e.edit(|d| {
        d.set_loop_enabled(true);
        d.set_loop_markers(0.0, 64.0);
    });
    r.e.play();
    r.run(9.0);
    assert_eq!(r.e.get_state(), TransportState::Playing);
    r.e.stop();
}

#[test]
fn n1_stop_during_a_count_in_cancels_the_pending_start() {
    let mut r = Rig::new();
    let id = r.drum_track();
    steps(&mut r, &id, 0, &[0, 4], 0.25);
    settings(
        &mut r,
        json!({"bars": 1, "countInBars": 1, "metronome": false}),
    );
    r.e.start_loop_recording();
    r.run(0.5);
    let t = r.now();
    r.e.stop();
    assert_eq!(r.e.get_state(), TransportState::Stopped);
    assert_eq!(r.e.get_count_in_remaining(), 0);
    r.run(3.0);
    assert!(
        r.pattern_hits().is_empty(),
        "the transport never starts on its own"
    );
    let late_clicks = r
        .events
        .iter()
        .filter(|e| matches!(e, super::EngineEvent::CountInClick { time, .. } if *time > t))
        .count();
    assert_eq!(late_clicks, 0, "pending count-in clicks are silenced");
    assert_eq!(r.e.shared().phase(), crate::core::Phase::Stopped);
    assert!(r.rms(t + 0.01, t + 3.0) < 1e-6);
}

#[test]
fn n4_a_seek_before_the_start_took_effect_starts_from_the_new_beat() {
    let mut r = Rig::new();
    let id = r.drum_track();
    steps(&mut r, &id, 0, &[0, 1, 2, 3], 1.0);
    r.e.play();
    r.e.seek(3.0); // before the core applied the start
    assert_eq!(r.e.get_state(), TransportState::Playing);
    assert_eq!(r.e.get_position(), 3.0);
    let t0 = r.now();
    r.run(0.4);
    let hits = hit_beats(&r, 0, t0, SPB);
    assert_eq!(
        hits,
        [0.0],
        "only beat 3, at the start: no step from beat 0"
    );
    assert!((r.e.get_position() - 3.8).abs() < 0.02);
    r.e.stop();
}

#[test]
fn n4_pattern_playback_is_kept_and_a_late_seek_just_moves() {
    let mut r = Rig::new();
    r.drum_track();
    r.e.play_pattern();
    r.e.seek(1.0);
    assert!(r.e.is_pattern_playback());
    assert_eq!(r.e.get_state(), TransportState::Playing);
    r.e.stop();
    r.e.play();
    r.run(0.5);
    r.e.seek(2.0);
    assert_eq!(r.e.get_position(), 2.0);
    r.run(0.25);
    assert!((r.e.get_position() - 2.5).abs() < 0.01);
    r.e.stop();
    r.e.seek(4.0); // stopped: no-op
    assert_eq!(r.e.get_position(), 0.0);
}

#[test]
fn u6_beat_play_loops_the_pattern_transiently() {
    let mut r = Rig::new();
    r.drum_track();
    settings(&mut r, json!({"bars": 1}));
    r.e.edit(|d| {
        d.set_loop_markers(4.0, 14.0);
        d.clear_history();
    });
    r.e.play_pattern();
    let m = r.e.doc().get_loop_markers().unwrap();
    assert_eq!((m.start_beat, m.end_beat), (4.0, 14.0));
    assert!(!r.e.doc().is_loop_enabled());
    let range = r.e.get_active_loop_range().unwrap();
    assert_eq!((range.start_beat, range.end_beat), (4.0, 8.0));
    assert_eq!(r.e.get_position(), 4.0);
    // tempo change while pattern-playing: the PATTERN loop holds
    r.e.edit(|d| d.set_bpm(60.0));
    r.run(4.5);
    assert!(
        (r.e.get_position() - 4.5).abs() < 0.02,
        "{}",
        r.e.get_position()
    );
    r.e.edit(|d| d.set_bpm(BPM));
    r.e.stop();
    assert!(
        r.e.get_active_loop_range().is_none(),
        "back to the Song loop (off)"
    );
    assert!(!r.e.doc().undo_manager().can_undo());
    // Song loop on: Beat Play still loops the pattern, stop restores the markers' loop
    r.e.edit(|d| d.set_loop_enabled(true));
    r.e.play_pattern();
    assert_eq!(r.e.get_active_loop_range().unwrap().end_beat, 8.0);
    r.e.stop();
    let range = r.e.get_active_loop_range().unwrap();
    assert_eq!((range.start_beat, range.end_beat), (4.0, 14.0));
    r.e.play();
    r.run(7.5); // 15 beats: wraps at 14 back to 4
    assert!(
        (r.e.get_position() - 5.0).abs() < 0.02,
        "{}",
        r.e.get_position()
    );
    r.e.stop();
}

fn legacy() -> Arrangement {
    serde_json::from_value(json!({
        "id": "a", "name": "Old", "bpm": 120, "lengthBeats": 16,
        "tracks": [{
            "id": "old-drums", "name": "Drums", "instrument": {"type": "drums", "presetIndex": 0},
            "regions": [{"id": "r", "startBeat": 0, "lengthBeats": 16, "notes": [
                {"note": 36, "startBeat": 2.5, "durationBeats": 0.25, "velocity": 0.8, "isDrum": true},
                {"note": 36, "startBeat": 10.5, "durationBeats": 0.25, "velocity": 0.8, "isDrum": true}
            ]}],
            "volume": -6, "pan": 0, "muted": false, "solo": false, "color": "#f00"
        }]
    }))
    .unwrap()
}

#[test]
fn r2_1_a_template_without_markers_gets_the_pattern_range_and_play_pattern_loops_it() {
    let mut r = Rig::new();
    r.e.edit(|d| d.load_from_template(&legacy()).unwrap());
    let m = r.e.doc().get_loop_markers().unwrap();
    assert_eq!((m.start_beat, m.end_beat), (0.0, 8.0));
    r.e.play_pattern();
    assert_eq!(r.e.get_state(), TransportState::Playing);
    assert!(r.e.is_pattern_playback());
    let t0 = r.now();
    r.run(8.2); // 16 beats: two passes of [0, 8)
    let hits = hit_beats(&r, 0, t0, SPB);
    assert_eq!(
        hits,
        [2.5, 10.5],
        "the note at 10.5 is outside the pattern loop"
    );
    r.e.stop();
    r.e.edit(|d| d.set_loop_markers(16.0, 20.0));
    settings(&mut r, json!({"bars": 1}));
    r.e.edit(|d| d.set_loop_markers(16.0, 19.0));
    r.e.play_pattern();
    let m = r.e.doc().get_loop_markers().unwrap();
    assert_eq!((m.start_beat, m.end_beat), (16.0, 19.0), "U6: markers kept");
    let range = r.e.get_active_loop_range().unwrap();
    assert_eq!((range.start_beat, range.end_beat), (16.0, 20.0));
    assert!((r.e.get_position() - 16.0).abs() < 1e-6);
    r.e.stop();
}

#[test]
fn r2_12_the_beat_metronome_setting_clicks_during_plain_pattern_playback() {
    let mut r = Rig::new();
    r.drum_track();
    settings(&mut r, json!({"metronome": true}));
    r.e.play_pattern();
    r.run(1.1);
    let n = r.clicks().len();
    assert!(n >= 2);
    assert!(r.clicks()[0].1, "beat 0 is accented");
    settings(&mut r, json!({"metronome": false}));
    r.run(1.0);
    assert_eq!(r.clicks().len(), n);
    r.e.stop();
    // Song play: only the global toggle clicks
    settings(&mut r, json!({"metronome": true}));
    r.e.play();
    r.run(1.0);
    assert_eq!(r.clicks().len(), n);
    r.e.stop();
    let v = r.e.doc().state_version();
    r.e.set_metronome_enabled(true);
    assert!(r.e.is_metronome_enabled());
    assert_eq!(r.e.doc().state_version(), v + 1);
    r.e.set_metronome_enabled(true);
    assert_eq!(r.e.doc().state_version(), v + 1);
    r.e.play();
    r.run(1.0);
    assert!(r.clicks().len() > n);
}

#[test]
fn swing_is_applied_at_playback_and_reapplied_when_it_changes() {
    let mut r = Rig::new();
    let id = r.drum_track();
    settings(&mut r, json!({"bars": 1, "quantize": "1/16", "swing": 0}));
    steps(&mut r, &id, 0, &[0, 1], 0.25);
    r.e.play_pattern();
    let t0 = r.now();
    r.run(0.3);
    settings(&mut r, json!({"swing": 0.5})); // rebuilt live
    r.run(2.0);
    settings(&mut r, json!({"quantize": "1/8T"})); // the record quantize does not change playback
    r.run(2.0);
    settings(&mut r, json!({"swingGrid": "1/8"})); // 0.25 is not on the 1/8 swing grid
    r.run(2.0);
    let hits = hit_beats(&r, 0, t0, SPB);
    assert_eq!(hits, [0.0, 0.25, 4.0, 4.375, 8.0, 8.375, 12.0, 12.25]);
    r.e.stop();
}

#[test]
fn r2_7_quantize_quarter_with_swing_does_not_delay_beats_2_and_4() {
    let mut r = Rig::new();
    let id = r.drum_track();
    settings(&mut r, json!({"bars": 1, "quantize": "1/4", "swing": 0.5}));
    assert_eq!(
        serde_json::to_value(r.e.doc().get_loop_settings().swing_grid).unwrap(),
        json!("1/16")
    );
    steps(&mut r, &id, 0, &[0, 1, 2, 3], 1.0);
    r.e.play();
    let t0 = r.now();
    r.run(2.1);
    assert_eq!(hit_beats(&r, 0, t0, SPB), [0.0, 1.0, 2.0, 3.0]);
    r.e.stop();
}

#[test]
fn r2_11_gate_note_lengths_follow_the_tempo_at_play_time() {
    let mut r = Rig::new();
    let id = r.drum_track();
    let kit = test_kit(&[0]);
    let p = json!({"mode": "gate", "decay": 0.005})
        .as_object()
        .unwrap()
        .clone();
    r.e.edit(|d| {
        d.set_kit(&id, &kit, Default::default());
        d.update_pad(&id, 0, &p);
    });
    r.e.wait_kits_ready();
    settings(&mut r, json!({"bars": 1}));
    steps(&mut r, &id, 0, &[4], 0.25);
    let region = r.e.doc().get_pattern_region(&id).unwrap().clone();
    let notes: Vec<_> = region
        .notes
        .iter()
        .map(|n| mdaw_model::RegionNote {
            duration_beats: 0.5,
            ..n.clone()
        })
        .collect();
    r.e.edit(|d| d.update_region_notes(&id, &region.id, notes));
    r.e.play();
    r.run(0.1);
    r.e.edit(|d| d.set_bpm(60.0));
    r.run(1.5);
    let on = r.pattern_hits()[0].2;
    // 0.5 beat at 60 BPM = 0.5 s (was 0.25 s at the old 120)
    assert!(r.rms(on + 0.3, on + 0.45) > 0.01, "still held at 0.45 s");
    let held = r.rms(on + 0.3, on + 0.45);
    assert!(
        r.rms(on + 0.52, on + 0.7) < 0.01 * held,
        "released at 0.5 s + decay"
    );
    r.e.stop();
}

#[test]
fn solo_and_mute_apply_at_schedule_time_and_mute_is_live() {
    let mut r = Rig::new();
    let ids: Vec<String> = (0..3)
        .map(|_| r.e.edit(|d| d.add_track(InstrumentType::Drums, 0)).id)
        .collect();
    for id in &ids {
        let kit = test_kit(&[]);
        r.e.edit(|d| {
            d.set_kit(id, &kit, Default::default());
            d.toggle_step(id, 0, 0, 0.25, None);
        });
    }
    r.e.wait_kits_ready();
    r.e.edit(|d| {
        d.set_track_mute(&ids[0], true);
        d.set_track_solo(&ids[2], true);
    });
    r.e.play();
    r.run(0.2);
    let tracks: Vec<String> = r.pattern_hits().iter().map(|h| h.0.clone()).collect();
    assert_eq!(tracks, [ids[2].clone()]);
    r.e.stop();
    r.events.clear();
    r.e.edit(|d| d.set_track_solo(&ids[2], false));
    r.e.play();
    r.run(0.2);
    let mut tracks: Vec<String> = r.pattern_hits().iter().map(|h| h.0.clone()).collect();
    tracks.sort();
    let mut want = vec![ids[1].clone(), ids[2].clone()];
    want.sort();
    assert_eq!(tracks, want);
    // muting during playback mutes the volume live (no reschedule)
    r.e.edit(|d| d.set_track_mute(&ids[1], true));
    r.e.edit(|d| d.toggle_step(&ids[1], 0, 1, 0.25, None)); // steps keep flowing
    let slot = r.e.track_slot(&ids[1]).unwrap();
    r.run(0.3);
    assert_eq!(r.e.shared().slot_mean_square(slot), 0.0);
    r.e.stop();
}

#[test]
fn stop_silences_the_sampler_voices_of_every_drum_track() {
    let mut r = Rig::new();
    let mut ids = Vec::new();
    for _ in 0..2 {
        let id = r.e.edit(|d| d.add_track(InstrumentType::Drums, 0)).id;
        let kit = test_kit(&[0]);
        r.e.edit(|d| {
            d.set_kit(&id, &kit, Default::default());
            d.toggle_step(&id, 0, 0, 0.25, None);
        });
        ids.push(id);
    }
    r.e.wait_kits_ready();
    r.e.play();
    r.run(0.5);
    assert!(r.rms(0.2, 0.5) > 0.01, "the 2 s samples ring");
    r.e.stop();
    let t = r.now();
    r.run(0.2);
    let before = r.rms(t - 0.2, t);
    assert!(
        r.rms(t + 0.01, t + 0.2) < 0.01 * before,
        "5 ms fade at stop"
    );
}

#[test]
fn step_edits_during_playback_are_heard_on_the_next_pass_and_clear_pattern_silences() {
    // refreshPlaybackSchedule (toggleStep / setStepVelocity / clearPattern while playing)
    let mut r = Rig::new();
    let id = r.drum_track();
    settings(&mut r, json!({"bars": 1}));
    r.e.play_pattern();
    let t0 = r.now();
    r.run(0.5);
    r.e.edit(|d| d.toggle_step(&id, 2, 2, 0.25, None)); // beat 0.5: already passed
    r.e.edit(|d| d.toggle_step(&id, 3, 12, 0.25, None)); // beat 3: still ahead
    r.run(2.0);
    assert_eq!(hit_beats(&r, 3, t0, SPB), [3.0]);
    assert_eq!(hit_beats(&r, 2, t0, SPB), [4.5]);
    r.e.edit(|d| d.clear_pattern(&id, None));
    r.run(2.0);
    assert_eq!(r.pattern_hits().len(), 2);
    r.e.stop();
}
