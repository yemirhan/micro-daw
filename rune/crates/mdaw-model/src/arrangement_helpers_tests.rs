//! Ported from src/utils/arrangementHelpers.test.ts, plus the swing golden fixture
//! (rune/parity/ts/swing.golden.ts → rune/parity/fixtures/swing.json).

use super::*;
use crate::kit::create_factory_kit;
use crate::migrate::tests_support::templates;
use crate::types::{BeatState, PadSample, PadSettings, Region, TrackInstrument, pad_to_note};
use QuantizeGrid as Q;
use serde_json::json;

fn hit(note: i32, start_beat: f64, velocity: f64) -> RegionNote {
    RegionNote { note: note as f64, start_beat, velocity, duration_beats: 0.25, is_drum: true }
}

fn drum_track(notes: Vec<RegionNote>, kit: bool) -> Track {
    Track {
        id: "t1".into(),
        name: "Drums".into(),
        instrument: TrackInstrument {
            kind: InstrumentType::Drums,
            preset_index: 0,
            synth_overrides: None,
            kit: kit.then(create_factory_kit),
        },
        regions: vec![Region {
            id: "r1".into(),
            start_beat: 0.0,
            length_beats: 4.0,
            notes,
            color: None,
            name: None,
            audio: None,
        }],
        volume: 0.0,
        pan: 0.0,
        muted: false,
        solo: false,
        color: "#fff".into(),
        effects: None,
        automation: None,
        extra: Default::default(),
    }
}

fn no_id() -> String {
    panic!("unexpected id")
}

fn close(a: f64, b: f64, digits: i32) -> bool {
    (a - b).abs() < 10f64.powi(-digits) / 2.0
}

// --- quantizeBeat ---

#[test]
fn snaps_to_straight_grids() {
    assert_eq!(quantize_beat(0.13, Q::Q16, 0.0, Q::Q16), 0.25);
    assert_eq!(quantize_beat(0.12, Q::Q16, 0.0, Q::Q16), 0.0);
    assert_eq!(quantize_beat(1.7, Q::Q8, 0.0, Q::Q8), 1.5);
    assert_eq!(quantize_beat(1.8, Q::Q4, 0.0, Q::Q4), 2.0);
    assert_eq!(quantize_beat(0.07, Q::Q32, 0.0, Q::Q32), 0.125);
}

#[test]
fn returns_the_beat_unchanged_for_off() {
    assert_eq!(quantize_beat(1.2345, Q::Off, 0.5, Q::Off), 1.2345);
}

#[test]
fn picks_the_nearest_swung_step_but_returns_its_straight_position() {
    assert_eq!(quantize_beat(0.3, Q::Q16, 0.5, Q::Q16), 0.25);
    assert_eq!(quantize_beat(0.375, Q::Q16, 0.5, Q::Q16), 0.25);
    assert_eq!(quantize_beat(0.4, Q::Q16, 0.6, Q::Q16), 0.25);
    assert_eq!(quantize_beat(0.4, Q::Q16, 0.0, Q::Q16), 0.5);
    assert_eq!(quantize_beat(0.52, Q::Q16, 0.5, Q::Q16), 0.5);
    assert_eq!(quantize_beat(0.47, Q::Q16, 0.5, Q::Q16), 0.5);
    assert_eq!(quantize_beat(0.45, Q::Q16, 2.0, Q::Q16), 0.25);
}

#[test]
fn handles_triplet_grids() {
    assert_eq!(quantize_beat(0.3, Q::Q8T, 0.0, Q::Q8T), 0.333333333);
    assert_eq!(quantize_beat(0.7, Q::Q8T, 0.5, Q::Q8T), 0.666666667);
    assert_eq!(quantize_beat(1.02, Q::Q8T, 0.0, Q::Q8T), 1.0);
    assert_eq!(quantize_beat(0.18, Q::Q16T, 0.6, Q::Q16T), 0.166666667);
    assert_eq!(quantize_beat(2.9, Q::Q16T, 0.0, Q::Q16T), 2.833333333);
}

#[test]
fn pattern_step_at_never_maps_a_note_past_the_last_step() {
    assert_eq!(pattern_step_at(0.0, 0.25, 16.0), 0.0);
    assert_eq!(pattern_step_at(1.24, 0.25, 16.0), 5.0);
    assert_eq!(pattern_step_at(3.9, 0.25, 16.0), 15.0);
    assert_eq!(pattern_step_at(3.99, 0.25, 16.0), 15.0);
    assert_eq!(pattern_step_at(-0.01, 0.25, 16.0), 0.0);
}

#[test]
fn swing_offset_beats_delays_odd_steps_of_the_swing_grid_only() {
    assert_eq!(swing_offset_beats(0.25, Q::Q16, 0.5), 0.125);
    assert_eq!(swing_offset_beats(4.75, Q::Q16, 0.5), 0.125);
    assert_eq!(swing_offset_beats(0.5, Q::Q16, 0.5), 0.0);
    assert_eq!(swing_offset_beats(0.3, Q::Q16, 0.5), 0.0);
    assert_eq!(swing_offset_beats(0.25, Q::Q16, 0.0), 0.0);
    assert_eq!(swing_offset_beats(0.5, SwingGrid::S8, 0.5), 0.25);
    assert_eq!(swing_offset_beats(0.25, SwingGrid::S8, 0.5), 0.0);
    assert_eq!(swing_offset_beats(1.0 / 3.0, Q::Q8T, 0.5), 0.0);
    assert_eq!(swing_offset_beats(0.25, Q::Off, 0.5), 0.125);
}

#[test]
fn grid_step_beats_values() {
    assert_eq!(grid_step_beats(Q::Q16), Some(0.25));
    assert!(close(grid_step_beats(Q::Q8T).unwrap(), 1.0 / 3.0, 2));
    assert_eq!(grid_step_beats(Q::Off), None);
}

// --- wrapBeat ---

#[test]
fn wraps_into_0_length() {
    assert_eq!(wrap_beat(4.25, 4.0), 0.25);
    assert_eq!(wrap_beat(-0.25, 4.0), 3.75);
    assert_eq!(wrap_beat(4.0, 4.0), 0.0);
    assert_eq!(wrap_beat(7.99999, 8.0), 0.0);
}

// --- mergeDrumHit ---

#[test]
fn appends_new_hits_and_keeps_the_louder_of_duplicates() {
    let mut notes = merge_drum_hit(&[], hit(36, 0.0, 0.5));
    notes = merge_drum_hit(&notes, hit(38, 0.0, 0.5));
    assert_eq!(notes.len(), 2);
    notes = merge_drum_hit(&notes, hit(36, 0.00001, 0.9));
    assert_eq!(notes.len(), 2);
    assert_eq!(notes[0].velocity, 0.9);
    notes = merge_drum_hit(&notes, hit(36, 0.0, 0.2));
    assert_eq!(notes[0].velocity, 0.9);
}

// --- GM drum migration ---

#[test]
fn maps_gm_notes_to_factory_pads_and_drops_unknown_notes() {
    let res = migrate_gm_drum_notes(&[
        hit(36, 0.0, 0.8),
        hit(38, 1.0, 0.8),
        hit(42, 0.5, 0.8),
        hit(46, 1.5, 0.8),
        hit(49, 0.0, 0.8),
        hit(51, 2.0, 0.8),
        hit(39, 3.0, 0.8),
        hit(100, 0.0, 0.8),
    ]);
    assert_eq!(res.dropped, 1);
    let want: Vec<f64> = [0, 1, 2, 3, 6, 7, 4].iter().map(|&p| pad_to_note(p) as f64).collect();
    assert_eq!(res.notes.iter().map(|n| n.note).collect::<Vec<_>>(), want);
}

#[test]
fn merges_legacy_notes_that_land_on_the_same_pad() {
    let res = migrate_gm_drum_notes(&[hit(35, 0.0, 0.4), hit(36, 0.0, 0.7)]);
    assert_eq!(res.notes, vec![hit(pad_to_note(0), 0.0, 0.7)]);
}

#[test]
fn migrates_kitless_drum_tracks_only() {
    let mut legacy = drum_track(vec![hit(38, 0.0, 0.8)], false);
    assert_eq!(migrate_drum_track(&mut legacy, None, &mut no_id), 0);
    assert_eq!(legacy.instrument.kit.as_ref().unwrap().pads.len(), 16);
    assert_eq!(legacy.regions[0].notes[0].note, pad_to_note(1) as f64);

    let mut modern = drum_track(vec![hit(38, 0.0, 0.8)], true);
    migrate_drum_track(&mut modern, None, &mut no_id);
    assert_eq!(modern.regions[0].notes[0].note, 38.0);
}

#[test]
fn every_template_drum_note_maps_to_the_matching_factory_voice() {
    let mut drum_tracks = 0;
    for (id, template) in templates() {
        let arrangement = &template["arrangement"];
        for raw in arrangement["tracks"].as_array().unwrap() {
            if raw["instrument"]["type"] != "drums" {
                continue;
            }
            drum_tracks += 1;
            let mut track: Track = serde_json::from_value(raw.clone()).unwrap();
            let gm: Vec<f64> = track.regions.iter().flat_map(|r| r.notes.iter().map(|n| n.note)).collect();
            assert_eq!(migrate_drum_track(&mut track, None, &mut no_id), 0, "{id}");
            let migrated: Vec<f64> = track.regions.iter().flat_map(|r| r.notes.iter().map(|n| n.note)).collect();
            assert!(migrated.iter().all(|&n| (36.0..=51.0).contains(&n)), "{id}");
            let back: Vec<f64> = migrated.iter().map(|&n| drum_note_to_gm(n, track.instrument.kit.as_ref())).collect();
            assert_eq!(back, gm, "{id}");
        }
    }
    assert!(drum_tracks >= 5);
}

// --- drumNoteToGm ---

#[test]
fn uses_the_factory_voice_gm_note_else_the_pad_note() {
    let mut kit = create_factory_kit();
    assert_eq!(drum_note_to_gm(pad_to_note(0) as f64, Some(&kit)), 36.0);
    assert_eq!(drum_note_to_gm(pad_to_note(2) as f64, Some(&kit)), 42.0);
    assert_eq!(drum_note_to_gm(pad_to_note(15) as f64, Some(&kit)), 56.0);
    kit.pads[1] = PadSettings {
        sample: Some(PadSample {
            name: "x".into(),
            source: PadSampleSource::File { path: "/x.wav".into(), sample_id: None },
        }),
        ..kit.pads[1].clone()
    };
    assert_eq!(drum_note_to_gm(pad_to_note(1) as f64, Some(&kit)), pad_to_note(1) as f64);
    assert_eq!(drum_note_to_gm(70.0, Some(&kit)), 70.0);
    // classic.test.ts 'padName and GM export understand classic voices' (GM half)
    let classic = crate::kit::create_classic_kit(crate::types::ClassicMachineId::Tr808);
    assert_eq!(drum_note_to_gm(pad_to_note(0) as f64, Some(&classic)), 36.0);
    assert_eq!(drum_note_to_gm(pad_to_note(2) as f64, Some(&classic)), 42.0);
    assert_eq!(drum_note_to_gm(pad_to_note(7) as f64, Some(&classic)), 56.0);
}

// --- loop / beat state normalization ---

#[test]
fn fills_defaults_and_rejects_invalid_values() {
    assert_eq!(normalize_loop_settings(&Value::Null, &DEFAULT_LOOP_SETTINGS), DEFAULT_LOOP_SETTINGS);
    assert_eq!(
        normalize_loop_settings(
            &json!({ "bars": 3, "quantize": "1/5", "swing": 9, "countInBars": 2, "overdub": false }),
            &DEFAULT_LOOP_SETTINGS
        ),
        LoopRecordSettings { swing: 0.75, count_in_bars: 2, overdub: false, ..DEFAULT_LOOP_SETTINGS }
    );
}

#[test]
fn falls_back_to_the_first_drum_track_for_a_stale_active_id() {
    let t = drum_track(vec![], true);
    let raw: BeatState = serde_json::from_value(json!({ "activeDrumTrackId": "gone", "selectedPad": 40 })).unwrap();
    let b = normalize_beat_state(Some(&raw), std::slice::from_ref(&t));
    assert_eq!((b.active_drum_track_id.as_deref(), b.selected_pad), (Some("t1"), 0));
    assert_eq!(normalize_beat_state(None, &[]).active_drum_track_id, None);
}

// --- review round 2: swing grid + gate length ---

#[test]
fn quantize_beat_swing_applies_on_its_own_grid() {
    assert_eq!(quantize_beat(1.1, Q::Q4, 0.5, SwingGrid::S16), 1.0);
    assert_eq!(quantize_beat(0.9, Q::Q4, 0.5, SwingGrid::S16), 1.0);
    assert_eq!(quantize_beat(0.72, Q::Q16, 0.5, SwingGrid::S8), 0.5);
    assert_eq!(quantize_beat(0.27, Q::Q16, 0.5, SwingGrid::S8), 0.25);
    assert_eq!(quantize_beat(0.74, Q::Q8, 0.5, SwingGrid::S8), 0.5);
    assert_eq!(quantize_beat(0.875, Q::Q32, 0.75, SwingGrid::S8), 0.5);
}

#[test]
fn normalize_loop_settings_swing_grid_migration() {
    let n = |v: Value| normalize_loop_settings(&v, &DEFAULT_LOOP_SETTINGS).swing_grid;
    assert_eq!(DEFAULT_LOOP_SETTINGS.swing_grid, SwingGrid::S16);
    assert_eq!(n(json!({ "quantize": "1/4", "swing": 0.3 })), SwingGrid::S16);
    assert_eq!(n(json!({ "quantize": "1/8", "swing": 0.3 })), SwingGrid::S8);
    assert_eq!(n(json!({ "quantize": "1/8", "swingGrid": "1/16" })), SwingGrid::S16);
    let base = LoopRecordSettings { swing_grid: SwingGrid::S8, ..DEFAULT_LOOP_SETTINGS };
    assert_eq!(normalize_loop_settings(&json!({ "swingGrid": "1/32" }), &base).swing_grid, SwingGrid::S8);
    assert_eq!(loop_swing_grid(None), SwingGrid::S16);
    assert_eq!(swing_offset_beats(0.5, SwingGrid::S8, 0.5), 0.25);
}

#[test]
fn loop_gate_beats_musical_length() {
    assert_eq!(loop_gate_beats(1.0, 2.0, 0.0, 4.0), 1.0);
    assert_eq!(loop_gate_beats(3.5, 0.5, 4.0, 4.0), 1.0);
    assert_eq!(loop_gate_beats(1.0, 1.5, 4.0, 4.0), 4.0);
    assert_eq!(loop_gate_beats(2.0, 2.0, 0.0, 4.0), 1.0 / 16.0);
    assert_eq!(loop_gate_beats(-0.25, 0.75, 0.0, 4.0), 1.0);
    assert!(close(loop_gate_beats(0.15, 1.567, 0.0, 4.0), 1.417, 6));
    assert_eq!(loop_gate_beats(3.5, 0.5, 0.0, 4.0), 1.0);
    assert_eq!(loop_gate_beats(1.0, 2.0, f64::NAN, 4.0), 1.0);
}

#[test]
fn small_helpers() {
    assert_eq!(snap_beat(1.3, 0.5), 1.5);
    assert_eq!(snap_beat(1.25, 0.5), 1.5); // Math.round: .5 rounds up
    assert_eq!(snap_beat(-1.25, 0.5), -1.0);
    assert_eq!(snap_beat(1.3, 0.0), 1.3);
    assert_eq!(beat_to_px(2.0, 30.0), 60.0);
    assert_eq!(px_to_beat(60.0, 30.0), 2.0);
}

// --- golden: loop.json (normalizeLoopSettings / normalizeBeatState, loop_settings.rs) ---

#[test]
fn golden_loop_and_beat_state() {
    use crate::loop_settings::beat_state_from_value;
    use crate::migrate::tests_support::assert_json_eq;
    let fx: Value = serde_json::from_str(include_str!("../../../parity/fixtures/loop.json")).unwrap();
    let bases: Vec<LoopRecordSettings> = serde_json::from_value(fx["bases"].clone()).unwrap();
    let track_sets: Vec<Vec<Track>> = serde_json::from_value(fx["trackSets"].clone()).unwrap();
    let mut n = 0;
    for r in fx["loops"].as_array().unwrap() {
        let got = normalize_loop_settings(&r[0], &bases[r[1].as_u64().unwrap() as usize]);
        assert_json_eq(
            &serde_json::to_value(got).unwrap(),
            &r[2],
            &format!("normalizeLoopSettings({}, base {})", r[0], r[1]),
        );
        n += 1;
    }
    for r in fx["beats"].as_array().unwrap() {
        let raw = (r[0] != "__undefined__").then(|| beat_state_from_value(&r[0]));
        let got = normalize_beat_state(raw.as_ref(), &track_sets[r[1].as_u64().unwrap() as usize]);
        assert_json_eq(
            &serde_json::to_value(got).unwrap(),
            &r[2],
            &format!("normalizeBeatState({}, tracks {})", r[0], r[1]),
        );
        n += 1;
    }
    assert!(n > 90, "{n}");
}

// --- golden: swing.json ---

fn grid(v: &Value) -> QuantizeGrid {
    QuantizeGrid::from_label(v.as_str().unwrap()).unwrap()
}

fn f(v: &Value) -> f64 {
    match v {
        Value::String(s) if s == "NaN" => f64::NAN,
        _ => v.as_f64().unwrap(),
    }
}

fn same(a: f64, b: &Value) -> bool {
    match b.as_f64() {
        Some(b) => a == b || (a - b).abs() <= 1e-9,
        None => a.is_nan() && b.is_null(),
    }
}

#[test]
fn golden_swing_vectors() {
    let fx: Value = serde_json::from_str(include_str!("../../../parity/fixtures/swing.json")).unwrap();
    let mut n = 0;
    for r in fx["quantize"].as_array().unwrap() {
        let got = quantize_beat(f(&r[0]), grid(&r[1]), f(&r[2]), grid(&r[3]));
        assert!(same(got, &r[4]), "quantize {r} -> {got}");
        n += 1;
    }
    for r in fx["swing"].as_array().unwrap() {
        let got = swing_offset_beats(f(&r[0]), grid(&r[1]), f(&r[2]));
        assert!(same(got, &r[3]), "swing {r} -> {got}");
        n += 1;
    }
    for r in fx["gate"].as_array().unwrap() {
        let got = loop_gate_beats(f(&r[0]), f(&r[1]), f(&r[2]), f(&r[3]));
        assert!(same(got, &r[4]), "gate {r} -> {got}");
        n += 1;
    }
    for r in fx["wrap"].as_array().unwrap() {
        let got = wrap_beat(f(&r[0]), f(&r[1]));
        assert!(same(got, &r[2]), "wrap {r} -> {got}");
        n += 1;
    }
    for r in fx["steps"].as_array().unwrap() {
        let got = pattern_step_at(f(&r[0]), f(&r[1]), f(&r[2]));
        assert!(same(got, &r[3]), "step {r} -> {got}");
        n += 1;
    }
    assert!(n > 40_000, "{n} vectors");
}
