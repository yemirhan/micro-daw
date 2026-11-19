//! Beat-mode unit tests: ports of the PURE cases of `src/services/ArrangementEngine.beat.test.ts`
//! and the engine-facing assertions of `src/hooks/useBeat.test.ts`. Loop-recording cases are
//! ported on their data half: the recorder's quantize / timing is replaced by the quantized
//! loop-relative beat it would compute, fed to `loop_take_record_hit`.
//!
//! SKIPPED (they need transport / audio / the loop recorder's timing; phase 3 ports them):
//! - beat.test "drum tracks and the active drum track": the live sink halves of "a new drum track
//!   gets the factory kit, becomes active and receives live pad hits", "re-registers the sink when
//!   the active track changes or is removed" and "re-registers the sink after undo rebuilds the
//!   track audio" (only `SyncDrumSink` / `RebuildAll` emission is checked here).
//! - beat.test "loop recording": "creates the drum track + pattern region and loops the pattern
//!   range" (transport half; the data half is ported), "quantizes hits and wraps them into the
//!   loop", "quantizes against the swung grid but stores straight; triplet grids", "note-repeat
//!   hits on a finer grid than the quantize keep their own grid", "merges hits into playback live
//!   and does not replay a hit that was snapped forward", "count-in: remaining beats, early hits
//!   ignored, pre-roll hits land on beat 1", "starts the metronome during loop play only when the
//!   setting is on", "ignores synth notes and note-offs".
//! - beat.test "review regressions": "position comes from transport ticks", "captureNoteOn/Off
//!   take an optional audio time", "count-in clicks live on their own synth", "a failing start
//!   rolls back to stopped and notifies" (data half: `abort_loop_take_start` is tested), "the
//!   metronome never clicks at/before its last click", "stop silences the sampler voices",
//!   "loop recording: gate pads get their held length (wrapped, capped)" (only the note update
//!   `loop_take_set_gate_length` is tested), "swing is applied at playback to odd steps"
//!   (only the `SwingChanged` / `RescheduleTrack` events are tested).
//! - beat.test "review round 2": "#3 switching the active drum track releases the pads held on
//!   the old sink", "#5 a gate pad held while the tempo changes", "#7 swing uses its own grid:
//!   quantize 1/4 + swing" (playback times), "#7 loop recording snaps to the quantize grid
//!   against the sounding swing", "#11 note lengths follow the tempo at play time", "#12 the
//!   beat-view metronome setting clicks during plain pattern playback", "#13 a new recorded hit is
//!   added to the running Part", "#10 setTrackEffect(compressor) puts the compressor in the path".
//! - beat.test "review round 3": "E1 a gate held across 60 -> 300 BPM", "E1 loop wraps are counted
//!   from the transport loop events", "U5 the metronome clicks ~9 dB lower", "setMetronomeEnabled
//!   notifies state listeners" (engine-only metronome state); the transport halves of the two U6
//!   cases (their data halves are ported).
//! - beat.test "review round 4": every case (B1 transport start/stop/tick hazards, L-e wrap
//!   pruning).
//! - useBeat.test: "saves the current kit to the kit store" (KitStore), the three "step gestures"
//!   cases (hook timers; they only call begin/endUndoGroup, covered by the group tests), "Beat
//!   Play starts the pattern through the engine playPattern()" (hook wiring), "lists drum tracks
//!   with numbered duplicate names" (`listDrumTracks` is a hook helper), the render-count halves
//!   of "L-a pattern edits and recorded hits re-render usePatternRegion() callers only".

use serde_json::{Map, Value, json};

use super::tests::{counter_doc, templates};
use super::*;
use crate::types::{
    Arrangement, InstrumentType, KitState, LoopMarkers, PadPlayMode, PadSample, PadSampleSource,
    QuantizeGrid, RegionNote, pad_to_note,
};

fn patch(v: Value) -> Map<String, Value> {
    v.as_object().cloned().unwrap()
}

/// `notesOf`: (pad, beat, velocity) of the pattern region's notes.
fn notes_of(doc: &ArrangementDoc, id: &str) -> Vec<(i64, f64, f64)> {
    doc.get_pattern_region(id)
        .map(|r| {
            r.notes
                .iter()
                .map(|n| (n.note as i64 - 36, n.start_beat, n.velocity))
                .collect()
        })
        .unwrap_or_default()
}

fn label(doc: &ArrangementDoc) -> Option<&str> {
    doc.undo_manager().undo_label()
}

fn bars(n: u32) -> LoopSettingsPatch {
    LoopSettingsPatch {
        bars: Some(n),
        ..Default::default()
    }
}

fn no_count_in(n: u32) -> LoopSettingsPatch {
    LoopSettingsPatch {
        bars: Some(n),
        count_in_bars: Some(0),
        metronome: Some(false),
        ..Default::default()
    }
}

fn legacy_arrangement() -> Arrangement {
    let notes: Vec<Value> = [36, 38, 42, 46, 99]
        .iter()
        .enumerate()
        .map(|(i, n)| json!({ "note": n, "startBeat": i as f64 * 0.5, "durationBeats": 0.25, "velocity": 100, "isDrum": true }))
        .collect();
    serde_json::from_value(json!({
        "id": "a", "name": "Old", "bpm": 120, "lengthBeats": 16,
        "tracks": [{
            "id": "old-drums", "name": "Drums", "instrument": { "type": "drums", "presetIndex": 0 },
            "regions": [{ "id": "r", "startBeat": 0, "lengthBeats": 4, "notes": notes }],
            "volume": -6, "pan": 0, "muted": false, "solo": false, "color": "#f00"
        }]
    }))
    .unwrap()
}

// --- drum tracks and the active drum track ---

#[test]
fn a_new_drum_track_gets_the_factory_kit_and_becomes_active() {
    let mut doc = counter_doc();
    assert_eq!(doc.get_active_drum_track_id(), None);
    let id = doc.ensure_drum_track();
    assert_eq!(doc.get_active_drum_track_id(), Some(id.clone()));
    let kit = doc.get_kit(&id);
    assert_eq!(kit.pads.len(), 16);
    assert!(matches!(
        kit.pads[0].sample.as_ref().unwrap().source,
        PadSampleSource::Factory {
            voice: crate::types::FactoryVoiceId::Kick
        }
    ));
    assert_eq!(doc.ensure_drum_track(), id);
    assert!(doc.drain_events().contains(&DocEvent::SyncDrumSink));
}

#[test]
fn the_active_drum_track_follows_set_and_remove() {
    let mut doc = counter_doc();
    let a = doc.add_track(InstrumentType::Drums, 0);
    let b = doc.add_track(InstrumentType::Drums, 0);
    assert_eq!(doc.get_active_drum_track_id(), Some(a.id.clone()));
    let kits = doc.kit_version();
    doc.drain_events();
    doc.set_active_drum_track(&b.id);
    assert_eq!(doc.drain_events(), vec![DocEvent::SyncDrumSink]);
    assert!(doc.kit_version() > kits);
    doc.remove_track(&b.id);
    assert_eq!(doc.get_active_drum_track_id(), Some(a.id.clone()));
    doc.remove_track(&a.id);
    assert_eq!(doc.get_active_drum_track_id(), None);
}

#[test]
fn undo_rebuilds_the_track_audio_and_resyncs_the_sink() {
    let mut doc = counter_doc();
    let id = doc.ensure_drum_track();
    doc.update_pad(&id, 0, &patch(json!({ "gainDb": -3 })));
    doc.drain_events();
    doc.perform_undo();
    let ev = doc.drain_events();
    assert!(ev.contains(&DocEvent::RebuildAll) && ev.contains(&DocEvent::SyncDrumSink));
}

// --- kit edits ---

#[test]
fn update_pad_and_set_pad_sample_are_undoable() {
    let mut doc = counter_doc();
    let id = doc.ensure_drum_track();
    let kits = doc.kit_version();
    doc.drain_events();
    doc.update_pad(&id, 2, &patch(json!({ "gainDb": -12, "pitch": 5 })));
    let p = &doc.get_kit(&id).pads[2];
    assert_eq!((p.gain_db, p.pitch), (-12.0, 5.0));
    assert_eq!(doc.drain_events(), vec![DocEvent::KitApplied(id.clone())]);
    assert!(doc.kit_version() > kits);
    assert_eq!(label(&doc), Some("Edit Pad"));

    doc.update_pad(&id, 4, &patch(json!({ "startSec": 0.2, "endSec": 0.4 })));
    let clap2 = PadSample {
        name: "Clap 2".into(),
        source: PadSampleSource::File {
            path: "/s/clap2.wav".into(),
            sample_id: None,
        },
    };
    doc.set_pad_sample(&id, 4, Some(&clap2));
    let p = doc.get_kit(&id).pads[4].clone();
    assert_eq!(
        (p.sample.unwrap().name.as_str(), p.start_sec, p.end_sec),
        ("Clap 2", 0.0, None)
    );

    doc.perform_undo();
    let p = doc.get_kit(&id).pads[4].clone();
    assert_eq!(
        (p.start_sec, p.end_sec, p.sample.unwrap().name.as_str()),
        (0.2, Some(0.4), "Clap")
    );
    doc.perform_undo();
    doc.perform_undo();
    let p = &doc.get_kit(&id).pads[2];
    assert_eq!((p.gain_db, p.pitch), (0.0, 0.0));
}

#[test]
fn the_same_sample_source_keeps_the_trim() {
    let mut doc = counter_doc();
    let id = doc.ensure_drum_track();
    let a = PadSample {
        name: "A".into(),
        source: PadSampleSource::File {
            path: "/a.wav".into(),
            sample_id: None,
        },
    };
    doc.set_pad_sample(&id, 1, Some(&a));
    doc.update_pad(
        &id,
        1,
        &patch(json!({ "startSec": 0.1, "endSec": 0.3, "reverse": true })),
    );
    let renamed = PadSample {
        name: "A2".into(),
        source: PadSampleSource::File {
            path: "/a.wav".into(),
            sample_id: Some("x".into()),
        },
    };
    doc.set_pad_sample(&id, 1, Some(&renamed));
    let p = doc.get_kit(&id).pads[1].clone();
    assert_eq!((p.start_sec, p.end_sec, p.reverse), (0.1, Some(0.3), true));
    doc.set_pad_sample(&id, 1, None);
    assert_eq!(label(&doc), Some("Clear Pad"));
    let p = doc.get_kit(&id).pads[1].clone();
    assert_eq!((p.sample, p.start_sec, p.reverse), (None, 0.0, false));
}

#[test]
fn a_knob_drag_inside_an_undo_group_is_one_undo_step() {
    let mut doc = counter_doc();
    let id = doc.ensure_drum_track();
    let start = doc.undo_manager().undo_len();
    doc.begin_undo_group("Pad Pitch");
    for pitch in [1, 2, 3, 4] {
        doc.update_pad(&id, 0, &patch(json!({ "pitch": pitch })));
    }
    doc.end_undo_group();
    assert_eq!(doc.undo_manager().undo_len(), start + 1);
    assert_eq!(label(&doc), Some("Pad Pitch"));
    doc.perform_undo();
    assert_eq!(doc.get_kit(&id).pads[0].pitch, 0.0);
}

#[test]
fn set_kit_replaces_the_whole_kit_validated() {
    let mut doc = counter_doc();
    let id = doc.ensure_drum_track();
    doc.set_kit(
        &id,
        &KitState {
            id: "k".into(),
            name: "Mine".into(),
            pads: vec![],
            extra: Default::default(),
        },
        KitSelectionSource::User,
    );
    let kit = doc.get_kit(&id);
    assert_eq!(
        (kit.id.as_str(), kit.name.as_str(), kit.pads.len()),
        ("k", "Mine", 16)
    );
    doc.perform_undo();
    assert_eq!(doc.get_kit(&id).name, "Factory");
}

// --- step pattern editing ---

#[test]
fn toggle_step_adds_and_removes_notes_at_the_step_position() {
    let mut doc = counter_doc();
    let id = doc.ensure_drum_track();
    doc.set_loop_settings(&bars(1));
    assert_eq!(
        doc.get_loop_markers(),
        Some(LoopMarkers {
            start_beat: 0.0,
            end_beat: 4.0
        })
    );
    doc.drain_events();
    doc.toggle_step(&id, 1, 4, 0.25, None);
    assert_eq!(notes_of(&doc, &id), vec![(1, 1.0, 0.8)]);
    let r = doc.get_pattern_region(&id).unwrap();
    assert_eq!((r.start_beat, r.length_beats), (0.0, 4.0));
    assert_eq!(label(&doc), Some("Toggle Step"));
    assert!(
        doc.drain_events()
            .contains(&DocEvent::RescheduleTrack(id.clone()))
    );

    doc.toggle_step(&id, 1, 4, 0.25, None);
    assert_eq!(notes_of(&doc, &id), vec![]);
    doc.perform_undo();
    assert_eq!(notes_of(&doc, &id), vec![(1, 1.0, 0.8)]);
    doc.toggle_step(&id, 1, 16, 0.25, None);
    assert_eq!(
        notes_of(&doc, &id).len(),
        1,
        "steps past the loop are ignored"
    );
}

#[test]
fn stores_steps_on_the_straight_grid_and_finds_them_again() {
    let mut doc = counter_doc();
    let id = doc.ensure_drum_track();
    doc.set_loop_settings(&LoopSettingsPatch {
        swing: Some(0.5),
        ..bars(1)
    });
    doc.toggle_step(&id, 2, 1, 0.25, Some(0.6));
    assert_eq!(notes_of(&doc, &id), vec![(2, 0.25, 0.6)]);
    doc.set_loop_settings(&LoopSettingsPatch {
        swing: Some(0.0),
        ..Default::default()
    });
    doc.set_step_velocity(&id, 2, 1, 0.25, 0.4);
    assert_eq!(notes_of(&doc, &id)[0].2, 0.4);
    doc.set_loop_settings(&LoopSettingsPatch {
        swing: Some(0.5),
        ..Default::default()
    });
    doc.set_step_velocity(&id, 2, 1, 0.25, 0.3);
    assert_eq!(notes_of(&doc, &id)[0].2, 0.3);
    doc.toggle_step(&id, 2, 1, 0.25, None);
    assert_eq!(notes_of(&doc, &id), vec![]);
}

/// Review Sol-5: the TS `noteToPad(36.5)` is 0.5 (non-null), so pattern membership (and thus
/// `clearPattern`) includes fractional notes in the pad range; real pad indexing does not.
#[test]
fn clear_pattern_removes_fractional_pad_range_notes_like_the_ts() {
    let mut doc = counter_doc();
    let id = doc.ensure_drum_track();
    doc.set_loop_settings(&bars(1));
    doc.toggle_step(&id, 0, 0, 0.25, None);
    let region = doc.get_pattern_region(&id).unwrap().clone();
    let mut notes = region.notes.clone();
    for (note, start) in [(36.5, 0.5), (51.9, 1.0), (52.0, 1.5), (35.5, 2.0)] {
        notes.push(RegionNote {
            note,
            velocity: 0.8,
            start_beat: start,
            duration_beats: 0.25,
            is_drum: true,
        });
    }
    doc.update_region_notes(&id, &region.id, notes);
    assert!(crate::types::is_pad_note_range(36.5) && crate::types::note_to_pad(36.5).is_none());
    doc.clear_pattern(&id, None);
    let left: Vec<f64> = doc
        .get_pattern_region(&id)
        .unwrap()
        .notes
        .iter()
        .map(|n| n.note)
        .collect();
    assert_eq!(left, vec![52.0, 35.5]);
}

#[test]
fn set_step_velocity_is_a_no_op_on_empty_steps_clear_pattern_clears_one_pad_or_all() {
    let mut doc = counter_doc();
    let id = doc.ensure_drum_track();
    doc.set_loop_settings(&bars(1));
    doc.toggle_step(&id, 0, 0, 0.25, None);
    doc.toggle_step(&id, 0, 8, 0.25, None);
    doc.toggle_step(&id, 3, 2, 0.25, None);
    let n = doc.undo_manager().undo_len();
    doc.set_step_velocity(&id, 0, 1, 0.25, 1.0);
    assert_eq!(doc.undo_manager().undo_len(), n);
    doc.clear_pattern(&id, Some(0));
    assert_eq!(notes_of(&doc, &id), vec![(3, 0.5, 0.8)]);
    doc.clear_pattern(&id, None);
    assert_eq!(notes_of(&doc, &id), vec![]);
    doc.perform_undo();
    doc.perform_undo();
    assert_eq!(notes_of(&doc, &id).len(), 3);
}

#[test]
fn the_pattern_follows_the_loop_start() {
    let mut doc = counter_doc();
    let id = doc.ensure_drum_track();
    doc.set_loop_markers(8.0, 12.0);
    doc.set_loop_settings(&bars(1));
    assert_eq!(
        doc.get_loop_markers(),
        Some(LoopMarkers {
            start_beat: 8.0,
            end_beat: 12.0
        })
    );
    doc.set_loop_settings(&bars(2));
    assert_eq!(
        doc.get_loop_markers(),
        Some(LoopMarkers {
            start_beat: 8.0,
            end_beat: 16.0
        })
    );
    doc.toggle_step(&id, 0, 0, 0.25, None);
    let r = doc.get_pattern_region(&id).unwrap();
    assert_eq!((r.start_beat, r.length_beats), (8.0, 8.0));
}

#[test]
fn only_a_change_of_bars_rewrites_the_loop_markers() {
    let mut doc = counter_doc();
    doc.ensure_drum_track();
    doc.set_loop_markers(4.0, 7.0);
    doc.drain_events();
    doc.set_loop_settings(&LoopSettingsPatch {
        swing: Some(0.4),
        quantize: Some(QuantizeGrid::Q8),
        metronome: Some(false),
        overdub: Some(false),
        count_in_bars: Some(2),
        ..Default::default()
    });
    assert_eq!(
        doc.get_loop_markers(),
        Some(LoopMarkers {
            start_beat: 4.0,
            end_beat: 7.0
        })
    );
    let ev = doc.drain_events();
    assert!(ev.contains(&DocEvent::SwingChanged) && !ev.contains(&DocEvent::LoopRangeChanged));
    doc.set_loop_settings(&bars(4));
    assert_eq!(
        doc.get_loop_markers(),
        Some(LoopMarkers {
            start_beat: 4.0,
            end_beat: 20.0
        })
    );
}

#[test]
fn a_note_in_the_last_half_step_is_found_on_the_last_step() {
    let mut doc = counter_doc();
    let id = doc.ensure_drum_track();
    doc.set_loop_settings(&bars(1));
    let region = doc.ensure_pattern_region(&id);
    let hit = RegionNote {
        note: f64::from(pad_to_note(3)),
        velocity: 0.8,
        start_beat: 3.9,
        duration_beats: 0.1,
        is_drum: true,
    };
    doc.edit_region_notes(&id, &region.id, vec![hit], None, None);
    doc.toggle_step(&id, 3, 15, 0.25, None);
    assert_eq!(notes_of(&doc, &id), vec![]);
}

// --- GM migration on load ---

#[test]
fn restore_from_snapshot_migrates_kitless_drum_tracks() {
    let mut doc = counter_doc();
    doc.restore_from_snapshot(&legacy_arrangement());
    let track = &doc.get_tracks()[0];
    assert_eq!(track.instrument.kit.as_ref().unwrap().pads.len(), 16);
    let notes: Vec<f64> = track.regions[0].notes.iter().map(|n| n.note).collect();
    assert_eq!(notes, vec![36.0, 37.0, 38.0, 39.0]);
    assert!(track.regions[0].notes.iter().all(|n| n.velocity <= 1.0));
    assert_eq!(doc.get_active_drum_track_id().as_deref(), Some("old-drums"));
    let snap = doc.get_arrangement().clone();
    doc.restore_from_snapshot(&snap);
    let notes: Vec<f64> = doc.get_tracks()[0].regions[0]
        .notes
        .iter()
        .map(|n| n.note)
        .collect();
    assert_eq!(notes, vec![36.0, 37.0, 38.0, 39.0]);
}

#[test]
fn templates_load_onto_the_factory_kit_with_remapped_beat_state() {
    let mut doc = counter_doc();
    let (_, hip_hop) = templates()
        .into_iter()
        .find(|(id, _)| id.contains("hip"))
        .unwrap();
    doc.load_from_template(&hip_hop).unwrap();
    let drums = doc
        .get_tracks()
        .iter()
        .find(|t| t.instrument.kind == InstrumentType::Drums)
        .unwrap()
        .clone();
    assert_eq!(doc.get_active_drum_track_id(), Some(drums.id.clone()));
    assert_eq!(drums.instrument.kit.as_ref().unwrap().id, "factory");
    let mut pads: Vec<i64> = drums
        .regions
        .iter()
        .flat_map(|r| r.notes.iter().map(|n| n.note as i64 - 36))
        .collect();
    pads.sort();
    pads.dedup();
    assert_eq!(pads, vec![0, 1, 2, 3, 4]);
    assert!(
        !doc.drain_events()
            .iter()
            .any(|e| matches!(e, DocEvent::MigrationWarning(_)))
    );
}

// --- loop recording (data half) ---

#[test]
fn a_take_creates_the_drum_track_and_pattern_and_loops_the_pattern_transiently() {
    let mut doc = counter_doc();
    doc.set_loop_settings(&no_count_in(2));
    let start = doc.begin_loop_take().unwrap();
    let id = doc.get_active_drum_track_id().unwrap();
    assert_eq!(start.track_id, id);
    let r = doc.get_pattern_region(&id).unwrap();
    assert_eq!((r.start_beat, r.length_beats), (0.0, 8.0));
    assert!(!doc.is_loop_enabled());
    assert_eq!(
        doc.get_active_loop_range(false),
        Some(LoopMarkers {
            start_beat: 0.0,
            end_beat: 8.0
        })
    );
    doc.stop_loop_take();
    assert!(!doc.is_loop_recording());
}

#[test]
fn hits_on_the_same_step_and_pad_dedupe_louder_wins() {
    let mut doc = counter_doc();
    doc.set_loop_settings(&no_count_in(1));
    doc.begin_loop_take().unwrap();
    let id = doc.get_active_drum_track_id().unwrap();
    // 1.02 → 1, 4.98 → next pass, step 1, 1.0 → 1 (quantized by the recorder)
    for v in [0.4, 0.9, 0.6] {
        doc.loop_take_record_hit(0, v, 1.0, 0.25).unwrap();
    }
    assert_eq!(notes_of(&doc, &id), vec![(0, 1.0, 0.9)]);
}

#[test]
fn overdub_keeps_the_existing_pattern() {
    let mut doc = counter_doc();
    let id = doc.ensure_drum_track();
    doc.set_loop_settings(&LoopSettingsPatch {
        overdub: Some(true),
        ..no_count_in(1)
    });
    doc.toggle_step(&id, 5, 0, 0.25, None);
    doc.begin_loop_take().unwrap();
    let hit = doc.loop_take_record_hit(1, 0.8, 2.0, 0.25).unwrap();
    assert!(hit.added && !hit.cleared && !hit.grown);
    let mut pads: Vec<i64> = notes_of(&doc, &id).iter().map(|n| n.0).collect();
    pads.sort();
    assert_eq!(pads, vec![1, 5]);
}

#[test]
fn replace_clears_the_pattern_on_the_first_hit_only() {
    let mut doc = counter_doc();
    let id = doc.ensure_drum_track();
    doc.set_loop_settings(&LoopSettingsPatch {
        overdub: Some(false),
        ..no_count_in(1)
    });
    doc.toggle_step(&id, 5, 0, 0.25, None);
    doc.toggle_step(&id, 6, 4, 0.25, None);
    doc.begin_loop_take().unwrap();
    assert_eq!(notes_of(&doc, &id).len(), 2);
    assert!(doc.loop_take_record_hit(1, 0.8, 2.0, 0.25).unwrap().cleared);
    assert!(!doc.loop_take_record_hit(2, 0.8, 1.0, 0.25).unwrap().cleared);
    assert_eq!(notes_of(&doc, &id), vec![(1, 2.0, 0.8), (2, 1.0, 0.8)]);
}

#[test]
fn the_whole_take_is_one_undo_step() {
    let mut doc = counter_doc();
    let id = doc.ensure_drum_track();
    doc.set_loop_settings(&no_count_in(1));
    doc.toggle_step(&id, 5, 0, 0.25, None);
    doc.begin_loop_take().unwrap();
    let states = doc.state_version();
    let kits = doc.kit_version();
    doc.loop_take_record_hit(1, 0.8, 1.0, 0.25);
    doc.loop_take_record_hit(2, 0.8, 2.0, 0.25);
    // useArrangement/useBeat L-a: one state notification per recorded hit, no kit notification.
    assert_eq!(doc.state_version(), states + 2);
    assert_eq!(doc.kit_version(), kits);
    doc.stop_loop_take();
    assert_eq!(label(&doc), Some("Loop Record"));
    doc.perform_undo();
    assert_eq!(notes_of(&doc, &id), vec![(5, 0.0, 0.8)]);
}

#[test]
fn gate_lengths_update_the_held_note() {
    let mut doc = counter_doc();
    let id = doc.ensure_drum_track();
    doc.update_pad(&id, 2, &patch(json!({ "mode": "gate" })));
    assert!(doc.is_gate_pad(&id, 2));
    assert_eq!(doc.get_kit(&id).pads[2].mode, PadPlayMode::Gate);
    doc.set_loop_settings(&no_count_in(1));
    doc.begin_loop_take().unwrap();
    let hit = doc.loop_take_record_hit(2, 0.8, 3.5, 0.25).unwrap();
    doc.drain_events();
    assert!(doc.loop_take_set_gate_length(&hit.region_id, hit.note.note, hit.note.start_beat, 1.0));
    assert_eq!(
        doc.drain_events(),
        vec![DocEvent::RescheduleTrack(id.clone())]
    );
    assert_eq!(
        doc.get_pattern_region(&id).unwrap().notes[0].duration_beats,
        1.0
    );
    assert!(!doc.loop_take_set_gate_length(&hit.region_id, hit.note.note, 0.0, 1.0));
}

#[test]
fn a_failed_take_start_rolls_back_its_structure_change() {
    let mut doc = counter_doc();
    doc.ensure_drum_track();
    doc.set_loop_settings(&LoopSettingsPatch {
        count_in_bars: Some(1),
        ..bars(1)
    });
    let markers = doc.get_loop_markers();
    let before = doc.get_arrangement().clone();
    assert!(doc.begin_loop_take().unwrap().structure_changed);
    doc.abort_loop_take_start();
    assert!(!doc.is_loop_recording());
    assert!(!doc.is_loop_enabled());
    assert_eq!(doc.get_loop_markers(), markers);
    assert_eq!(
        doc.get_arrangement().tracks[0].regions,
        before.tracks[0].regions
    );
}

#[test]
fn the_take_end_hook_runs_before_the_doc_bookkeeping() {
    let mut doc = counter_doc();
    doc.set_loop_settings(&no_count_in(1));
    doc.begin_loop_take().unwrap();
    let hit = doc.loop_take_record_hit(0, 0.8, 0.0, 0.25).unwrap();
    let rid = hit.region_id.clone();
    doc.set_take_end_hook(Some(Box::new(move |d: &mut ArrangementDoc| {
        assert!(
            d.is_loop_recording(),
            "the take is still open for closing gates"
        );
        d.loop_take_set_gate_length(&rid, 36.0, 0.0, 2.0);
    })));
    doc.perform_undo();
    assert!(!doc.is_loop_recording());
    doc.perform_redo();
    let id = doc.get_active_drum_track_id().unwrap();
    assert_eq!(
        doc.get_pattern_region(&id).unwrap().notes[0].duration_beats,
        2.0
    );
}

// --- starter project ---

#[test]
fn reset_to_starter_one_active_drums_track_default_loop_no_history() {
    let mut doc = counter_doc();
    doc.add_track(InstrumentType::Synth, 0);
    doc.reset_to_starter();
    let tracks = doc.get_tracks().to_vec();
    assert_eq!(tracks.len(), 1);
    assert_eq!(
        (tracks[0].name.as_str(), tracks[0].instrument.kind),
        ("Drums", InstrumentType::Drums)
    );
    assert_eq!(doc.get_active_drum_track_id(), Some(tracks[0].id.clone()));
    assert_eq!(doc.get_kit(&tracks[0].id).id, "factory");
    let lp = doc.get_loop_settings();
    assert_eq!(
        (lp.bars, lp.quantize, lp.metronome),
        (2, QuantizeGrid::Q16, true)
    );
    assert_eq!(
        doc.get_loop_markers(),
        Some(LoopMarkers {
            start_beat: 0.0,
            end_beat: 8.0
        })
    );
    assert!(!doc.undo_manager().can_undo());
    assert!(doc.drain_events().contains(&DocEvent::SyncDrumSink));
}

// --- review regressions (undo groups, active track, round 2) ---

#[test]
fn undo_groups_nest_one_entry_with_the_outermost_label() {
    let mut doc = counter_doc();
    let id = doc.ensure_drum_track();
    let start = doc.undo_manager().undo_len();
    doc.begin_undo_group("Paste pad");
    doc.begin_undo_group("Pad Pitch");
    doc.update_pad(&id, 0, &patch(json!({ "pitch": 3 })));
    doc.end_undo_group();
    assert_eq!(doc.undo_manager().undo_len(), start);
    doc.toggle_step(&id, 0, 0, 0.25, None);
    doc.set_step_velocity(&id, 0, 0, 0.25, 0.3);
    doc.end_undo_group();
    assert_eq!(doc.undo_manager().undo_len(), start + 1);
    assert_eq!(label(&doc), Some("Paste pad"));
    doc.end_undo_group();
    assert_eq!(doc.undo_manager().undo_len(), start + 1);
    doc.perform_undo();
    assert_eq!(doc.get_kit(&id).pads[0].pitch, 0.0);
    assert_eq!(notes_of(&doc, &id), vec![]);
}

#[test]
fn an_empty_undo_group_adds_no_entry() {
    let mut doc = counter_doc();
    doc.ensure_drum_track();
    let before = label(&doc).map(str::to_owned);
    let n = doc.undo_manager().undo_len();
    doc.begin_undo_group("Nothing");
    doc.end_undo_group();
    assert_eq!(doc.undo_manager().undo_len(), n);
    assert_eq!(label(&doc).map(str::to_owned), before);
}

#[test]
fn redo_twice_after_undo_twice_restores_all_changes() {
    let mut doc = counter_doc();
    let id = doc.ensure_drum_track();
    for s in [0, 4, 8] {
        doc.toggle_step(&id, 0, s, 0.25, None);
    }
    doc.perform_undo();
    doc.perform_undo();
    assert_eq!(notes_of(&doc, &id).len(), 1);
    doc.perform_redo();
    assert_eq!(notes_of(&doc, &id).len(), 2);
    doc.perform_redo();
    assert_eq!(notes_of(&doc, &id).len(), 3);
    doc.perform_undo();
    assert_eq!(notes_of(&doc, &id).len(), 2);
}

#[test]
fn arming_a_drum_track_makes_it_the_active_drum_track() {
    let mut doc = counter_doc();
    let a = doc.add_track(InstrumentType::Drums, 0);
    let b = doc.add_track(InstrumentType::Drums, 0);
    assert_eq!(doc.get_active_drum_track_id(), Some(a.id));
    doc.arm_track_for_recording(Some(&b.id));
    assert_eq!(doc.get_active_drum_track_id(), Some(b.id.clone()));
    let synth = doc.add_track(InstrumentType::Synth, 0);
    doc.arm_track_for_recording(Some(&synth.id));
    assert_eq!(doc.get_active_drum_track_id(), Some(b.id));
    assert_eq!(doc.get_armed_track_id(), Some(synth.id.as_str()));
}

#[test]
fn a_template_without_loop_markers_gets_the_pattern_range_and_play_pattern_loops_it() {
    let mut doc = counter_doc();
    let mut arr = legacy_arrangement();
    arr.tracks[0].regions[0].length_beats = 16.0;
    arr.tracks[0].regions[0].notes = vec![
        RegionNote {
            note: 36.0,
            start_beat: 2.5,
            duration_beats: 0.25,
            velocity: 0.8,
            is_drum: true,
        },
        RegionNote {
            note: 36.0,
            start_beat: 10.5,
            duration_beats: 0.25,
            velocity: 0.8,
            is_drum: true,
        },
    ];
    doc.load_from_template(&arr).unwrap();
    assert_eq!(
        doc.get_loop_markers(),
        Some(LoopMarkers {
            start_beat: 0.0,
            end_beat: 8.0
        })
    );
    assert_eq!(
        doc.prepare_play_pattern(),
        LoopMarkers {
            start_beat: 0.0,
            end_beat: 8.0
        }
    );
    assert_eq!(
        doc.get_active_loop_range(true),
        Some(LoopMarkers {
            start_beat: 0.0,
            end_beat: 8.0
        })
    );

    doc.set_loop_markers(16.0, 20.0);
    doc.set_loop_settings(&bars(1));
    doc.set_loop_markers(16.0, 19.0);
    assert_eq!(
        doc.prepare_play_pattern(),
        LoopMarkers {
            start_beat: 16.0,
            end_beat: 20.0
        }
    );
    assert_eq!(
        doc.get_loop_markers(),
        Some(LoopMarkers {
            start_beat: 16.0,
            end_beat: 19.0
        })
    );
}

#[test]
fn loading_saved_data_without_markers_migrates_them_too() {
    let mut doc = counter_doc();
    let raw = serde_json::to_string(&legacy_arrangement()).unwrap();
    assert!(matches!(
        doc.load_from_local_storage(Some(&raw)),
        Ok(Some(_))
    ));
    assert_eq!(
        doc.get_loop_markers(),
        Some(LoopMarkers {
            start_beat: 0.0,
            end_beat: 8.0
        })
    );
}

#[test]
fn undo_during_the_count_in_of_an_empty_take_ends_it_and_keeps_redo() {
    let mut doc = counter_doc();
    let id = doc.ensure_drum_track();
    doc.toggle_step(&id, 0, 0, 0.25, None);
    doc.set_loop_settings(&LoopSettingsPatch {
        count_in_bars: Some(1),
        metronome: Some(false),
        ..bars(4)
    });
    assert!(doc.begin_loop_take().unwrap().structure_changed);
    doc.perform_undo();
    assert!(!doc.is_loop_recording());
    assert_eq!(notes_of(&doc, &id), vec![]);
    assert!(doc.undo_manager().can_redo());
    assert_ne!(label(&doc), Some("Loop Record"));
    doc.perform_redo();
    assert_eq!(notes_of(&doc, &id), vec![(0, 0.0, 0.8)]);
}

#[test]
fn undo_mid_take_with_hits_undoes_the_take_redo_brings_it_back() {
    let mut doc = counter_doc();
    let id = doc.ensure_drum_track();
    doc.set_loop_settings(&no_count_in(1));
    doc.toggle_step(&id, 5, 0, 0.25, None);
    doc.begin_loop_take().unwrap();
    doc.loop_take_record_hit(1, 0.8, 1.0, 0.25);
    doc.loop_take_record_hit(2, 0.8, 2.0, 0.25);
    doc.perform_undo();
    assert!(!doc.is_loop_recording());
    assert_eq!(notes_of(&doc, &id), vec![(5, 0.0, 0.8)]);
    assert_eq!(label(&doc), Some("Toggle Step"));
    doc.perform_redo();
    assert_eq!(notes_of(&doc, &id).len(), 3);
}

#[test]
fn undo_inside_an_open_gesture_undoes_the_gesture_only() {
    let mut doc = counter_doc();
    let id = doc.ensure_drum_track();
    doc.toggle_step(&id, 0, 0, 0.25, None);
    doc.toggle_step(&id, 0, 4, 0.25, None);
    doc.begin_undo_group("Pad Cutoff");
    doc.update_pad(&id, 0, &patch(json!({ "filterCutoff": 2000 })));
    doc.update_pad(&id, 0, &patch(json!({ "filterCutoff": 1500 })));
    doc.perform_undo();
    assert_eq!(doc.get_kit(&id).pads[0].filter_cutoff, 20000.0);
    assert_eq!(notes_of(&doc, &id).len(), 2);
    doc.end_undo_group();
    assert_eq!(label(&doc), Some("Toggle Step"));
    assert!(doc.undo_manager().can_redo());
    doc.perform_redo();
    assert_eq!(doc.get_kit(&id).pads[0].filter_cutoff, 1500.0);
    assert_eq!(notes_of(&doc, &id).len(), 2);
}

#[test]
fn a_redo_inside_an_open_gesture_that_changed_something_is_a_no_op() {
    let mut doc = counter_doc();
    let id = doc.ensure_drum_track();
    doc.toggle_step(&id, 0, 0, 0.25, None);
    doc.perform_undo();
    doc.begin_undo_group("Pad Gain");
    doc.update_pad(&id, 1, &patch(json!({ "gainDb": -6 })));
    doc.perform_redo();
    assert_eq!(notes_of(&doc, &id), vec![]);
    doc.update_pad(&id, 1, &patch(json!({ "gainDb": -9 })));
    doc.end_undo_group();
    assert_eq!(label(&doc), Some("Pad Gain"));
    doc.perform_undo();
    assert_eq!(doc.get_kit(&id).pads[1].gain_db, -6.0);
    doc.perform_undo();
    assert_eq!(doc.get_kit(&id).pads[1].gain_db, 0.0);
}

#[test]
fn restoring_a_snapshot_never_writes_history() {
    let mut doc = counter_doc();
    let id = doc.ensure_drum_track();
    doc.toggle_step(&id, 0, 0, 0.25, None);
    doc.set_loop_settings(&no_count_in(4));
    let snap = doc.get_arrangement().clone();
    assert!(doc.begin_loop_take().unwrap().structure_changed);
    let depth = doc.undo_manager().undo_len();
    doc.restore_from_snapshot(&snap);
    assert_eq!(doc.undo_manager().undo_len(), depth);
    assert!(!doc.is_loop_recording());
}

#[test]
fn new_drum_tracks_are_numbered() {
    let mut doc = counter_doc();
    assert_eq!(doc.add_track(InstrumentType::Drums, 0).name, "Drums");
    assert_eq!(doc.add_track(InstrumentType::Drums, 0).name, "Drums 2");
    let synth = doc.add_track(InstrumentType::Synth, 0);
    let drums = crate::types::TrackInstrument {
        kind: InstrumentType::Drums,
        preset_index: 0,
        synth_overrides: None,
        kit: None,
        extra: Default::default(),
    };
    doc.set_track_instrument(&synth.id, &drums);
    assert_eq!(doc.get_tracks()[2].name, "Drums 3");
}

#[test]
fn beat_play_loops_the_pattern_transiently_markers_and_song_loop_kept() {
    let mut doc = counter_doc();
    doc.ensure_drum_track();
    doc.set_loop_settings(&bars(1));
    doc.set_loop_markers(4.0, 14.0);
    doc.clear_history();
    assert_eq!(
        doc.prepare_play_pattern(),
        LoopMarkers {
            start_beat: 4.0,
            end_beat: 8.0
        }
    );
    assert_eq!(
        doc.get_active_loop_range(true),
        Some(LoopMarkers {
            start_beat: 4.0,
            end_beat: 8.0
        })
    );
    assert_eq!(
        doc.get_loop_markers(),
        Some(LoopMarkers {
            start_beat: 4.0,
            end_beat: 14.0
        })
    );
    assert!(!doc.is_loop_enabled());
    assert_eq!(doc.get_active_loop_range(false), None);
    assert!(!doc.undo_manager().can_undo());
    doc.set_loop_enabled(true);
    assert_eq!(
        doc.get_active_loop_range(true),
        Some(LoopMarkers {
            start_beat: 4.0,
            end_beat: 8.0
        })
    );
    assert_eq!(
        doc.get_active_loop_range(false),
        Some(LoopMarkers {
            start_beat: 4.0,
            end_beat: 14.0
        })
    );
    doc.set_loop_markers(4.0, 8.0);
    assert!(doc.undo_manager().can_undo());
}

#[test]
fn loop_recording_loops_the_pattern_without_rewriting_the_markers() {
    let mut doc = counter_doc();
    doc.ensure_drum_track();
    doc.set_loop_settings(&no_count_in(1));
    doc.set_loop_markers(0.0, 12.0);
    doc.begin_loop_take().unwrap();
    assert_eq!(
        doc.get_active_loop_range(false),
        Some(LoopMarkers {
            start_beat: 0.0,
            end_beat: 4.0
        })
    );
    doc.stop_loop_take();
    assert_eq!(
        doc.get_loop_markers(),
        Some(LoopMarkers {
            start_beat: 0.0,
            end_beat: 12.0
        })
    );
    assert!(!doc.is_loop_enabled());
}

// --- useBeat.test (engine-facing) ---

#[test]
fn assign_file_creates_the_drum_track_on_first_edit() {
    let mut doc = counter_doc();
    let id = doc.ensure_drum_track();
    let sample = PadSample {
        name: "Break".into(),
        source: PadSampleSource::File {
            path: "/s/Break.wav".into(),
            sample_id: None,
        },
    };
    doc.set_pad_sample(&id, 2, Some(&sample));
    assert_eq!(doc.get_active_drum_track_id(), Some(id.clone()));
    assert_eq!(doc.get_kit(&id).pads[2].sample, Some(sample));
}

#[test]
fn chop_to_pads_is_one_undo_step() {
    let mut doc = counter_doc();
    let id = doc.ensure_drum_track();
    let sample = PadSample {
        name: "Loop".into(),
        source: PadSampleSource::File {
            path: "/s/Loop.wav".into(),
            sample_id: None,
        },
    };
    doc.set_pad_sample(&id, 4, Some(&sample));
    let n = doc.undo_manager().undo_len();
    doc.begin_undo_group("Chop to pads");
    for (pad, (s, e)) in [(4usize, (0.0, 0.5)), (5, (0.5, 1.0)), (6, (1.0, 2.0))] {
        if pad != 4 {
            let named = PadSample {
                name: format!("Loop {}", pad - 4 + 1),
                ..sample.clone()
            };
            doc.set_pad_sample(&id, pad, Some(&named));
        }
        doc.update_pad(
            &id,
            pad,
            &patch(json!({ "startSec": s, "endSec": e, "reverse": false })),
        );
    }
    doc.end_undo_group();
    assert_eq!(doc.undo_manager().undo_len(), n + 1);
    let pads = doc.get_kit(&id).pads;
    assert_eq!(
        (
            pads[4].start_sec,
            pads[4].end_sec,
            pads[4].sample.as_ref().unwrap().name.as_str()
        ),
        (0.0, Some(0.5), "Loop")
    );
    assert_eq!(
        (
            pads[5].start_sec,
            pads[5].end_sec,
            pads[5].sample.as_ref().unwrap().name.as_str()
        ),
        (0.5, Some(1.0), "Loop 2")
    );
    assert_eq!(
        (
            pads[6].start_sec,
            pads[6].end_sec,
            pads[6].sample.as_ref().unwrap().name.as_str()
        ),
        (1.0, Some(2.0), "Loop 3")
    );
}

#[test]
fn copies_and_pastes_a_pad_with_its_settings() {
    let mut doc = counter_doc();
    let id = doc.ensure_drum_track();
    doc.update_pad(&id, 0, &patch(json!({ "pitch": 7, "gainDb": -3 })));
    let copied = doc.get_kit(&id).pads[0].clone();
    let mut rest = serde_json::to_value(&copied).unwrap();
    rest.as_object_mut().unwrap().remove("sample");
    doc.begin_undo_group("Paste pad");
    doc.set_pad_sample(&id, 9, copied.sample.as_ref());
    doc.update_pad(&id, 9, rest.as_object().unwrap());
    doc.end_undo_group();
    let p = &doc.get_kit(&id).pads[9];
    assert_eq!((p.pitch, p.gain_db, &p.sample), (7.0, -3.0, &copied.sample));
    assert_eq!(label(&doc), Some("Paste pad"));
}

#[test]
fn selects_pads_and_toggles_steps_in_the_pattern() {
    let mut doc = counter_doc();
    doc.set_selected_pad(7);
    assert_eq!(doc.get_selected_pad(), 7);
    let n = doc.state_version();
    doc.set_selected_pad(7);
    doc.set_selected_pad(16);
    assert_eq!(doc.state_version(), n, "same / invalid pad: no change");
    let id = doc.ensure_drum_track();
    doc.toggle_step(&id, 7, 3, 0.25, None);
    assert!(
        doc.get_pattern_region(&id)
            .unwrap()
            .notes
            .iter()
            .any(|n| n.note == 43.0)
    );
    let v = doc.state_version();
    doc.toggle_step(&id, 7, 3, 0.25, None);
    assert_ne!(doc.state_version(), v);
}

#[test]
fn beat_play_on_a_project_whose_loop_is_longer_than_the_pattern() {
    let mut doc = counter_doc();
    doc.reset_to_starter();
    doc.set_loop_settings(&bars(2));
    doc.set_loop_markers(0.0, 16.0);
    doc.prepare_play_pattern();
    assert_eq!(
        doc.get_loop_markers(),
        Some(LoopMarkers {
            start_beat: 0.0,
            end_beat: 16.0
        })
    );
    assert_eq!(
        doc.get_active_loop_range(true),
        Some(LoopMarkers {
            start_beat: 0.0,
            end_beat: 8.0
        })
    );
    assert_eq!(doc.get_active_loop_range(false), None);
    assert!(!doc.is_loop_enabled());
}

#[test]
fn loop_pattern_sets_the_loop_to_the_pattern_range_as_one_undo_step() {
    let mut doc = counter_doc();
    doc.reset_to_starter();
    doc.set_loop_settings(&bars(2));
    doc.set_loop_markers(0.0, 16.0);
    let n = doc.undo_manager().undo_len();
    let start = doc.get_pattern_start();
    let length = doc.get_pattern_length();
    doc.begin_undo_group("Loop pattern");
    if !doc.is_loop_enabled() {
        doc.set_loop_enabled(true);
    }
    doc.set_loop_markers(start, start + length);
    doc.end_undo_group();
    assert_eq!(doc.undo_manager().undo_len(), n + 1);
    assert_eq!(label(&doc), Some("Loop pattern"));
    assert_eq!(
        doc.get_loop_markers(),
        Some(LoopMarkers {
            start_beat: 0.0,
            end_beat: 8.0
        })
    );
    assert!(doc.is_loop_enabled());
}

#[test]
fn creates_a_new_drum_track_makes_it_active_and_switches_back() {
    let mut doc = counter_doc();
    let first = doc.ensure_drum_track();
    doc.update_pad(&first, 0, &patch(json!({ "pitch": 5 })));
    let second = doc.add_track(InstrumentType::Drums, 0).id;
    doc.set_active_drum_track(&second);
    assert_ne!(second, first);
    assert_eq!(doc.get_active_drum_track_id(), Some(second.clone()));
    assert_ne!(doc.get_kit(&second).pads[0].pitch, 5.0);
    doc.set_active_drum_track(&first);
    assert_eq!(doc.get_active_drum_track_id(), Some(first.clone()));
    assert_eq!(doc.get_kit(&first).pads[0].pitch, 5.0);
}

// --- pad sound shaping (Rust-only) ---

#[test]
fn pad_shaping_defaults_are_neutral_and_omitted_from_the_json() {
    use crate::types::{PadSettings, PadShape};
    let d = PadSettings::default();
    assert!(d.shape.is_neutral());
    let v = serde_json::to_value(&d).unwrap();
    for k in ["ampAttack", "ampSustain", "filterType", "pitchEnv", "drive"] {
        assert!(v.get(k).is_none(), "{k} serialized while neutral");
    }
    // An old pad (no shaping keys) loads with the defaults, through serde and normalizePad.
    let old = json!({ "gainDb": -3, "decay": 0.2, "filterCutoff": 900 });
    let p: PadSettings = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(p.shape, PadShape::default());
    assert!(p.extra.is_empty());
    assert_eq!(
        crate::kit::normalize_pad(&old, None).shape,
        PadShape::default()
    );
}

#[test]
fn pad_shaping_round_trips_and_is_clamped() {
    use crate::types::{PadFilterType, PadSettings};
    let raw = json!({
        "ampAttack": 0.01, "ampHold": 0.02, "ampDecay": 0.4, "ampSustain": 0.3,
        "filterType": "hp", "filterResonance": 0.6, "filterEnv": 2.5, "filterEnvDecay": 0.2,
        "pitchEnv": 12, "pitchEnvDecay": 0.05, "drive": 0.4
    });
    let p = crate::kit::normalize_pad(&raw, None);
    assert_eq!(p.shape.filter_type, PadFilterType::Hp);
    assert_eq!((p.shape.amp_sustain, p.shape.pitch_env), (0.3, 12.0));
    let back: PadSettings = serde_json::from_value(serde_json::to_value(&p).unwrap()).unwrap();
    assert_eq!(back.shape, p.shape);
    assert!(
        back.extra.is_empty(),
        "shaping keys must not land in `extra`"
    );
    let wild = json!({ "ampAttack": 99, "ampSustain": -1, "filterType": "notch",
        "filterEnv": -40, "pitchEnv": 100, "drive": "x" });
    let s = crate::kit::normalize_pad(&wild, None).shape;
    assert_eq!(
        (
            s.amp_attack,
            s.amp_sustain,
            s.filter_type,
            s.filter_env,
            s.pitch_env,
            s.drive
        ),
        (2.0, 0.0, PadFilterType::Lp, -6.0, 48.0, 0.0)
    );
}

#[test]
fn pad_shaping_edits_are_undoable() {
    let mut doc = counter_doc();
    let id = doc.ensure_drum_track();
    doc.update_pad(&id, 1, &patch(json!({ "ampSustain": 0.2, "drive": 0.5 })));
    let s = doc.get_kit(&id).pads[1].shape;
    assert_eq!((s.amp_sustain, s.drive), (0.2, 0.5));
    assert_eq!(label(&doc), Some("Edit Pad"));
    doc.perform_undo();
    assert!(doc.get_kit(&id).pads[1].shape.is_neutral());
    doc.perform_redo();
    assert_eq!(doc.get_kit(&id).pads[1].shape.drive, 0.5);
}
