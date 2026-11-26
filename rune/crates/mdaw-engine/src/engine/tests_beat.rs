//! Kits, GM migration, the starter project, Song (linear / punch) recording, N2 kit-selection
//! events, Song audio regions (the start-inside quirk, B3 in-place reload,
//! ArrangementEngine.reload.test.ts; N5 unavailable samples), on the real engine + core.

use std::sync::Arc;

use serde_json::json;

use super::TransportState;
use super::test_rig::*;
use mdaw_dsp::buffer::AudioBuffer;
use mdaw_model::doc::{KitSelectedEvent, KitSelectionSource};
use mdaw_model::types::SampleRef;
use mdaw_model::{Arrangement, InstrumentType};

fn obj(v: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
    v.as_object().unwrap().clone()
}

/// Peak output RMS over 5 ms windows in [a, b).
fn peak_rms(r: &Rig, a: f64, b: f64) -> f64 {
    let mut m = 0f64;
    let mut t = a;
    while t < b {
        m = m.max(r.rms(t, t + 0.005));
        t += 0.005;
    }
    m
}

#[test]
fn pad_edits_reach_the_sampler_and_undo_restores_them() {
    let mut r = Rig::new();
    let id = r.drum_track();
    r.pad_down(2, 1.0, None);
    r.run(0.05);
    let loud = peak_rms(&r, 0.0, 0.05);
    r.e.edit(|d| d.update_pad(&id, 2, &obj(json!({"gainDb": -12, "pitch": 5}))));
    assert_eq!(r.e.doc().get_kit(&id).pads[2].gain_db, -12.0);
    r.run(0.05); // the channel gain ramps over 20 ms
    let t = r.now();
    r.pad_down(2, 1.0, None);
    r.run(0.05);
    let quiet = peak_rms(&r, t, t + 0.05);
    let db = 20.0 * (quiet / loud).log10();
    assert!((db + 12.0).abs() < 1.5, "{db}");
    r.e.edit(|d| d.perform_undo());
    assert_eq!(r.e.doc().get_kit(&id).pads[2].gain_db, 0.0);
    r.run(0.05);
    let t = r.now();
    r.pad_down(2, 1.0, None);
    r.run(0.05);
    assert!((peak_rms(&r, t, t + 0.05) / loud - 1.0).abs() < 0.05);
}

#[test]
fn set_kit_replaces_the_whole_kit_and_reports_the_selection() {
    let mut r = Rig::new();
    let id = r.drum_track();
    r.e.take_kit_selected();
    let kit = test_kit(&[0]);
    r.e.edit(|d| d.set_kit(&id, &kit, KitSelectionSource::PackLoad));
    r.e.wait_kits_ready();
    r.pad_down(0, 1.0, None);
    r.run(0.5);
    assert!(r.rms(0.3, 0.5) > 0.01, "pad 0 now plays the 2 s sample");
    r.e.edit(|d| d.update_pad(&id, 0, &obj(json!({"gainDb": -6}))));
    assert_eq!(
        r.e.take_kit_selected(),
        [KitSelectedEvent {
            track_id: id.clone(),
            kit_id: "test".into(),
            source: KitSelectionSource::PackLoad
        }],
        "pad edits are not kit selections (N2)"
    );
}

#[test]
fn n2_the_session_changes_on_new_template_open_not_on_edits_or_undo() {
    let mut r = Rig::new();
    let s0 = r.e.doc().get_session_id();
    r.e.edit(|d| {
        d.add_track(InstrumentType::Synth, 0);
        d.set_bpm(100.0);
        d.perform_undo();
        d.perform_redo();
    });
    assert_eq!(r.e.doc().get_session_id(), s0);
    r.e.edit(|d| d.reset_to_starter());
    let s1 = r.e.doc().get_session_id();
    assert_ne!(s1, s0);
    let arr = r.e.doc().get_arrangement().clone();
    r.e.edit(|d| d.load_project(&arr));
    assert_ne!(r.e.doc().get_session_id(), s1);
}

#[test]
fn reset_to_starter_one_active_drums_track_with_the_factory_kit_and_pattern_loop() {
    let mut r = Rig::new();
    r.e.edit(|d| {
        d.add_track(InstrumentType::Synth, 0);
        d.reset_to_starter();
    });
    let tracks = r.e.doc().get_tracks();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].name, "Drums");
    let id = tracks[0].id.clone();
    assert_eq!(r.e.doc().get_kit(&id).id, "factory");
    assert_eq!(r.e.live_sink_slot(), r.e.track_slot(&id));
    let m = r.e.doc().get_loop_markers().unwrap();
    assert_eq!((m.start_beat, m.end_beat), (0.0, 8.0));
    assert!(!r.e.doc().undo_manager().can_undo());
}

#[test]
fn gm_migration_on_restore_plays_the_mapped_pads() {
    let notes: Vec<serde_json::Value> = [36, 38, 42, 46, 99]
        .iter()
        .enumerate()
        .map(|(i, n)| {
            json!({"note": n, "startBeat": i as f64 * 0.5, "durationBeats": 0.25, "velocity": 100, "isDrum": true})
        })
        .collect();
    let legacy: Arrangement = serde_json::from_value(json!({
        "id": "a", "name": "Old", "bpm": 120, "lengthBeats": 16,
        "tracks": [{
            "id": "old-drums", "name": "Drums", "instrument": {"type": "drums", "presetIndex": 0},
            "regions": [{"id": "r", "startBeat": 0, "lengthBeats": 4, "notes": notes}],
            "volume": -6, "pan": 0, "muted": false, "solo": false, "color": "#f00"
        }]
    }))
    .unwrap();
    let mut r = Rig::new();
    r.e.edit(|d| d.restore_from_snapshot(&legacy));
    assert!(
        !r.e.take_warnings().is_empty(),
        "the unmapped note 99 is reported"
    );
    let id = r.e.doc().get_tracks()[0].id.clone();
    assert_eq!(r.e.doc().get_kit(&id).id, "factory");
    let pads: Vec<f64> = r.e.doc().get_tracks()[0].regions[0]
        .notes
        .iter()
        .map(|n| n.note)
        .collect();
    assert_eq!(pads.len(), 4);
    r.e.wait_kits_ready();
    r.e.play();
    r.run(2.1);
    assert_eq!(r.pattern_hits().len(), 4);
    assert!(peak_rms(&r, 0.0, 2.0) > 1e-3);
}

#[test]
fn r2_14_new_drum_tracks_are_numbered() {
    let mut r = Rig::new();
    assert_eq!(
        r.e.edit(|d| d.add_track(InstrumentType::Drums, 0)).name,
        "Drums"
    );
    assert_eq!(
        r.e.edit(|d| d.add_track(InstrumentType::Drums, 0)).name,
        "Drums 2"
    );
    let synth = r.e.edit(|d| d.add_track(InstrumentType::Synth, 0)).id;
    let inst = serde_json::from_value(json!({"type": "drums", "presetIndex": 0})).unwrap();
    r.e.edit(|d| d.set_track_instrument(&synth, &inst));
    let t =
        r.e.doc()
            .get_tracks()
            .iter()
            .find(|t| t.id == synth)
            .unwrap();
    assert_eq!(t.name, "Drums 3");
    assert!(r.e.track_slot(&synth).is_some(), "rebuilt as a drum track");
}

#[test]
fn arming_a_drum_track_makes_it_the_live_sink() {
    let mut r = Rig::new();
    let a = r.e.edit(|d| d.add_track(InstrumentType::Drums, 0)).id;
    let b = r.e.edit(|d| d.add_track(InstrumentType::Drums, 0)).id;
    assert_eq!(r.e.live_sink_slot(), r.e.track_slot(&a));
    r.e.edit(|d| d.arm_track_for_recording(Some(&b)));
    assert_eq!(r.e.live_sink_slot(), r.e.track_slot(&b));
    let s = r.e.edit(|d| d.add_track(InstrumentType::Synth, 0)).id;
    r.e.edit(|d| d.arm_track_for_recording(Some(&s)));
    assert_eq!(r.e.live_sink_slot(), r.e.track_slot(&b));
}

// --- Song recording -------------------------------------------------------------------------

#[test]
fn song_recording_captures_pads_and_synth_notes_into_a_region_from_0() {
    let mut r = Rig::new();
    let id = r.drum_track();
    r.e.edit(|d| d.arm_track_for_recording(Some(&id)));
    r.e.start_recording_to_track(None);
    assert_eq!(r.e.get_state(), TransportState::Recording);
    assert_eq!(r.e.get_recording_track_id(), Some(id.as_str()));
    assert!(!r.e.is_punch_recording_active());
    r.run(0.01);
    r.hit_at_beat(1.0, 3, 0.7);
    let t = r.time_of_beat(2.0);
    r.run_until(t);
    r.e.capture_note_on(60.0, 0.9, Some(t));
    r.run_until(t + 0.75); // 1.5 beats
    let live = r.e.get_live_region().unwrap();
    assert_eq!(live.id, super::LIVE_REGION_ID);
    assert_eq!(live.notes.len(), 2, "the held note is shown");
    r.e.capture_note_off(60.0, Some(t + 0.75));
    r.run_until(t + 1.6);
    let region = r.e.stop_recording_to_track().unwrap();
    assert_eq!(r.e.get_state(), TransportState::Stopped);
    assert_eq!(region.start_beat, 0.0);
    assert_eq!(region.length_beats, 6.0, "max(1, ceil(end beat ≈ 5.2))");
    let n: Vec<(f64, f64, f64)> = region
        .notes
        .iter()
        .map(|n| {
            (
                n.note,
                (n.start_beat * 1e3).round() / 1e3,
                (n.duration_beats * 1e3).round() / 1e3,
            )
        })
        .collect();
    assert_eq!(n, [(39.0, 1.0, 0.25), (60.0, 2.0, 1.5)]);
    assert_eq!(r.e.doc().undo_manager().undo_label(), Some("Record"));
}

#[test]
fn song_recording_with_nothing_captured_still_pushes_record_and_auto_creates_a_synth_track() {
    let mut r = Rig::new();
    r.e.start_recording_to_track(None);
    let t = &r.e.doc().get_tracks()[0];
    assert_eq!(t.instrument.kind, InstrumentType::Synth);
    assert_eq!(r.e.doc().get_armed_track_id(), Some(t.id.as_str()));
    r.run(0.2);
    assert!(r.e.stop_recording_to_track().is_none());
    assert_eq!(r.e.doc().undo_manager().undo_label(), Some("Record"));
}

#[test]
fn punch_recording_loops_the_markers_and_replaces_overlapping_regions() {
    let mut r = Rig::new();
    let id = r.drum_track();
    r.e.edit(|d| {
        d.toggle_step(&id, 0, 0, 0.25, None); // a pattern region at [0, 8)
        d.set_loop_enabled(true);
        d.set_loop_markers(4.0, 8.0);
    });
    r.e.start_recording_to_track(Some(&id));
    assert!(r.e.is_punch_recording_active());
    assert_eq!(r.e.get_position(), 4.0);
    r.run(0.01);
    r.hit_at_beat(5.0, 1, 0.8);
    r.hit_at_beat(10.0, 2, 0.8); // second pass, position 6
    // a synth note held across the loop end is closed by the pass's loop-close callback
    let t = r.time_of_beat(11.0);
    r.run_until(t);
    r.e.capture_note_on(64.0, 0.5, Some(t));
    r.run_until(r.time_of_beat(12.5));
    let region = r.e.stop_recording_to_track().unwrap();
    assert_eq!((region.start_beat, region.length_beats), (4.0, 4.0));
    let n: Vec<(f64, f64, f64)> = region
        .notes
        .iter()
        .map(|n| (n.note, (n.start_beat * 1e3).round() / 1e3, n.duration_beats))
        .collect();
    assert_eq!(n, [(37.0, 1.0, 0.25), (38.0, 2.0, 0.25), (64.0, 3.0, 1.0)]);
    let regions = &r.e.doc().get_tracks()[0].regions;
    assert_eq!(
        regions.len(),
        1,
        "the overlapping pattern region was replaced"
    );
}

#[test]
fn b1_song_record_a_hit_before_the_scheduled_start_is_stored_at_the_record_start() {
    let mut r = Rig::new();
    let id = r.drum_track();
    r.e.play();
    r.run(1.0);
    r.e.stop();
    r.e.edit(|d| d.arm_track_for_recording(Some(&id)));
    r.e.start_recording_to_track(Some(&id));
    r.pad_down(1, 0.8, None);
    r.run(0.01);
    assert_eq!(r.e.get_live_region().unwrap().notes[0].start_beat, 0.0);
    r.e.stop();
}

// --- Song audio regions ---------------------------------------------------------------------

fn dc(value: f32, seconds: f64) -> Arc<AudioBuffer> {
    Arc::new(AudioBuffer::mono(
        SR,
        vec![value; (seconds * SR as f64) as usize],
    ))
}

fn audio_track(r: &mut Rig, sample: &str, at: f64, seconds: f64) -> (String, String) {
    r.samples
        .buffers
        .lock()
        .unwrap()
        .insert(sample.into(), dc(0.25, seconds));
    let t = r.e.edit(|d| d.add_track(InstrumentType::Audio, 0)).id;
    let s = SampleRef {
        id: sample.into(),
        name: sample.into(),
        path: format!("/data/packs/uzu/{sample}.wav"),
        duration_seconds: seconds,
        sample_rate: SR as f64,
        channels: 1,
    };
    let region = r.e.edit(|d| d.add_audio_region(&t, Some(&s), at)).unwrap();
    (t, region.id)
}

#[test]
fn audio_regions_play_from_their_start_and_from_inside_when_playback_lands_there() {
    let mut r = Rig::new();
    audio_track(&mut r, "s1", 1.0, 1.0); // beats [1, 3)
    r.e.play();
    r.run(2.0);
    assert!(r.rms(0.05, 0.45) < 1e-6);
    assert!(r.rms(0.6, 1.4) > 0.05, "plays at beat 1 for its length");
    assert!(r.rms(1.6, 1.9) < 1e-3); // the strip's disabled-FX leaks (−56 dB echoes, fx/xfade.rs) ring on
    r.e.stop();
    // Playback from inside the region plays the rest of it (the Electron app's quirk, silence
    // until the next clip start, is gone: user report 2026-11-26).
    let t = r.now();
    r.e.play_from(2.0);
    r.run(0.8);
    assert!(r.rms(t + 0.05, t + 0.45) > 0.05, "the second half plays");
    assert!(
        r.rms(t + 0.6, t + 0.8) < 1e-3,
        "and ends where the clip ends"
    );
    r.e.stop();
}

/// Moving the playhead while the Song plays: the clip under the new place plays from that point
/// (and one left behind stops), in both directions.
#[test]
fn seeking_while_playing_plays_the_clip_under_the_playhead() {
    let mut r = Rig::new();
    audio_track(&mut r, "s1", 4.0, 2.0); // beats [4, 8)
    r.e.play();
    r.run(0.3);
    assert!(r.rms(0.05, 0.3) < 1e-6, "nothing before the clip");
    // into the middle of the clip
    let t = r.now();
    r.e.locate(6.0);
    r.run(0.5);
    assert!(
        r.rms(t + 0.05, t + 0.5) > 0.05,
        "the clip plays from beat 6"
    );
    // back out of it: it stops
    let t = r.now();
    r.e.locate(0.0);
    r.run(0.5);
    assert!(r.rms(t + 0.1, t + 0.5) < 1e-3, "left behind: silent");
    // and in again
    let t = r.now();
    r.e.locate(5.0);
    r.run(0.4);
    assert!(r.rms(t + 0.05, t + 0.4) > 0.05);
    r.e.stop();
}

#[test]
fn b3_a_reload_while_playing_reaches_the_next_start_of_the_region() {
    let mut r = Rig::new();
    let (track, _) = audio_track(&mut r, "s1", 0.0, 0.5); // beat [0, 1)
    r.e.edit(|d| {
        d.set_loop_enabled(true);
        d.set_loop_markers(0.0, 2.0);
    });
    r.e.play();
    r.run(0.3);
    r.samples
        .buffers
        .lock()
        .unwrap()
        .insert("s1".into(), dc(0.5, 0.5));
    r.e.refresh_audio_players(&["s1".to_string()]);
    let t = r.now();
    r.run(1.2); // the next pass starts at t ≈ 1.0 s
    let slot = r.e.track_slot(&track).unwrap();
    assert!(slot < 64);
    let old = r.rms(t + 0.02, t + 0.15);
    let new = r.rms(1.05, 1.4);
    assert!(old > 0.01 && (new / old - 2.0).abs() < 0.05, "{old} {new}");
    assert_eq!(
        r.e.doc()
            .get_tracks()
            .iter()
            .find(|x| x.id == track)
            .unwrap()
            .regions[0]
            .audio
            .as_ref()
            .unwrap()
            .sample_id,
        "s1"
    );
    r.e.stop();
}

#[test]
fn b3_a_reload_while_stopped_is_picked_up_by_the_next_play_and_a_removed_pack_keeps_playing() {
    let mut r = Rig::new();
    audio_track(&mut r, "s1", 0.0, 0.5);
    r.samples
        .buffers
        .lock()
        .unwrap()
        .insert("s1".into(), dc(0.5, 0.5));
    r.e.refresh_audio_players(&["s1".to_string()]);
    r.e.play();
    r.run(0.3);
    let a = r.rms(0.05, 0.25);
    r.e.stop();
    // N5: the pack is gone; the loaded audio (kept by the sample manager) still plays
    r.samples.unavailable.lock().unwrap().push("s1".into());
    assert_eq!(r.e.get_unavailable_audio_regions().len(), 1);
    assert!(r.e.is_sample_unavailable("s1"));
    let t = r.now();
    r.e.play();
    r.run(0.3);
    let b = r.rms(t + 0.05, t + 0.25);
    assert!(a > 0.01 && (a - b).abs() < 1e-3, "{a} {b}");
    r.e.stop();
}
