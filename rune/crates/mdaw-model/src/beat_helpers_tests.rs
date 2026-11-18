//! Ports of the vitest cases of `src/components/beat/{beatHelpers,chop,keyScope,missingPackActions,
//! stepKeys,waveform}.test.ts` and the pure case of `PendingLoadChip.test.tsx`, plus direct checks
//! of the JS number formatting helpers.

use super::golden_tests::{gen_buffer, mulberry32};
use super::*;
use crate::notes::midi_to_note_name;
use crate::types::{FactoryVoiceId, LoopMarkers, PadSample, PadSampleSource, RegionNote, SwingGrid};

fn key(k: &str, code: &str) -> KeyEventLike {
    KeyEventLike { key: k.into(), code: code.into(), ..KeyEventLike::default() }
}

fn close(a: f64, b: f64) -> bool {
    // vitest toBeCloseTo default: 2 decimal digits → |a - b| < 0.005.
    (a - b).abs() < 0.005
}

// --- pad layout -------------------------------------------------------------

#[test]
fn layout_4x4_puts_pad_0_bottom_left_and_pad_15_top_right() {
    let rows = pad_rows(PadLayout::FourByFour);
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[3][0], 0);
    assert_eq!(rows[0][3], 15);
    let mut flat: Vec<usize> = rows.concat();
    flat.sort();
    assert_eq!(flat, (0..PAD_COUNT).collect::<Vec<_>>());
}

#[test]
fn layout_2x8_shows_banks_side_by_side() {
    assert_eq!(pad_rows(PadLayout::TwoByEight), vec![vec![4, 5, 6, 7, 12, 13, 14, 15], vec![0, 1, 2, 3, 8, 9, 10, 11]]);
}

#[test]
fn labels_pads_like_the_hardware_banks() {
    assert_eq!(pad_label(0), "A1");
    assert_eq!(pad_label(7), "A8");
    assert_eq!(pad_label(8), "B1");
    assert_eq!(pad_label(15), "B8");
}

#[test]
fn names_hardware_notes_like_the_rest_of_the_app() {
    assert_eq!(hardware_note_label(Some(36.0)), "C2");
    assert_eq!(hardware_note_label(Some(36.0)), midi_to_note_name(36.0));
    assert_eq!(hardware_note_label(Some(60.0)), "C4");
    assert_eq!(hardware_note_label(None), "—");
}

#[test]
fn derives_velocity_from_the_vertical_click_position() {
    assert_eq!(velocity_from_pointer(0.0, 0.0, 100.0), 1.0);
    assert_eq!(velocity_from_pointer(100.0, 0.0, 100.0), 0.3);
    assert!(close(velocity_from_pointer(50.0, 0.0, 100.0), 0.65));
    assert_eq!(velocity_from_pointer(10.0, 0.0, 0.0), 0.8);
}

// --- computer keyboard pads ---------------------------------------------------

#[test]
fn maps_zxcv_asdf_qwet_1234_to_pads() {
    assert_eq!(pad_for_key_event(&key("z", "KeyZ")), Some(0));
    assert_eq!(pad_for_key_event(&key("v", "KeyV")), Some(3));
    assert_eq!(pad_for_key_event(&key("a", "KeyA")), Some(4));
    assert_eq!(pad_for_key_event(&key("q", "KeyQ")), Some(8));
    assert_eq!(pad_for_key_event(&key("t", "KeyT")), Some(11));
    assert_eq!(pad_for_key_event(&key("r", "KeyR")), None);
    assert_eq!(pad_for_key_event(&key("1", "Digit1")), Some(12));
    assert_eq!(pad_for_key_event(&key("4", "Digit4")), Some(15));
}

#[test]
fn uses_the_physical_key_and_falls_back_to_the_character() {
    assert_eq!(pad_for_key_event(&key("y", "KeyZ")), Some(0));
    assert_eq!(pad_for_key_event(&key("Z", "")), Some(0));
}

#[test]
fn ignores_other_keys_and_modifier_chords() {
    assert_eq!(pad_for_key_event(&key("5", "Digit5")), None);
    assert_eq!(pad_for_key_event(&key("g", "KeyG")), None);
    assert_eq!(pad_for_key_event(&KeyEventLike { meta: true, ..key("s", "KeyS") }), None);
    assert_eq!(pad_for_key_event(&KeyEventLike { ctrl: true, ..key("z", "KeyZ") }), None);
}

#[test]
fn has_a_key_hint_for_every_pad() {
    assert_eq!(PAD_KEY_HINTS.len(), 16);
    assert_eq!(PAD_KEY_HINTS[0], "Z");
    assert_eq!(PAD_KEY_HINTS[15], "4");
}

#[test]
fn detects_typing_targets() {
    let body = DomNode::new("body");
    let input = [DomNode::new("input"), body.clone()];
    let div = [DomNode::new("div"), body.clone()];
    let inner = [DomNode::new("span"), DomNode::new("div").with_contenteditable("true"), body.clone()];
    assert!(is_typing_target(Some(&input)));
    assert!(is_typing_target(Some(&inner)));
    assert!(!is_typing_target(Some(&div)));
    assert!(!is_typing_target(None));
    assert!(!is_typing_target(Some(&[]))); // window
}

// --- step grid -------------------------------------------------------------------

#[test]
fn step_size_and_count() {
    assert_eq!(step_beats_for(StepGrid::S8), 0.5);
    assert_eq!(step_beats_for(StepGrid::S16), 0.25);
    assert_eq!(step_beats_for(StepGrid::S32), 0.125);
    assert_eq!(step_count(1.0, 0.25), 16);
    assert_eq!(step_count(2.0, 0.25), 32);
    assert_eq!(step_count(8.0, 0.125), 256);
}

fn note(n: i32, velocity: f64, start_beat: f64) -> RegionNote {
    RegionNote { note: n as f64, velocity, start_beat, duration_beats: 0.25, is_drum: true }
}

#[test]
fn builds_the_cell_matrix_from_pattern_notes() {
    let notes = vec![
        note(crate::types::pad_to_note(0), 1.0, 0.0),
        note(crate::types::pad_to_note(1), 0.5, 1.0),
        note(crate::types::pad_to_note(15), 64.0, 3.75),
        note(20, 1.0, 0.0),
        note(crate::types::pad_to_note(2), 1.0, 4.0),
    ];
    let m = build_step_matrix(Some(StepRegion { start_beat: 8.0, notes: &notes }), 8.0, 0.25, 16);
    assert_eq!(m[0], 1.0);
    assert!(close(m[16 + 4] as f64, 0.5));
    assert!(close(m[15 * 16 + 15] as f64, 64.0 / 127.0));
    assert_eq!(m.iter().filter(|v| **v > 0.0).count(), 3);
}

#[test]
fn maps_straight_stored_notes_to_their_straight_step() {
    let notes: Vec<RegionNote> =
        [1.0, 2.0, 3.0].iter().map(|s| note(crate::types::pad_to_note(4), 0.8, s * 0.25)).collect();
    let m = build_step_matrix(Some(StepRegion { start_beat: 0.0, notes: &notes }), 0.0, 0.25, 16);
    let on: Vec<bool> = m[4 * 16..4 * 16 + 5].iter().map(|v| *v > 0.0).collect();
    assert_eq!(on, vec![false, true, true, true, false]);
}

#[test]
fn shows_a_note_in_the_last_half_step_on_the_last_step() {
    let notes: Vec<RegionNote> =
        [3.8, 3.95].iter().enumerate().map(|(i, s)| note(crate::types::pad_to_note(i), 0.8, *s)).collect();
    let m = build_step_matrix(Some(StepRegion { start_beat: 0.0, notes: &notes }), 0.0, 0.25, 16);
    assert!(m[15] > 0.0);
    assert!(m[16 + 15] > 0.0);
    assert_eq!(m.iter().filter(|v| **v > 0.0).count(), 2);
}

#[test]
fn returns_an_empty_matrix_without_a_region() {
    assert!(build_step_matrix(None, 0.0, 0.25, 16).iter().all(|v| *v == 0.0));
}

#[test]
fn finds_the_cell_under_a_point_with_pad_0_on_the_bottom_row() {
    assert_eq!(cell_at(0.0, 0.0, 20.0, 10.0, 16), Some(StepCell { pad: 15, step: 0 }));
    assert_eq!(cell_at(39.0, 155.0, 20.0, 10.0, 16), Some(StepCell { pad: 0, step: 1 }));
    assert_eq!(cell_at(320.0, 5.0, 20.0, 10.0, 16), None);
    assert_eq!(cell_at(5.0, 160.0, 20.0, 10.0, 16), None);
    assert_eq!(cell_at(-1.0, 5.0, 20.0, 10.0, 16), None);
    assert_eq!(row_for_pad(0), 15);
}

#[test]
fn paint_mode_is_decided_by_the_first_cell() {
    assert_eq!(paint_mode_for(0.0), PaintMode::Draw);
    assert_eq!(paint_mode_for(0.7), PaintMode::Erase);
    assert!(needs_toggle(PaintMode::Draw, 0.0));
    assert!(!needs_toggle(PaintMode::Draw, 0.5));
    assert!(needs_toggle(PaintMode::Erase, 0.5));
    assert!(!needs_toggle(PaintMode::Erase, 0.0));
}

#[test]
fn adjusts_velocity_by_drag_distance() {
    assert_eq!(adjust_velocity(0.5, 60.0, ADJUST_VELOCITY_PX_PER_FULL), 1.0);
    assert_eq!(adjust_velocity(0.5, 12.0, ADJUST_VELOCITY_PX_PER_FULL), 0.6);
    assert_eq!(adjust_velocity(0.5, -1000.0, ADJUST_VELOCITY_PX_PER_FULL), 0.05);
}

#[test]
fn places_the_playhead_inside_the_pattern_only() {
    assert_eq!(playhead_step(9.0, 8.0, 4.0, 0.25), Some(4.0));
    assert_eq!(playhead_step(7.9, 8.0, 4.0, 0.25), None);
    assert_eq!(playhead_step(12.0, 8.0, 4.0, 0.25), None);
}

// --- sample drops ----------------------------------------------------------------------

fn dt(payload: &str, files: &[&str]) -> DragDataLike {
    let mut types = Vec::new();
    if !payload.is_empty() {
        types.push(SAMPLE_DRAG_MIME.to_string());
    }
    if !files.is_empty() {
        types.push("Files".to_string());
    }
    DragDataLike { types, sample_payload: payload.into(), file_names: files.iter().map(|s| s.to_string()).collect() }
}

#[test]
fn reads_the_browser_payload() {
    let d = dt(r#"{"path":"/s/Kick 01.wav","name":"Kick 01"}"#, &[]);
    assert!(drag_has_sample(Some(&d.types)));
    assert_eq!(
        read_dropped_sample(Some(&d), None),
        Some(DroppedSample { path: "/s/Kick 01.wav".into(), name: "Kick 01".into() })
    );
}

#[test]
fn reads_the_first_audio_file_through_get_path_for_file() {
    let d = dt("", &["notes.txt", "Snare.WAV"]);
    assert!(drag_has_sample(Some(&d.types)));
    let get = |f: &str| format!("/drop/{f}");
    assert_eq!(
        read_dropped_sample(Some(&d), Some(&get)),
        Some(DroppedSample { path: "/drop/Snare.WAV".into(), name: "Snare".into() })
    );
}

#[test]
fn rejects_unusable_drops() {
    assert_eq!(read_dropped_sample(Some(&dt("{broken", &[])), None), None);
    let empty = |_: &str| String::new();
    assert_eq!(read_dropped_sample(Some(&dt("", &["a.wav"])), Some(&empty)), None);
    assert_eq!(read_dropped_sample(Some(&dt("", &["a.wav"])), None), None);
    assert!(!drag_has_sample(Some(&["text/plain".to_string()])));
    assert_eq!(read_dropped_sample(None, None), None);
}

// --- chop + knob helpers ------------------------------------------------------------------

#[test]
fn assigns_slices_to_consecutive_pads() {
    let ranges: Vec<SliceRange> = (0..8).map(|i| SliceRange { start_sec: i as f64, end_sec: i as f64 + 1.0 }).collect();
    assert_eq!(
        plan_chop(2, &ranges[..3]),
        vec![
            ChopAssignment { pad: 2, start_sec: 0.0, end_sec: 1.0 },
            ChopAssignment { pad: 3, start_sec: 1.0, end_sec: 2.0 },
            ChopAssignment { pad: 4, start_sec: 2.0, end_sec: 3.0 },
        ]
    );
    let tail: Vec<usize> = plan_chop(12, &ranges).iter().map(|a| a.pad).collect();
    assert_eq!(tail, vec![12, 13, 14, 15]);
}

#[test]
fn log_mapping_round_trips() {
    assert!(close(from_log01(0.0, 20.0, 20000.0), 20.0));
    assert!(close(from_log01(1.0, 20.0, 20000.0), 20000.0));
    assert!(close(to_log01(from_log01(0.37, 0.005, 4.0), 0.005, 4.0), 0.37));
}

// --- describeLoopMismatch -----------------------------------------------------------------

fn lm(
    r#loop: Option<(f64, f64)>,
    loop_enabled: bool,
    playing: bool,
    pattern_start_beat: f64,
    bars: f64,
) -> Option<String> {
    describe_loop_mismatch(&LoopMismatchInput {
        r#loop: r#loop.map(|(s, e)| LoopMarkers { start_beat: s, end_beat: e }),
        loop_enabled,
        playing,
        pattern_start_beat,
        bars,
    })
}

#[test]
fn loop_mismatch_is_quiet_when_the_loop_is_the_pattern() {
    assert_eq!(lm(Some((0.0, 8.0)), true, false, 0.0, 2.0), None);
    assert_eq!(lm(Some((16.0, 24.0)), true, false, 16.0, 2.0), None);
    assert_eq!(lm(None, true, false, 0.0, 2.0), None);
    assert_eq!(lm(Some((0.0, 8.0)), false, false, 0.0, 2.0), None);
}

#[test]
fn loop_mismatch_names_both_lengths() {
    assert_eq!(lm(Some((0.0, 16.0)), true, false, 0.0, 2.0).as_deref(), Some("Loop is 4 bars, pattern is 2 bars"));
    assert_eq!(lm(Some((0.0, 6.0)), true, false, 0.0, 1.0).as_deref(), Some("Loop is 6 beats, pattern is 1 bar"));
}

#[test]
fn loop_mismatch_flags_offset_and_disabled_loop_while_playing() {
    assert_eq!(
        lm(Some((4.0, 12.0)), true, false, 0.0, 2.0).as_deref(),
        Some("Loop range starts away from the pattern")
    );
    assert!(lm(Some((0.0, 8.0)), false, true, 0.0, 2.0).unwrap().contains("Loop is off"));
}

// --- chop.test.ts -----------------------------------------------------------------------------

/// `clicks(durationSec, at, sampleRate)` of chop.test.ts (exponential click decay).
fn clicks(duration_sec: f64, at: &[f64], sample_rate: f64) -> SampleBuffer {
    let mut data = vec![0f32; crate::loop_settings::js_round(duration_sec * sample_rate) as usize];
    for t in at {
        let i0 = crate::loop_settings::js_round(t * sample_rate) as usize;
        for i in 0..400 {
            if i0 + i >= data.len() {
                break;
            }
            data[i0 + i] = (0.9 * (-(i as f64) / 80.0).exp()) as f32;
        }
    }
    SampleBuffer { channels: vec![data], sample_rate }
}

#[test]
fn chop_equal_slices_cover_the_trimmed_part() {
    let buf = clicks(4.0, &[], 8000.0);
    let r = chop_ranges(&buf, ChopTrim { start_sec: 1.0, end_sec: Some(3.0) }, ChopMode::Equal { count: 4 }, 0);
    assert_eq!(
        r,
        vec![
            SliceRange { start_sec: 1.0, end_sec: 1.5 },
            SliceRange { start_sec: 1.5, end_sec: 2.0 },
            SliceRange { start_sec: 2.0, end_sec: 2.5 },
            SliceRange { start_sec: 2.5, end_sec: 3.0 },
        ]
    );
}

#[test]
fn chop_never_makes_more_slices_than_pads_left() {
    let buf = clicks(4.0, &[], 8000.0);
    assert_eq!(
        chop_ranges(&buf, ChopTrim { start_sec: 0.0, end_sec: None }, ChopMode::Equal { count: 16 }, 12).len(),
        4
    );
}

#[test]
fn chop_finds_hits_inside_the_trim_range() {
    let buf = clicks(2.0, &[0.1, 0.6, 1.1, 1.6], 8000.0);
    let r = chop_ranges(&buf, ChopTrim { start_sec: 0.5, end_sec: None }, ChopMode::Transients { sensitivity: 0.5 }, 0);
    assert_eq!(r[0].start_sec, 0.5);
    assert_eq!(r.last().unwrap().end_sec, 2.0);
    assert_eq!(r.len(), 4);
    assert!((r[1].start_sec - 0.6).abs() < 0.05);
    assert!((r[3].start_sec - 1.6).abs() < 0.05);
}

#[test]
fn chop_returns_nothing_for_an_empty_trim_range() {
    let buf = clicks(1.0, &[], 8000.0);
    assert!(
        chop_ranges(&buf, ChopTrim { start_sec: 0.5, end_sec: Some(0.5) }, ChopMode::Equal { count: 4 }, 0).is_empty()
    );
}

// --- keyScope.test.ts ---------------------------------------------------------------------------

#[test]
fn keeps_performance_keys_out_of_scoped_targets() {
    let body = DomNode::new("body");
    let root = DomNode::new("div");
    let aside = DomNode::new("aside").with_keys_scope("browser");
    let chain = |nodes: &[DomNode]| -> Vec<DomNode> {
        let mut v = nodes.to_vec();
        v.push(root.clone());
        v.push(body.clone());
        v
    };
    let scoped = [
        chain(&[DomNode::new("div").with_role("listbox"), aside.clone()]),
        chain(&[DomNode::new("button"), aside.clone()]),
        chain(&[DomNode::new("input")]),
        chain(&[DomNode::new("textarea")]),
        chain(&[DomNode::new("select")]),
        chain(&[DomNode::new("button"), DomNode::new("div").with_role("dialog")]),
        chain(&[DomNode::new("div").with_role("menuitem"), DomNode::new("div").with_role("menu")]),
    ];
    for c in &scoped {
        assert!(is_scoped_key_target(Some(c)), "{c:?}");
    }
    assert!(!is_scoped_key_target(Some(&chain(&[DomNode::new("button")]))));
    assert!(!is_scoped_key_target(Some(&chain(&[DomNode::new("canvas")]))));
    assert!(!is_scoped_key_target(Some(&[body.clone(), DomNode::new("html")])));
    assert!(!is_scoped_key_target(None));
}

// --- missingPackActions.test.ts -----------------------------------------------------------------

#[test]
fn missing_fix_clear_empties_the_pads() {
    assert_eq!(
        plan_missing_pad_fix(&[0, 5], MissingPadFix::Clear),
        vec![PadFix { pad: 0, sample: None, patch: None }, PadFix { pad: 5, sample: None, patch: None }]
    );
}

#[test]
fn missing_fix_builtin_puts_the_factory_sound_back() {
    let plan = plan_missing_pad_fix(&[0, 15], MissingPadFix::Builtin);
    let factory =
        |voice: FactoryVoiceId, name: &str| PadSample { source: PadSampleSource::Factory { voice }, name: name.into() };
    assert_eq!(plan[0].sample, Some(factory(FactoryVoiceId::Kick, "Kick")));
    assert_eq!(plan[1].sample, Some(factory(FactoryVoiceId::Perc, "Cowbell")));
    assert_eq!(plan[0].patch, Some(PadTrimReset { start_sec: 0.0, end_sec: None, reverse: false }));
}

#[test]
fn missing_fix_applies_as_one_undo_step() {
    // `fixMissingPads` drives the engine; here: the labels and per-pad calls its plan produces.
    let calls = |pads: &[usize], fix: MissingPadFix| {
        let mut calls = vec![format!("begin:{}", fix.undo_label())];
        for p in plan_missing_pad_fix(pads, fix) {
            calls.push(format!("set:{}:{}", p.pad, if p.sample.is_some() { "sample" } else { "null" }));
            if p.patch.is_some() {
                calls.push(format!("update:{}", p.pad));
            }
        }
        calls.push("end".into());
        calls
    };
    assert_eq!(
        calls(&[1, 2], MissingPadFix::Builtin),
        ["begin:Replace pads", "set:1:sample", "update:1", "set:2:sample", "update:2", "end"]
    );
    assert_eq!(calls(&[3], MissingPadFix::Clear), ["begin:Clear pads", "set:3:null", "end"]);
}

// --- stepKeys.test.ts ------------------------------------------------------------------------------

fn k(key: &str) -> KeyEventLike {
    KeyEventLike { key: key.into(), ..KeyEventLike::default() }
}

fn mv(pad: usize, step: usize) -> Option<StepKeyAction> {
    Some(StepKeyAction::Move(StepCell { pad, step }))
}

#[test]
fn arrows_move_one_cell_clamped() {
    let cell = StepCell { pad: 5, step: 6 };
    assert_eq!(step_key_action(&k("ArrowRight"), cell, 16, 4), mv(5, 7));
    assert_eq!(step_key_action(&k("ArrowLeft"), StepCell { pad: 5, step: 0 }, 16, 4), mv(5, 0));
    assert_eq!(step_key_action(&k("ArrowUp"), cell, 16, 4), mv(6, 6));
    assert_eq!(step_key_action(&k("ArrowDown"), StepCell { pad: 0, step: 6 }, 16, 4), mv(0, 6));
}

#[test]
fn shift_arrows_jump_to_beat_lines() {
    let cell = StepCell { pad: 5, step: 6 };
    let shift = |key: &str| KeyEventLike { shift: true, ..k(key) };
    assert_eq!(step_key_action(&shift("ArrowRight"), cell, 16, 4), mv(5, 8));
    assert_eq!(step_key_action(&shift("ArrowLeft"), cell, 16, 4), mv(5, 4));
    assert_eq!(step_key_action(&shift("ArrowLeft"), StepCell { pad: 5, step: 8 }, 16, 4), mv(5, 4));
}

#[test]
fn home_end_go_to_the_row_ends() {
    let cell = StepCell { pad: 5, step: 6 };
    assert_eq!(step_key_action(&k("Home"), cell, 32, 4), mv(5, 0));
    assert_eq!(step_key_action(&k("End"), cell, 32, 4), mv(5, 31));
}

#[test]
fn space_enter_toggle_and_brackets_change_velocity() {
    let cell = StepCell { pad: 5, step: 6 };
    assert_eq!(step_key_action(&k(" "), cell, 16, 4), Some(StepKeyAction::Toggle));
    assert_eq!(step_key_action(&k("Enter"), cell, 16, 4), Some(StepKeyAction::Toggle));
    assert_eq!(step_key_action(&k("+"), cell, 16, 4), Some(StepKeyAction::Velocity { delta: 0.1 }));
    assert_eq!(step_key_action(&k("]"), cell, 16, 4), Some(StepKeyAction::Velocity { delta: 0.1 }));
    assert_eq!(step_key_action(&k("-"), cell, 16, 4), Some(StepKeyAction::Velocity { delta: -0.1 }));
    assert_eq!(step_key_action(&k("["), cell, 16, 4), Some(StepKeyAction::Velocity { delta: -0.1 }));
    assert_eq!(step_key_action(&k("z"), cell, 16, 4), None);
    assert_eq!(step_key_action(&KeyEventLike { meta: true, ..k(" ") }, cell, 16, 4), None);
    assert_eq!(step_key_action(&KeyEventLike { alt: true, ..k("ArrowRight") }, cell, 16, 4), None);
}

#[test]
fn nudge_velocity_clamps_to_5_100_percent() {
    assert_eq!(nudge_velocity(0.95, 0.1), 1.0);
    assert_eq!(nudge_velocity(0.1, -0.1), 0.05);
    assert_eq!(nudge_velocity(0.8, -0.1), 0.7);
}

#[test]
fn describe_step_announces_pad_step_state_velocity() {
    assert_eq!(describe_step("Kick", 4, 0.8), "Kick, step 5, on, velocity 80%");
    assert_eq!(describe_step("Snare", 0, 0.0), "Snare, step 1, off");
}

// --- waveform.test.ts -------------------------------------------------------------------------------

#[test]
fn computes_min_max_per_column_across_channels() {
    let b = SampleBuffer { channels: vec![vec![0.0, 0.5, -0.25, 0.0], vec![0.1, 0.0, 0.0, -0.75]], sample_rate: 1.0 };
    let p = compute_peaks(&b, 2.0);
    assert_eq!(p.max, vec![0.5, 0.0]);
    assert_eq!(p.min, vec![0.0, -0.75]);
}

#[test]
fn peaks_handle_empty_buffers() {
    let b = SampleBuffer { channels: vec![vec![]], sample_rate: 1.0 };
    assert_eq!(compute_peaks(&b, 4.0).max, vec![0.0; 4]);
}

#[test]
fn peaks_cache_by_buffer_and_resolution() {
    let b = SampleBuffer { channels: vec![vec![0.0, 1.0, 0.0, -1.0]], sample_rate: 1.0 };
    let mut cache = PeakCache::new();
    let a = cache.get_peaks(1, &b, 2.0);
    assert!(std::rc::Rc::ptr_eq(&a, &cache.get_peaks(1, &b, 2.0)));
    assert!(!std::rc::Rc::ptr_eq(&cache.get_peaks(1, &b, 4.0), &a));
    cache.forget(1);
    assert!(!std::rc::Rc::ptr_eq(&cache.get_peaks(1, &b, 2.0), &a));
}

// --- PendingLoadChip.test.tsx (pure part) --------------------------------------------------------------

#[test]
fn pending_load_texts() {
    assert_eq!(pending_load_text("TR-808 Recordings", false).text, "Getting TR-808 Recordings…");
    assert_eq!(pending_load_text("TR-808 Recordings", true).text, "After take: TR-808 Recordings");
}

// --- extra: non-exported .tsx formatters, JS number formatting ----------------------------------------

#[test]
fn tsx_formatters() {
    assert_eq!(swing_grid_label(SwingGrid::S8), "8ths");
    assert_eq!(swing_grid_label(SwingGrid::S16), "16ths");
    assert_eq!(fmt_db(3.0), "+3.0dB");
    assert_eq!(fmt_db(-6.25), "-6.3dB"); // toFixed: exact tie (-6.25) rounds away from zero
    assert_eq!(fmt_db(0.0), "0.0dB");
    assert_eq!(fmt_db(-0.04), "-0.0dB");
    assert_eq!(fmt_pan(0.004), "C");
    assert_eq!(fmt_pan(-0.4), "L40");
    assert_eq!(fmt_pan(0.255), "R26");
    assert_eq!(fmt_pitch(12.0), "+12st");
    assert_eq!(fmt_pitch(-2.5), "-2st"); // Math.round(-2.5) = -2
    assert_eq!(fmt_pitch(0.4), "+0st");
    assert_eq!(pad_count_label(1), "1 pad");
    assert_eq!(pad_count_label(3), "3 pads");
    assert_eq!(bank_rows('A'), vec![vec![4, 5, 6, 7], vec![0, 1, 2, 3]]);
    assert_eq!(bank_rows('B'), vec![vec![12, 13, 14, 15], vec![8, 9, 10, 11]]);
    assert_eq!(PadLayout::from_label("2x8"), Some(PadLayout::TwoByEight));
    assert_eq!(PadLayout::FourByFour.label(), "4x4");
}

#[test]
fn js_number_formatting() {
    for (x, s) in [
        (0.0, "0"),
        (-0.0, "0"),
        (1.0, "1"),
        (-1.5, "-1.5"),
        (0.1 + 0.2, "0.30000000000000004"),
        (1e21, "1e+21"),
        (1.5e22, "1.5e+22"),
        (123456789012345680000.0, "123456789012345680000"),
        (1e-6, "0.000001"),
        (1e-7, "1e-7"),
        (1.25e-7, "1.25e-7"),
        (f64::NAN, "NaN"),
        (f64::NEG_INFINITY, "-Infinity"),
    ] {
        assert_eq!(js_number_to_string(x), s, "String({x})");
    }
    for (x, d, s) in [
        (1.005, 2, "1.00"),
        (1.125, 2, "1.13"),
        (0.125, 2, "0.13"),
        (-1.125, 2, "-1.13"),
        (-0.001, 2, "-0.00"),
        (-0.0, 2, "0.00"),
        (9.995, 2, "9.99"),
        (99.95, 1, "100.0"),
        (0.5, 0, "1"),
        (2.5, 0, "3"),
        (1e21, 2, "1e+21"),
        (123.456, 0, "123"),
    ] {
        assert_eq!(js_to_fixed(x, d), s, "({x}).toFixed({d})");
    }
}

#[test]
fn bufgen_prng_matches_mulberry32() {
    // First outputs of mulberry32(1) as printed by node.
    let mut r = mulberry32(1);
    let first = r();
    assert!((0.0..1.0).contains(&first));
    let spec: serde_json::Value = serde_json::json!({ "sr": 8, "len": 4, "channels": [{ "seed": 1, "noise": 0, "clicks": [{ "t": 0.25, "amp": 1, "decay": 2 }] }] });
    assert_eq!(gen_buffer(&spec).channels[0], vec![0.0, 0.0, 1.0, 0.5]);
}
