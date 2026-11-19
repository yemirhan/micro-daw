//! Unit tests for `ArrangementDoc`: ports of the PURE (non-transport, non-audio) cases of
//! `src/services/ArrangementEngine.pianoRoll.test.ts`, `ArrangementEngine.round5.test.ts`,
//! `src/hooks/useArrangement.test.tsx` (engine-facing assertions) plus Song-side edits. The beat
//! cases (`ArrangementEngine.beat.test.ts`, `src/hooks/useBeat.test.ts`) are in `tests_beat.rs`.
//!
//! SKIPPED here (they need transport / audio; phase 3 ports them against the engine):
//! - pianoRoll.test: "previewNote is a self-releasing one-shot (no stuck notes)"; "previewNote
//!   ignores unknown tracks and clamps silly input"; "reschedules only the edited track while
//!   playing (transport keeps running)" (only its `RescheduleTrack` event is checked here);
//!   "undo while playing keeps every track scheduled and resumes from the same beat" (only the
//!   `RebuildAll` event + notes are checked here); "changing a track instrument while playing
//!   reschedules it" (only `TrackAudioRebuilt` is checked).
//! - reload.test (B3, all audio players): "a reload while playing swaps the new buffer into the
//!   scheduled player"; "a reload while stopped is picked up by the next play"; "a removed pack
//!   keeps the region playing its loaded audio".
//! - round5.test: N1 "cancels the pending transport start even though Tone reports the transport
//!   stopped"; N1 "a stopped transport with no pending start is not stopped again"; N4 "restarts
//!   playback from the new beat"; N4 "keeps pattern playback across the restart"; N4 "a seek
//!   after the start took effect just moves the transport"; N5 sample-manager half of "lists the
//!   audio regions whose pack sample became unavailable and signals the change" (reload, change
//!   signal, revisions; the region listing is ported with an injected predicate).
//! - useArrangement.test (render counting of React slices, playhead frames, metronome engine
//!   state): "a host using actions + transport only does not re-render on recorded hits or
//!   playhead frames"; "the legacy default (everything) still re-renders on hits and frames";
//!   "usePlaybackPosition follows the playhead; transport snapshots keep identity on hits";
//!   "metronome toggle is engine state (no local copy)". The engine-side facts they rely on
//!   (one state notification per recorded hit, no kit notification) are checked in
//!   `tests_beat.rs`.

use serde_json::json;

use super::*;
use crate::types::{
    AudioRegionData, AutomationParameter, InstrumentType, LoopMarkers, Region, RegionNote,
    SampleRef, TrackInstrument,
};

pub(super) fn counter_doc() -> ArrangementDoc {
    let mut n = 0;
    ArrangementDoc::with_id_generator(Box::new(move || {
        n += 1;
        format!("uuid-{n}")
    }))
}

pub(super) fn note(n: f64, start: f64, dur: f64) -> RegionNote {
    RegionNote {
        note: n,
        velocity: 0.8,
        start_beat: start,
        duration_beats: dur,
        is_drum: false,
    }
}

pub(super) fn region(id: &str, start: f64, len: f64, notes: Vec<RegionNote>) -> Region {
    Region {
        id: id.into(),
        start_beat: start,
        length_beats: len,
        notes,
        color: None,
        name: None,
        audio: None,
        extra: Default::default(),
    }
}

fn label(doc: &ArrangementDoc) -> Option<&str> {
    doc.undo_manager().undo_label()
}

// --- pianoRoll.test ---

#[test]
fn edit_region_notes_is_a_single_undo_step_and_can_grow_the_region() {
    let mut doc = counter_doc();
    let track = doc.add_track(InstrumentType::Synth, 0);
    doc.add_region(
        &track.id,
        region("r1", 4.0, 4.0, vec![note(60.0, 0.0, 1.0)]),
    );
    let before = doc.get_tracks()[0].regions[0].notes.clone();
    doc.drain_events();
    doc.edit_region_notes(
        &track.id,
        "r1",
        vec![note(60.0, 0.0, 1.0), note(64.0, 6.0, 2.0)],
        Some("Draw Note"),
        Some(8.0),
    );
    let r = &doc.get_tracks()[0].regions[0];
    assert_eq!(r.notes.len(), 2);
    assert_eq!(r.length_beats, 8.0);
    assert_eq!(label(&doc), Some("Draw Note"));
    // "reschedules only the edited track while playing": the engine rebuilds that track's Part.
    assert_eq!(
        doc.drain_events(),
        vec![DocEvent::RescheduleTrack(track.id.clone())]
    );
    doc.perform_undo();
    let r = &doc.get_tracks()[0].regions[0];
    assert_eq!(r.notes, before);
    assert_eq!(r.length_beats, 4.0);
    assert!(doc.drain_events().contains(&DocEvent::RebuildAll));
}

#[test]
fn edit_region_notes_never_shrinks_the_region() {
    let mut doc = counter_doc();
    let track = doc.add_track(InstrumentType::Synth, 0);
    doc.add_region(&track.id, region("r1", 0.0, 16.0, vec![]));
    doc.edit_region_notes(
        &track.id,
        "r1",
        vec![note(60.0, 0.0, 1.0)],
        Some("Edit"),
        Some(4.0),
    );
    assert_eq!(doc.get_tracks()[0].regions[0].length_beats, 16.0);
    // an unknown region is validated first: no undo entry
    let n = doc.undo_manager().undo_len();
    doc.edit_region_notes(&track.id, "nope", vec![], None, None);
    assert_eq!(doc.undo_manager().undo_len(), n);
}

#[test]
fn changing_a_track_instrument_rebuilds_its_audio() {
    let mut doc = counter_doc();
    let track = doc.add_track(InstrumentType::Synth, 0);
    doc.drain_events();
    doc.set_track_instrument(
        &track.id,
        &TrackInstrument {
            kind: InstrumentType::Synth,
            preset_index: 1,
            synth_overrides: None,
            kit: None,
            extra: Default::default(),
            plugin: None,
        },
    );
    assert!(
        doc.drain_events()
            .contains(&DocEvent::TrackAudioRebuilt(track.id.clone()))
    );
    assert_eq!(doc.get_tracks()[0].name, "Saw Lead");
}

#[test]
fn normalises_legacy_velocities_when_loading_a_template_or_snapshot() {
    for (_, tpl) in templates() {
        for t in &tpl.tracks {
            for r in &t.regions {
                assert!(r.notes.iter().all(|n| n.velocity <= 1.0));
            }
        }
    }
    let mut doc = counter_doc();
    let mut legacy = doc.get_arrangement().clone();
    let mut track = doc.add_track(InstrumentType::Synth, 0);
    track.id = "t1".into();
    track.regions = vec![region(
        "r1",
        0.0,
        4.0,
        vec![
            RegionNote {
                velocity: 127.0,
                ..note(60.0, 0.0, 1.0)
            },
            RegionNote {
                velocity: 0.5,
                ..note(62.0, 1.0, 1.0)
            },
        ],
    )];
    legacy.tracks = vec![track];
    doc.load_from_template(&legacy).unwrap();
    let v: Vec<f64> = doc.get_tracks()[0].regions[0]
        .notes
        .iter()
        .map(|n| n.velocity)
        .collect();
    assert_eq!(v, vec![1.0, 0.5]);
}

pub(super) fn templates() -> Vec<(&'static str, Arrangement)> {
    crate::templates_data::TEMPLATE_FILES
        .into_iter()
        .map(|(id, s)| {
            let v: serde_json::Value = serde_json::from_str(s).unwrap();
            (
                id,
                serde_json::from_value(v["arrangement"].clone()).unwrap(),
            )
        })
        .collect()
}

// --- round5.test (N2, N5) ---

#[test]
fn the_session_changes_on_new_template_open_not_on_edits_or_undo() {
    let mut doc = counter_doc();
    doc.reset_to_starter();
    doc.set_bpm(120.0);
    let s0 = doc.get_session_id();
    doc.add_track(InstrumentType::Synth, 0);
    doc.set_bpm(100.0);
    assert_eq!(doc.get_session_id(), s0);
    assert!(doc.undo_manager().can_undo());
    doc.perform_undo();
    doc.perform_redo();
    assert_eq!(doc.get_session_id(), s0);

    doc.reset_to_starter();
    let s1 = doc.get_session_id();
    assert_ne!(s1, s0);
    let snap = doc.get_arrangement().clone();
    doc.load_from_template(&snap).unwrap();
    let s2 = doc.get_session_id();
    assert_ne!(s2, s1);
    let snap = doc.get_arrangement().clone();
    doc.load_project(&snap);
    let s3 = doc.get_session_id();
    assert_ne!(s3, s2);
    let other = Arrangement {
        id: "other-project".into(),
        ..doc.get_arrangement().clone()
    };
    doc.restore_from_snapshot(&other);
    assert_ne!(doc.get_session_id(), s3);
}

#[test]
fn set_kit_reports_the_selection_with_its_source_pad_edits_and_undo_do_not() {
    let mut doc = counter_doc();
    doc.reset_to_starter();
    let track_id = doc.get_active_drum_track_id().unwrap();
    doc.drain_events();
    let kit = KitStateExt::with_id(doc.get_kit(&track_id), "k1", "K1");
    doc.set_kit(&track_id, &kit, KitSelectionSource::User);
    doc.set_kit(
        &track_id,
        &KitStateExt::with_id(kit.clone(), "k2", "K1"),
        KitSelectionSource::PackLoad,
    );
    doc.set_kit(&track_id, &kit, KitSelectionSource::User);
    doc.update_pad(&track_id, 0, json!({ "gainDb": -6 }).as_object().unwrap());
    doc.perform_undo();
    let seen: Vec<KitSelectedEvent> = doc
        .drain_events()
        .into_iter()
        .filter_map(|e| {
            if let DocEvent::KitSelected(k) = e {
                Some(k)
            } else {
                None
            }
        })
        .collect();
    let ev = |kit_id: &str, source| KitSelectedEvent {
        track_id: track_id.clone(),
        kit_id: kit_id.into(),
        source,
    };
    assert_eq!(
        seen,
        vec![
            ev("k1", KitSelectionSource::User),
            ev("k2", KitSelectionSource::PackLoad),
            ev("k1", KitSelectionSource::User)
        ]
    );
}

struct KitStateExt;
impl KitStateExt {
    fn with_id(mut kit: crate::types::KitState, id: &str, name: &str) -> crate::types::KitState {
        kit.id = id.into();
        kit.name = name.into();
        kit
    }
}

#[test]
fn is_loop_recording_reflects_a_take() {
    let mut doc = counter_doc();
    doc.reset_to_starter();
    doc.set_loop_settings(&LoopSettingsPatch {
        count_in_bars: Some(0),
        metronome: Some(false),
        ..Default::default()
    });
    assert!(doc.begin_loop_take().is_some());
    assert!(doc.is_loop_recording());
    assert!(doc.begin_loop_take().is_none(), "one take at a time");
    doc.stop_loop_take();
    assert!(!doc.is_loop_recording());
}

#[test]
fn lists_the_audio_regions_whose_sample_is_unavailable() {
    let mut doc = counter_doc();
    let track = doc.add_track(InstrumentType::Audio, 0);
    let s = |id: &str, path: &str| SampleRef {
        id: id.into(),
        name: "x".into(),
        path: path.into(),
        duration_seconds: 1.0,
        sample_rate: 44100.0,
        channels: 1,
    };
    let kick = s("kick", "/data/packs/uzu-drumkit/kick/10_bd.wav");
    let region = doc.add_audio_region(&track.id, Some(&kick), 0.0).unwrap();
    doc.add_audio_region(&track.id, Some(&s("loop", "/Users/me/loop.wav")), 8.0);
    assert!(doc.get_unavailable_audio_regions(|_| false).is_empty());
    assert_eq!(
        doc.get_unavailable_audio_regions(|id| id == "kick"),
        vec![UnavailableAudioRegion {
            track_id: track.id.clone(),
            region_id: region.id,
            sample_id: "kick".into()
        }]
    );
}

// --- useArrangement.test (engine-facing) ---

#[test]
fn opted_out_slices_read_fresh_values() {
    let mut doc = counter_doc();
    doc.reset_to_starter();
    doc.set_loop_settings(&LoopSettingsPatch {
        bars: Some(1),
        count_in_bars: Some(0),
        metronome: Some(false),
        overdub: Some(true),
        quantize: Some(crate::types::QuantizeGrid::Q16),
        ..Default::default()
    });
    let id = doc.get_active_drum_track_id().unwrap();
    doc.toggle_step(&id, 0, 0, 0.25, None);
    assert_eq!(doc.get_pattern_region(&id).unwrap().notes.len(), 1);
    assert!(!doc.get_tracks().is_empty());
}

// --- Song-side edits (engine semantics, inventory §1.1) ---

#[test]
fn add_track_defaults_names_and_colours() {
    let mut doc = counter_doc();
    let a = doc.add_track(InstrumentType::Synth, 3);
    assert_eq!(
        (a.name.as_str(), a.volume, a.pan, a.color.as_str()),
        ("Warm Pad", -6.0, 0.0, TRACK_COLORS[0])
    );
    assert_eq!(doc.add_track(InstrumentType::Synth, 99).name, "Synth");
    let audio = doc.add_track(InstrumentType::Audio, 5);
    assert_eq!(
        (audio.name.as_str(), audio.instrument.preset_index),
        ("Audio", 0)
    );
    assert_eq!(doc.add_track(InstrumentType::Drums, 0).name, "Drums");
    assert_eq!(doc.add_track(InstrumentType::Drums, 0).name, "Drums 2");
    assert_eq!(label(&doc), Some("Add Track"));
    for i in 0..5 {
        doc.add_track(InstrumentType::Synth, 0);
        let _ = i;
    }
    assert_eq!(doc.get_tracks()[8].color, TRACK_COLORS[0], "colours cycle");
}

#[test]
fn mixer_setters_write_no_history_but_a_missing_track_still_pushes_for_undoable_edits() {
    let mut doc = counter_doc();
    let t = doc.add_track(InstrumentType::Synth, 0);
    let n = doc.undo_manager().undo_len();
    doc.set_track_volume(&t.id, -12.0);
    doc.set_track_mute(&t.id, true);
    doc.set_track_pan(&t.id, 0.5);
    doc.set_track_solo(&t.id, true);
    assert_eq!(doc.undo_manager().undo_len(), n);
    doc.add_region("missing", region("r", 0.0, 4.0, vec![]));
    assert_eq!(doc.undo_manager().undo_len(), n + 1);
    assert_eq!(label(&doc), Some("Add Region"));
}

#[test]
fn split_region_midi_and_audio() {
    let mut doc = counter_doc();
    let t = doc.add_track(InstrumentType::Synth, 0);
    let mut r = region(
        "r1",
        0.0,
        8.0,
        vec![
            note(60.0, 0.0, 1.0),
            note(62.0, 2.5, 2.0),
            note(64.0, 5.0, 1.0),
        ],
    );
    r.name = Some("Verse".into());
    doc.add_region(&t.id, r);
    let n = doc.undo_manager().undo_len();
    doc.split_region(&t.id, "r1", 0.0);
    doc.split_region(&t.id, "r1", 8.0);
    doc.split_region(&t.id, "missing", 3.0);
    assert_eq!(
        doc.get_tracks()[0].regions.len(),
        1,
        "outside (0, len): no-op"
    );
    // Phase-8 review #13 (Electron bug fixed in both apps): no undo entry for a no-op split.
    assert_eq!(doc.undo_manager().undo_len(), n);
    assert_eq!(label(&doc), Some("Add Region"));
    doc.split_region(&t.id, "r1", 3.0);
    let regions = &doc.get_tracks()[0].regions;
    assert_eq!(regions[0].name.as_deref(), Some("Verse (L)"));
    assert_eq!(regions[1].name.as_deref(), Some("Verse (R)"));
    assert_eq!(
        regions[0]
            .notes
            .iter()
            .map(|n| n.duration_beats)
            .collect::<Vec<_>>(),
        vec![1.0, 0.5]
    );
    assert_eq!(
        regions[1]
            .notes
            .iter()
            .map(|n| (n.start_beat, n.duration_beats))
            .collect::<Vec<_>>(),
        vec![(0.0, 1.5), (2.0, 1.0)]
    );

    let a = doc.add_track(InstrumentType::Audio, 0);
    let audio = AudioRegionData {
        sample_id: "s".into(),
        sample_path: "/s.wav".into(),
        offset_seconds: 0.5,
        gain_db: 0.0,
        original_duration: 6.0,
        extra: Default::default(),
    };
    doc.add_region(
        &a.id,
        Region {
            audio: Some(audio),
            ..region("a1", 4.0, 8.0, vec![])
        },
    );
    doc.split_region(&a.id, "a1", 7.0);
    let regions = &doc.get_tracks()[1].regions;
    assert_eq!(regions[0].length_beats, 3.0);
    assert_eq!(regions[1].audio.as_ref().unwrap().offset_seconds, 0.5 + 1.5);
    assert_eq!(regions[1].name, None);
}

#[test]
fn resize_region_trims_notes_and_audio_offsets() {
    let mut doc = counter_doc();
    let t = doc.add_track(InstrumentType::Synth, 0);
    doc.add_region(
        &t.id,
        region(
            "r",
            0.0,
            8.0,
            vec![
                note(60.0, 0.0, 2.0),
                note(62.0, 3.0, 4.0),
                note(64.0, 7.5, 0.5),
            ],
        ),
    );
    doc.resize_region(&t.id, "r", Some(1.0), None);
    let r = &doc.get_tracks()[0].regions[0];
    assert_eq!(r.start_beat, 1.0);
    assert_eq!(
        r.notes
            .iter()
            .map(|n| (n.start_beat, n.duration_beats))
            .collect::<Vec<_>>(),
        vec![(0.0, 1.0), (2.0, 4.0), (6.5, 0.5)]
    );
    doc.resize_region(&t.id, "r", None, Some(4.0));
    let r = &doc.get_tracks()[0].regions[0];
    assert_eq!(
        r.notes
            .iter()
            .map(|n| (n.start_beat, n.duration_beats))
            .collect::<Vec<_>>(),
        vec![(0.0, 1.0), (2.0, 2.0)]
    );
    doc.resize_region(&t.id, "r", None, Some(0.0));
    assert_eq!(doc.get_tracks()[0].regions[0].length_beats, 0.25);
    assert_eq!(label(&doc), Some("Trim Region"));

    let a = doc.add_track(InstrumentType::Audio, 0);
    let audio = AudioRegionData {
        sample_id: "s".into(),
        sample_path: "/s.wav".into(),
        offset_seconds: 0.0,
        gain_db: 0.0,
        original_duration: 6.0,
        extra: Default::default(),
    };
    doc.add_region(
        &a.id,
        Region {
            audio: Some(audio),
            ..region("a", 4.0, 8.0, vec![])
        },
    );
    doc.resize_region(&a.id, "a", Some(5.0), None);
    assert_eq!(
        doc.get_tracks()[1].regions[0]
            .audio
            .as_ref()
            .unwrap()
            .offset_seconds,
        0.5
    );
}

#[test]
fn duplicate_copy_paste_and_auto_extend() {
    let mut doc = counter_doc();
    let t = doc.add_track(InstrumentType::Synth, 0);
    doc.add_region(
        &t.id,
        Region {
            name: Some("A".into()),
            ..region("r", 60.0, 8.0, vec![note(60.0, 0.0, 1.0)])
        },
    );
    assert_eq!(doc.get_arrangement().length_beats, 72.0);
    doc.duplicate_region(&t.id, "r");
    let dup = doc.get_tracks()[0].regions[1].clone();
    assert_eq!(
        (dup.start_beat, dup.name.as_deref()),
        (68.0, Some("A (copy)"))
    );
    assert_eq!(doc.get_arrangement().length_beats, 80.0);
    assert!(!doc.has_clipboard());
    doc.paste_region(&t.id, 0.0);
    assert_eq!(
        label(&doc),
        Some("Duplicate Region"),
        "no clipboard: no-op without history"
    );
    doc.copy_region(&t.id, "r");
    doc.paste_region(&t.id, 100.0);
    assert_eq!(doc.get_tracks()[0].regions[2].start_beat, 100.0);
    assert_eq!(doc.get_arrangement().length_beats, 112.0);
    assert_eq!(label(&doc), Some("Paste Region"));
}

#[test]
fn markers_sorted_clamped_coloured_and_seek_targets() {
    let mut doc = counter_doc();
    let a = doc.add_marker("A", 16.0, None);
    let b = doc.add_marker("B", -4.0, Some("#fff"));
    doc.add_marker("C", 8.0, None);
    assert_eq!(a.color, MARKER_COLORS[0]);
    assert_eq!(b.beat, 0.0);
    let beats: Vec<f64> = doc.get_markers().iter().map(|m| m.beat).collect();
    assert_eq!(beats, vec![0.0, 8.0, 16.0]);
    assert_eq!(doc.get_markers()[1].color, MARKER_COLORS[2]);
    assert_eq!(doc.next_marker_target(8.0), Some(16.0));
    assert_eq!(doc.next_marker_target(7.995), Some(16.0));
    assert_eq!(doc.previous_marker_target(8.0), Some(0.0));
    assert_eq!(doc.previous_marker_target(0.0), None);
    assert_eq!(doc.marker_seek_target(&a.id), Some(16.0));
    doc.update_marker(
        &a.id,
        &MarkerUpdate {
            beat: Some(-1.0),
            ..Default::default()
        },
    );
    // stable sort: A (now 0) stays after B (already at 0)
    assert_eq!(doc.get_markers()[1].id, a.id);
    assert_eq!(label(&doc), Some("Update Marker"));
}

#[test]
fn groups_one_group_per_track_and_cleanup_on_remove() {
    let mut doc = counter_doc();
    let ids: Vec<String> = (0..3)
        .map(|_| doc.add_track(InstrumentType::Synth, 0).id)
        .collect();
    doc.create_group("G1", &ids[0..2]);
    let g2 = doc.create_group("G2", &ids[1..3]);
    assert_eq!(doc.get_groups()[0].track_ids, vec![ids[0].clone()]);
    assert_eq!(g2.color, TRACK_COLORS[1]);
    doc.remove_track(&ids[0]);
    assert_eq!(doc.get_groups().len(), 1, "empty groups are dropped");
    doc.toggle_group_collapsed(&g2.id);
    assert_eq!(label(&doc), Some("Remove Track"));
    doc.set_group_solo(&g2.id, true);
    assert!(doc.track_audible(&ids[1]));
}

#[test]
fn automation_lanes_and_points() {
    let mut doc = counter_doc();
    let t = doc.add_track(InstrumentType::Synth, 0);
    doc.add_automation_lane(&t.id, AutomationParameter::Volume);
    doc.add_automation_lane(&t.id, AutomationParameter::Volume);
    assert_eq!(doc.get_tracks()[0].automation.as_ref().unwrap().len(), 1);
    doc.set_automation_point(&t.id, AutomationParameter::Volume, 4.1, 0.7, 0.25);
    doc.set_automation_point(&t.id, AutomationParameter::Volume, 1.0, 1.5, 0.0);
    let pts = &doc.get_tracks()[0].automation.as_ref().unwrap()[0].points;
    assert_eq!(
        pts.iter().map(|p| (p.beat, p.value)).collect::<Vec<_>>(),
        vec![(1.0, 1.0), (4.0, 0.7)]
    );
    let n = doc.undo_manager().undo_len();
    doc.toggle_automation_lane_visibility(&t.id, AutomationParameter::Volume);
    assert_eq!(doc.undo_manager().undo_len(), n);
    doc.drain_events();
    doc.remove_automation_lane(&t.id, AutomationParameter::Volume);
    assert_eq!(
        doc.drain_events(),
        vec![DocEvent::TrackEffectsReset(t.id.clone())]
    );
}

#[test]
fn loop_enabled_uses_the_note_range_and_loop_markers_are_clamped() {
    let mut doc = counter_doc();
    let t = doc.add_track(InstrumentType::Synth, 0);
    doc.add_region(
        &t.id,
        region(
            "r",
            4.0,
            8.0,
            vec![note(60.0, 1.0, 0.05), note(62.0, 6.0, 1.0)],
        ),
    );
    doc.set_loop_enabled(true);
    assert_eq!(
        doc.get_loop_markers(),
        Some(LoopMarkers {
            start_beat: 5.0,
            end_beat: 11.0
        })
    );
    // quirk: the end is clamped against the RAW start (`max(start + 1, end)` with start = -2)
    doc.set_loop_markers(-2.0, 0.5);
    assert_eq!(
        doc.get_loop_markers(),
        Some(LoopMarkers {
            start_beat: 0.0,
            end_beat: 0.5
        })
    );
    doc.set_loop_markers(10.0, 1000.0);
    assert_eq!(
        doc.get_loop_markers(),
        Some(LoopMarkers {
            start_beat: 10.0,
            end_beat: 64.0
        })
    );
    assert_eq!(doc.get_active_loop_range(false), doc.get_loop_markers());
    doc.set_loop_enabled(false);
    assert_eq!(doc.get_active_loop_range(false), None);
}

#[test]
fn commit_song_recording_plain_and_punch() {
    let mut doc = counter_doc();
    let t = doc.add_track(InstrumentType::Synth, 0);
    assert!(
        doc.commit_song_recording(&t.id, vec![], None, 0.0, 3.0)
            .is_none()
    );
    assert_eq!(
        label(&doc),
        Some("Record"),
        "pushed even when nothing was captured"
    );
    let r = doc
        .commit_song_recording(&t.id, vec![note(60.0, 0.5, 1.0)], None, 0.0, 2.3)
        .unwrap();
    assert_eq!((r.start_beat, r.length_beats), (0.0, 3.0));
    doc.add_region(&t.id, region("keep", 20.0, 4.0, vec![]));
    let punch = LoopMarkers {
        start_beat: 2.0,
        end_beat: 6.0,
    };
    let r2 = doc
        .commit_song_recording(&t.id, vec![note(62.0, 0.0, 1.0)], Some(punch), 2.0, 9.0)
        .unwrap();
    let ids: Vec<&str> = doc.get_tracks()[0]
        .regions
        .iter()
        .map(|r| r.id.as_str())
        .collect();
    assert_eq!(
        ids,
        vec!["keep", r2.id.as_str()],
        "overlapping regions replaced"
    );
    assert_eq!((r2.start_beat, r2.length_beats), (2.0, 4.0));
}

#[test]
fn import_audio_sample_names_the_track_after_the_sample() {
    let mut doc = counter_doc();
    let sample = SampleRef {
        id: "s1".into(),
        name: "Break".into(),
        path: "/b.wav".into(),
        duration_seconds: 2.0,
        sample_rate: 44100.0,
        channels: 2,
    };
    let r = doc.import_audio_sample(&sample).unwrap();
    assert_eq!(doc.get_tracks()[0].name, "Break");
    assert_eq!(r.length_beats, 4.0);
    assert_eq!(label(&doc), Some("Add Audio Region"));
}

#[test]
fn local_storage_round_trip_and_bad_data() {
    let mut doc = counter_doc();
    doc.reset_to_starter();
    let saved = doc.save_to_local_storage();
    let s = doc.get_session_id();
    assert!(matches!(
        doc.load_from_local_storage(Some("{oops")),
        Err(crate::migrate::ProjectLoadError::Json(_))
    ));
    assert_eq!(doc.load_from_local_storage(None), Ok(None));
    assert_eq!(doc.load_from_local_storage(Some("")), Ok(None));
    assert_eq!(doc.get_session_id(), s);
    assert_eq!(doc.load_from_local_storage(Some(&saved)), Ok(Some(vec![])));
    assert_eq!(doc.get_session_id(), s + 1);
    // A shape the TS migration throws on: reported, session bumped (as in the TS), arrangement kept.
    let before = doc.get_arrangement().clone();
    assert!(matches!(
        doc.load_from_local_storage(Some(r#"{"tracks":"x"}"#)),
        Err(crate::migrate::ProjectLoadError::Shape(_))
    ));
    assert_eq!(doc.get_session_id(), s + 2);
    assert_eq!(doc.get_arrangement(), &before);
    // Coercible garbage loads, with warnings.
    let warnings = doc
        .load_from_local_storage(Some(
            r#"{"id":"x","bpm":null,"lengthBeats":"100","tracks":[{"id":"t","instrument":{"type":"synth","presetIndex":2.5},"regions":[],"volume":null}]}"#,
        ))
        .unwrap()
        .unwrap();
    assert_eq!(doc.get_bpm(), 120.0);
    assert_eq!(doc.get_arrangement().length_beats, 100.0);
    assert_eq!(doc.get_tracks()[0].instrument.preset_index, 0);
    let paths: Vec<&str> = warnings.iter().map(|w| w.path.as_str()).collect();
    assert!(
        paths.contains(&"$.bpm") && paths.contains(&"$.lengthBeats"),
        "{paths:?}"
    );
    assert!(
        paths.contains(&"$.tracks[0].volume")
            && paths.contains(&"$.tracks[0].instrument.presetIndex")
    );
}
