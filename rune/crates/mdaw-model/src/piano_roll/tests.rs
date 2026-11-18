//! Port of `src/utils/pianoRollHelpers.test.ts` (same cases, same assertions).

use super::*;
use crate::types::RegionNote;

fn n(note: f64, start_beat: f64) -> RegionNote {
    nd(note, start_beat, 1.0)
}
fn nd(note: f64, start_beat: f64, duration_beats: f64) -> RegionNote {
    nv(note, start_beat, duration_beats, 0.8)
}
fn nv(note: f64, start_beat: f64, duration_beats: f64, velocity: f64) -> RegionNote {
    RegionNote {
        note,
        start_beat,
        duration_beats,
        velocity,
        is_drum: false,
    }
}

fn piano() -> RowLayout {
    create_piano_layout_default()
}

fn close(a: f64, b: f64, digits: i32) -> bool {
    // vitest toBeCloseTo: |a - b| < 10^-digits / 2
    (a - b).abs() < 10f64.powi(-digits) / 2.0
}

const NO_HIT: HitTestOptions<'static> = HitTestOptions {
    edge_px: 6.0,
    min_width_px: 4.0,
    resizable: true,
    selected: None,
};

// ----- layouts -----

#[test]
fn layouts_maps_pitches_to_rows_with_the_highest_pitch_on_top() {
    let piano = piano();
    assert_eq!(piano.row_count, 128);
    assert_eq!(piano.pitch_to_row(127.0), 0.0);
    assert_eq!(piano.pitch_to_row(0.0), 127.0);
    assert_eq!(piano.row_to_pitch(piano.pitch_to_row(60.0)), 60.0);
    assert_eq!(piano.pitch_to_row(128.0), -1.0);
    assert_eq!(piano.row_label(piano.pitch_to_row(60.0) as i64), "C4");
    assert_eq!(piano.row_to_pitch(-5.0), 127.0); // clamped
}

#[test]
fn layouts_shades_black_key_rows() {
    let piano = piano();
    assert!(is_shaded_row(&piano, piano.pitch_to_row(61.0)));
    assert!(!is_shaded_row(&piano, piano.pitch_to_row(60.0)));
}

#[test]
fn layouts_drum_layout_lists_gm_pads_first_and_adds_rows_for_unknown_pitches() {
    let drums = create_drum_layout(&[36.0, 37.0, 38.0]);
    assert_eq!(drums.row_to_pitch(0.0), 36.0);
    assert_eq!(drums.row_label(0), "Kick");
    assert_eq!(drums.row_count, 9); // 8 pads + note 37
    assert_eq!(drums.pitch_to_row(37.0), 8.0);
    assert_eq!(drums.row_label(8), "C#2");
    assert_eq!(drums.pitch_to_row(99.0), -1.0);
}

// ----- snapping -----

#[test]
fn snapping_snaps_to_nearest_floor_ceil_grid_lines() {
    assert_eq!(snap_beat(0.3, 0.25, 0.0), 0.25);
    assert_eq!(snap_beat(0.4, 0.25, 0.0), 0.5);
    assert_eq!(snap_floor(0.49, 0.25, 0.0), 0.25);
    assert_eq!(snap_ceil(0.26, 0.25, 0.0), 0.5);
    assert_eq!(snap_ceil(0.5, 0.25, 0.0), 0.5);
}

#[test]
fn snapping_snap_off_only_rounds_to_ticks() {
    assert_eq!(snap_beat(0.1 + 0.2, 0.0, 0.0), 0.3);
}

#[test]
fn snapping_respects_the_region_offset_so_the_grid_follows_song_bars() {
    assert_eq!(snap_beat(1.4, 4.0, 2.0), 2.0);
    assert_eq!(snap_beat(0.9, 4.0, 2.0), 2.0);
    assert_eq!(snap_beat(-0.5, 4.0, 2.0), -2.0);
}

#[test]
fn snapping_handles_triplets_without_float_drift() {
    let t = 1.0 / 3.0;
    assert!(close(snap_beat(0.34, t, 0.0), 1.0 / 3.0, 3));
    assert_eq!(snap_beat(0.9, t, 0.0), 1.0);
    assert_eq!(round_beat(0.1 + 0.2), 0.3);
}

// ----- formatting -----

#[test]
fn formatting_formats_lengths() {
    assert_eq!(format_beats(0.25), "1/16");
    assert_eq!(format_beats(0.75), "1/8 .");
    assert_eq!(format_beats(1.0 / 3.0), "1/8 T");
    assert_eq!(format_beats(8.0), "2 bars");
    assert_eq!(format_beats(0.625), "5/32");
}

#[test]
fn formatting_formats_positions_as_bar_beat_sixteenth() {
    assert_eq!(format_position(0.0), "1.1.1");
    assert_eq!(format_position(5.25), "2.2.2");
}

#[test]
fn formatting_converts_velocity_for_display() {
    assert_eq!(velocity_to_midi(1.0), 127.0);
    assert_eq!(velocity_to_midi(0.0), 1.0);
}

// ----- grid generation -----

#[test]
fn grid_produces_bar_beat_and_subdivision_levels() {
    let lines = compute_grid_lines(0.0, 4.0, 64.0, 0.25, 0.0, 6.0);
    assert_eq!(lines.len(), 17);
    assert_eq!(
        lines[0],
        GridLine {
            beat: 0.0,
            level: 0
        }
    );
    assert_eq!(
        lines[1],
        GridLine {
            beat: 0.25,
            level: 2
        }
    );
    assert_eq!(
        lines[4],
        GridLine {
            beat: 1.0,
            level: 1
        }
    );
    assert_eq!(
        lines[16],
        GridLine {
            beat: 4.0,
            level: 0
        }
    );
}

#[test]
fn grid_coarsens_subdivisions_that_would_be_too_dense() {
    assert_eq!(grid_subdivision(0.125, 8.0, 6.0), 1.0);
    let lines = compute_grid_lines(0.0, 8.0, 8.0, 0.125, 0.0, 6.0);
    assert!(lines.iter().all(|l| l.level < 2));
}

#[test]
fn grid_includes_triplet_subdivisions_and_keeps_beat_lines() {
    let lines = compute_grid_lines(0.0, 1.0, 120.0, 1.0 / 3.0, 0.0, 6.0);
    assert_eq!(
        lines.iter().map(|l| l.level).collect::<Vec<_>>(),
        vec![0, 2, 2, 1]
    );
}

#[test]
fn grid_thins_out_bars_when_zoomed_far_out() {
    let lines = compute_grid_lines(0.0, 64.0, 1.0, 0.25, 0.0, 6.0);
    assert!(lines.iter().all(|l| l.level == 0));
    assert!(lines.len() < 17);
}

#[test]
fn grid_labels_bars_using_absolute_song_positions() {
    let labels = compute_ruler_labels(0.0, 8.0, 12.0, 4.0, 44.0);
    assert_eq!(
        labels.iter().map(|l| l.label.as_str()).collect::<Vec<_>>(),
        vec!["2", "3", "4"]
    );
    let fine = compute_ruler_labels(0.0, 1.0, 60.0, 0.0, 44.0);
    assert_eq!(
        fine.iter().map(|l| l.label.as_str()).collect::<Vec<_>>(),
        vec!["1", "1.2"]
    );
}

// ----- hit-testing and selection -----

fn hit_notes() -> Vec<RegionNote> {
    vec![nd(60.0, 0.0, 1.0), nd(60.0, 2.0, 1.0), nd(64.0, 0.0, 4.0)]
}
const PX: f64 = 40.0;
const RH: f64 = 10.0;
fn y_of(pitch: f64) -> f64 {
    piano().pitch_to_row(pitch) * RH + 5.0
}

#[test]
fn hit_finds_the_note_body_and_edges() {
    let (notes, piano) = (hit_notes(), piano());
    let hit = |x: f64, p: f64| hit_test_note(&notes, x, y_of(p), &piano, PX, RH, &NO_HIT);
    assert_eq!(
        hit(20.0, 60.0),
        Some(NoteHit {
            index: 0,
            edge: NoteEdge::Body
        })
    );
    assert_eq!(
        hit(38.0, 60.0),
        Some(NoteHit {
            index: 0,
            edge: NoteEdge::End
        })
    );
    assert_eq!(
        hit(81.0, 60.0),
        Some(NoteHit {
            index: 1,
            edge: NoteEdge::Start
        })
    );
    assert_eq!(hit(60.0, 60.0), None);
    assert_eq!(hit(20.0, 62.0), None);
}

#[test]
fn hit_never_reports_edges_when_not_resizable() {
    let (notes, piano) = (hit_notes(), piano());
    let opts = HitTestOptions {
        resizable: false,
        ..Default::default()
    };
    assert_eq!(
        hit_test_note(&notes, 38.0, y_of(60.0), &piano, PX, RH, &opts).map(|h| h.edge),
        Some(NoteEdge::Body)
    );
}

#[test]
fn hit_prefers_selected_notes_when_overlapping() {
    let piano = piano();
    let stacked = [nd(60.0, 0.0, 2.0), nd(60.0, 0.0, 2.0)];
    assert_eq!(
        hit_test_note(&stacked, 20.0, y_of(60.0), &piano, PX, RH, &NO_HIT).map(|h| h.index),
        Some(1)
    );
    let sel = [0];
    let opts = HitTestOptions {
        selected: Some(&sel),
        ..Default::default()
    };
    assert_eq!(
        hit_test_note(&stacked, 20.0, y_of(60.0), &piano, PX, RH, &opts).map(|h| h.index),
        Some(0)
    );
}

#[test]
fn hit_keeps_very_short_notes_grabbable() {
    let tiny = [nd(60.0, 1.0, 0.01)];
    assert_eq!(
        hit_test_note(&tiny, 42.0, y_of(60.0), &piano(), PX, RH, &NO_HIT).map(|h| h.index),
        Some(0)
    );
}

#[test]
fn hit_selects_notes_inside_a_rubber_band() {
    let (notes, piano) = (hit_notes(), piano());
    let r60 = piano.pitch_to_row(60.0);
    let r64 = piano.pitch_to_row(64.0);
    assert_eq!(notes_in_rect(&notes, 0.5, 1.5, r60, r60, &piano), vec![0]);
    assert_eq!(
        notes_in_rect(&notes, 0.5, 2.5, r64, r60, &piano),
        vec![0, 1, 2]
    );
}

#[test]
fn hit_click_selection_semantics() {
    assert_eq!(apply_click_selection(&[1, 2], 3, false), vec![3]);
    assert_eq!(apply_click_selection(&[1, 2], 2, false), vec![1, 2]);
    assert_eq!(apply_click_selection(&[1, 2], 2, true), vec![1]);
    assert_eq!(apply_click_selection(&[1], 2, true), vec![1, 2]);
}

// ----- move / transpose -----

#[test]
fn move_moves_selected_notes_by_beats_and_rows() {
    let notes = [n(60.0, 1.0), n(62.0, 2.0), n(64.0, 3.0)];
    let out = move_notes(&notes, &[0, 1], 1.0, -2.0, &piano());
    assert_eq!((out[0].note, out[0].start_beat), (62.0, 2.0));
    assert_eq!((out[1].note, out[1].start_beat), (64.0, 3.0));
    assert_eq!(out[2], notes[2]);
}

#[test]
fn move_clamps_so_notes_never_go_before_0_or_out_of_range() {
    let piano = piano();
    let notes = [n(126.0, 0.5), n(60.0, 2.0)];
    assert_eq!(
        clamp_move_delta(&notes, &[0, 1], -3.0, -5.0, &piano),
        MoveDelta {
            d_beat: -0.5,
            d_row: -1.0
        }
    );
    let out = move_notes(&notes, &[0, 1], -3.0, -5.0, &piano);
    assert_eq!(
        out.iter()
            .map(|x| (x.note, x.start_beat))
            .collect::<Vec<_>>(),
        vec![(127.0, 0.0), (61.0, 1.5)]
    );
}

#[test]
fn move_snaps_the_grabbed_note_onto_the_grid() {
    assert_eq!(snapped_move_delta(0.1, 0.5, 0.25, 0.0), 0.4);
    assert_eq!(snapped_move_delta(0.1, 0.5, 0.0, 0.0), 0.5);
}

#[test]
fn move_transposes_by_semitones_octaves() {
    let notes = [n(60.0, 0.0)];
    assert_eq!(transpose_notes(&notes, &[0], 12.0, &piano())[0].note, 72.0);
    assert_eq!(transpose_notes(&notes, &[0], -1.0, &piano())[0].note, 59.0);
}

#[test]
fn move_moves_drum_notes_between_pad_rows() {
    let drums = create_drum_layout(&[]);
    let notes = [RegionNote {
        is_drum: true,
        ..nd(36.0, 0.0, 0.25)
    }];
    assert_eq!(move_notes(&notes, &[0], 0.0, 1.0, &drums)[0].note, 38.0);
    assert_eq!(move_notes(&notes, &[0], 0.0, -1.0, &drums)[0].note, 36.0); // clamped at top
}

// ----- resize / draw / length -----

#[test]
fn resize_from_the_end_snapping_the_anchor_edge() {
    let notes = [nd(60.0, 0.0, 1.0), nd(62.0, 1.0, 0.5)];
    let out = resize_notes(&notes, &[0, 1], NoteEdge::End, 0, 0.6, 0.25, 0.0);
    assert_eq!(out[0].duration_beats, 1.5);
    assert_eq!(out[1].duration_beats, 1.0);
}

#[test]
fn resize_from_the_start_keeping_the_end_fixed() {
    let notes = [nd(60.0, 1.0, 1.0)];
    let out = resize_notes(&notes, &[0], NoteEdge::Start, 0, -0.4, 0.25, 0.0);
    assert_eq!((out[0].start_beat, out[0].duration_beats), (0.5, 1.5));
    let shrunk = resize_notes(&notes, &[0], NoteEdge::Start, 0, 5.0, 0.25, 0.0);
    assert_eq!(
        (shrunk[0].start_beat, shrunk[0].duration_beats),
        (1.75, 0.25)
    );
}

#[test]
fn resize_does_not_stretch_short_notes_up_to_the_grid_just_by_grabbing_them() {
    let notes = [nd(60.0, 0.0, 0.1)];
    assert!(close(
        resize_notes(&notes, &[0], NoteEdge::End, 0, 0.0, 0.25, 0.0)[0].duration_beats,
        0.1,
        2
    ));
    assert_eq!(
        resize_notes(&notes, &[0], NoteEdge::End, 0, -1.0, 0.0, 0.0)[0].duration_beats,
        MIN_NOTE_LENGTH
    );
}

#[test]
fn draw_span_extends_to_the_nearest_grid_line_min_one_step() {
    assert_eq!(
        draw_span(1.0, 1.05, 0.25, 0.0),
        DrawSpan {
            start_beat: 1.0,
            duration_beats: 0.25
        }
    );
    assert_eq!(
        draw_span(1.0, 2.4, 0.25, 0.0),
        DrawSpan {
            start_beat: 1.0,
            duration_beats: 1.5
        }
    );
    assert_eq!(
        draw_span(1.0, 0.3, 0.25, 0.0),
        DrawSpan {
            start_beat: 0.25,
            duration_beats: 1.0
        }
    );
}

#[test]
fn changes_lengths_by_a_step() {
    assert_eq!(
        change_lengths(&[nd(60.0, 0.0, 1.0)], &[0], -2.0, MIN_NOTE_LENGTH)[0].duration_beats,
        MIN_NOTE_LENGTH
    );
    assert_eq!(
        change_lengths(&[nd(60.0, 0.0, 1.0)], &[0], 0.25, MIN_NOTE_LENGTH)[0].duration_beats,
        1.25
    );
}

// ----- delete / duplicate / clipboard -----

fn clip_notes() -> Vec<RegionNote> {
    vec![nd(60.0, 0.0, 1.0), nd(64.0, 1.0, 1.0), nd(67.0, 2.0, 0.5)]
}

#[test]
fn clipboard_deletes() {
    assert_eq!(
        delete_notes(&clip_notes(), &[0, 2])
            .iter()
            .map(|x| x.note)
            .collect::<Vec<_>>(),
        vec![64.0]
    );
}

#[test]
fn clipboard_duplicates_the_selection_after_itself_grid_aligned() {
    let notes = clip_notes();
    let NotesEdit {
        notes: out,
        selection,
    } = duplicate_notes(&notes, &[0, 1], 0.25);
    assert_eq!(out.len(), 5);
    assert_eq!(selection, vec![3, 4]);
    assert_eq!((out[3].note, out[3].start_beat), (60.0, 2.0));
    assert_eq!((out[4].note, out[4].start_beat), (64.0, 3.0));
    // span 2.5 beats with a bar grid -> shift by a whole bar
    assert_eq!(
        duplicate_notes(&notes, &[0, 1, 2], 4.0).notes[3].start_beat,
        4.0
    );
}

#[test]
fn clipboard_appends_shifted_copies_alt_drag_duplicate() {
    let notes = clip_notes();
    let piano = piano();
    let NotesEdit {
        notes: out,
        selection,
    } = append_copies(&notes, &[2], 1.0, -12.0, Some(&piano));
    assert_eq!(selection, vec![3]);
    assert_eq!((out[3].note, out[3].start_beat), (79.0, 3.0));
    assert_eq!(out[2], notes[2]);
}

#[test]
fn clipboard_copy_normalises_and_paste_places_at_a_beat() {
    let notes = clip_notes();
    let clip = copy_notes(&notes, &[1, 2]);
    assert_eq!(
        clip.iter().map(|x| x.start_beat).collect::<Vec<_>>(),
        vec![0.0, 1.0]
    );
    let NotesEdit {
        notes: out,
        selection,
    } = paste_notes(&notes, &clip, 8.0, None);
    assert_eq!(selection, vec![3, 4]);
    assert_eq!(out[3].start_beat, 8.0);
    assert_eq!(out[4].start_beat, 9.0);
    assert!(
        paste_notes(&[], &clip, 0.0, Some(true))
            .notes
            .iter()
            .all(|x| x.is_drum)
    );
}

// ----- quantize -----

fn q_notes() -> Vec<RegionNote> {
    vec![nd(60.0, 0.1, 0.9), nd(62.0, 1.13, 0.3)]
}

#[test]
fn quantize_starts_of_all_notes_legacy_signature() {
    let notes = q_notes();
    let d = QuantizeOptions::default();
    assert_eq!(
        quantize_notes(&notes, 0.25, &d)
            .iter()
            .map(|x| x.start_beat)
            .collect::<Vec<_>>(),
        vec![0.0, 1.25]
    );
    assert_eq!(quantize_notes(&notes, 0.0, &d), notes);
}

#[test]
fn quantize_only_selected_notes_optionally_ends() {
    let notes = q_notes();
    let ix = [1];
    let out = quantize_notes(
        &notes,
        0.25,
        &QuantizeOptions {
            indices: Some(&ix),
            ends: true,
            ..Default::default()
        },
    );
    assert_eq!(out[0], notes[0]);
    assert_eq!((out[1].start_beat, out[1].duration_beats), (1.25, 0.25));
}

#[test]
fn quantize_supports_partial_strength() {
    let out = quantize_notes(
        &[n(60.0, 0.2)],
        1.0,
        &QuantizeOptions {
            strength: 0.5,
            ..Default::default()
        },
    );
    assert_eq!(out[0].start_beat, 0.1);
}

// ----- velocity -----

fn v_notes() -> Vec<RegionNote> {
    vec![
        nv(60.0, 0.0, 1.0, 0.5),
        nv(62.0, 1.0, 1.0, 0.5),
        nv(64.0, 2.0, 1.0, 0.5),
        nv(65.0, 3.0, 1.0, 0.5),
    ]
}

#[test]
fn velocity_sets_and_offsets_with_clamping() {
    let notes = v_notes();
    assert_eq!(set_velocities(&notes, &[0], 2.0)[0].velocity, 1.0);
    assert!(
        offset_velocities(&notes, &[0, 1], -1.0)[..2]
            .iter()
            .all(|x| x.velocity > 0.0)
    );
    assert_eq!(offset_velocities(&notes, &[2], 0.25)[2].velocity, 0.75);
}

#[test]
fn velocity_ramps_linearly_across_the_dragged_range() {
    let notes = v_notes();
    let out = ramp_velocities(&notes, None, 0.0, 0.2, 3.0, 0.8);
    let rounded: Vec<f64> = out
        .iter()
        .map(|x| format!("{:.2}", x.velocity).parse().unwrap())
        .collect();
    assert_eq!(rounded, vec![0.2, 0.4, 0.6, 0.8]);
    let only_some = ramp_velocities(&notes, Some(&[1]), 0.0, 0.2, 3.0, 0.8);
    assert_eq!(only_some[0].velocity, 0.5);
    assert!(close(only_some[1].velocity, 0.4, 2));
}

#[test]
fn velocity_finds_stems_at_x() {
    let notes = v_notes();
    assert_eq!(
        notes_at_x(&notes, 41.0, 40.0, NOTES_AT_X_TOL_PX, None),
        vec![1]
    );
    assert_eq!(
        notes_at_x(&notes, 41.0, 40.0, 5.0, Some(&[0])),
        Vec::<usize>::new()
    );
}

// ----- view helpers -----

#[test]
fn view_extends_regions_to_whole_bars_never_shrinks() {
    assert_eq!(required_region_length(&[nd(60.0, 7.0, 2.0)], 8.0), 12.0);
    assert_eq!(required_region_length(&[nd(60.0, 0.0, 1.0)], 8.0), 8.0);
}

#[test]
fn view_zooms_around_the_cursor() {
    assert_eq!(zoom_scroll(100.0, 50.0, 10.0, 20.0), 250.0);
    assert!(zoom_from_wheel(100.0, -100.0, 10.0, 120.0, ZOOM_WHEEL_SENSITIVITY) > 100.0);
    assert_eq!(
        zoom_from_wheel(100.0, 10000.0, 10.0, 120.0, ZOOM_WHEEL_SENSITIVITY),
        10.0
    );
    assert!(close(
        slider_to_zoom(zoom_to_slider(64.0, 6.0, 640.0), 6.0, 640.0),
        64.0,
        2
    ));
}

#[test]
fn view_centres_the_notes_vertically_on_open() {
    let piano = piano();
    let notes = [n(60.0, 0.0), n(72.0, 1.0)];
    let top = center_scroll_top(&notes, &piano, 10.0, 200.0);
    let center_row = (top + 100.0) / 10.0;
    assert!(center_row > piano.pitch_to_row(72.0));
    assert!(center_row < piano.pitch_to_row(60.0) + 1.0);
    assert_eq!(
        center_scroll_top(&[], &create_drum_layout(&[]), 10.0, 200.0),
        0.0
    );
}

#[test]
fn view_fits_the_region_into_the_viewport() {
    let fit = fit_view(
        &[nd(60.0, 0.0, 1.0), nd(72.0, 15.0, 1.0)],
        &piano(),
        16.0,
        816.0,
        340.0,
    );
    assert_eq!(fit.px_per_beat, 50.0);
    assert_eq!(fit.row_height, 20.0);
}

#[test]
fn view_follows_the_playhead_page_by_page() {
    assert_eq!(follow_scroll(500.0, 0.0, 1000.0, FOLLOW_MARGIN), None);
    assert_eq!(
        follow_scroll(990.0, 0.0, 1000.0, FOLLOW_MARGIN),
        Some(890.0)
    );
    assert_eq!(follow_scroll(10.0, 400.0, 1000.0, FOLLOW_MARGIN), Some(0.0));
}
