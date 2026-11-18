//! `inventory/views.md` §6 — PianoRollEditor, PianoRollToolbar, NoteGrid, PianoRollRuler,
//! VelocityLane, PianoKeyColumn / DrumKeyColumn — and §7 on the Song dock.

use mdaw_model::RegionNote;
use mdaw_model::piano_roll::{PianoRollTool, SNAP_OPTIONS};
use rune::Key;

use super::harness::*;
use super::t_song_arr::texts_of;

type Rt = rune::Runtime<crate::app::App>;

/// The Bass region open in the piano roll, laid out and fitted.
fn bass_rt() -> (Rt, String, String) {
    let mut rt = song_rt();
    let b = tid(&rt, "Bass");
    let r = rid(&rt, &b, 0);
    rt.state.open_piano_roll(&b, &r);
    rt.invalidate();
    run(&mut rt, 0.6);
    (rt, b, r)
}

fn notes(rt: &Rt, t: &str, r: &str) -> Vec<RegionNote> {
    region(rt, t, r).notes
}

/// Window point of (region-relative beat, pitch) in the grid.
fn grid_pt(rt: &Rt, beat: f64, pitch: f64) -> (f32, f32) {
    let g = rect(rt, "pr-grid");
    let vp = rt.state.song.piano.vp;
    let c = rt.state.piano_ctx().unwrap();
    let row = c.layout.pitch_to_row(pitch);
    (
        g[0] + (beat * vp.ppb - vp.left) as f32,
        g[1] + ((row + 0.5) * vp.row_h - vp.top) as f32,
    )
}

#[test]
fn open_fits_and_closes_with_back_or_escape() {
    let (mut rt, _b, _r) = bass_rt();
    let vp = rt.state.song.piano.vp;
    assert!(rt.state.song.piano.did_fit);
    assert!(
        (12.0..=22.0).contains(&vp.row_h),
        "piano rows clamped 12–22 by fit: {}",
        vp.row_h
    );
    assert_eq!(texts_of(&rt, "pr-region-name"), "Bass Riff");
    assert_eq!(texts_of(&rt, "pr-count"), "10 notes");
    assert!(texts(&rt).contains(&"grid 1/16".to_owned()));
    for t in ["Vel", "127", "64", "1"] {
        assert!(texts(&rt).contains(&t.to_owned()), "{t}");
    }
    click(&mut rt, "pr-back");
    assert_eq!(rt.state.song.editing, None);
    assert!(has(&rt, "arrangement"));
    let b = tid(&rt, "Bass");
    let r = rid(&rt, &b, 0);
    rt.state.open_piano_roll(&b, &r);
    run(&mut rt, 0.3);
    press(&mut rt, ev(Key::Escape, rune::Mods::default()));
    assert_eq!(rt.state.song.editing, None);
}

#[test]
fn a_region_that_is_gone_offers_the_way_back() {
    let (mut rt, b, r) = bass_rt();
    rt.state.remove_region(&b, &r);
    rt.invalidate();
    frames(&mut rt, 3);
    assert!(texts(&rt).contains(&"This region no longer exists.".to_owned()));
    click(&mut rt, "piano-roll-back-link");
    assert_eq!(rt.state.song.editing, None);
}

#[test]
fn settings_persist_and_the_clipboard_survives_reopening() {
    let (mut rt, b, r) = bass_rt();
    rt.state.set_pr_snap(0.5);
    rt.state.set_pr_follow(false);
    let raw = rt
        .state
        .svc
        .settings
        .borrow()
        .get_item("micro-daw-piano-roll")
        .map(str::to_owned)
        .unwrap();
    assert_eq!(raw, r#"{"snap":0.5,"drawLength":1,"follow":false}"#);
    press(&mut rt, ev(Key::Command('a'), cmd()));
    press(&mut rt, ev(Key::Command('c'), cmd()));
    rt.state.close_piano_roll();
    rt.state.open_piano_roll(&b, &r);
    assert_eq!(rt.state.song.piano.prefs.snap, 0.5);
    assert!(!rt.state.song.piano.prefs.follow);
    assert_eq!(rt.state.song.piano.clipboard.len(), 10);
}

#[test]
fn keys_select_copy_cut_paste_duplicate_delete_transpose_nudge() {
    let (mut rt, b, r) = bass_rt();
    let n0 = notes(&rt, &b, &r);
    press(&mut rt, ev(Key::Command('a'), cmd()));
    assert_eq!(rt.state.song.piano.selection.len(), 10);
    assert_eq!(texts_of(&rt, "pr-count"), "10/10 sel");
    // Transpose: ↑ one semitone, Shift+↑ an octave.
    press(&mut rt, ev(Key::Up, rune::Mods::default()));
    assert_eq!(notes(&rt, &b, &r)[0].note, n0[0].note + 1.0);
    assert_eq!(undo_label(&rt).as_deref(), Some("Transpose Notes"));
    press(&mut rt, ev(Key::Up, shift()));
    assert_eq!(notes(&rt, &b, &r)[0].note, n0[0].note + 13.0);
    // Nudge by the grid step; Alt changes lengths.
    press(&mut rt, ev(Key::Right, rune::Mods::default()));
    assert_eq!(notes(&rt, &b, &r)[0].start_beat, n0[0].start_beat + 0.25);
    assert_eq!(undo_label(&rt).as_deref(), Some("Move Notes"));
    press(&mut rt, ev(Key::Right, alt()));
    assert_eq!(
        notes(&rt, &b, &r)[0].duration_beats,
        n0[0].duration_beats + 0.25
    );
    assert_eq!(undo_label(&rt).as_deref(), Some("Change Length"));
    // Duplicate, cut, paste.
    press(&mut rt, ev(Key::Command('d'), cmd()));
    assert_eq!(notes(&rt, &b, &r).len(), 20);
    assert_eq!(undo_label(&rt).as_deref(), Some("Duplicate Notes"));
    press(&mut rt, ev(Key::Command('x'), cmd()));
    assert_eq!(notes(&rt, &b, &r).len(), 10);
    assert_eq!(undo_label(&rt).as_deref(), Some("Cut Notes"));
    press(&mut rt, ev(Key::Command('v'), cmd()));
    assert_eq!(notes(&rt, &b, &r).len(), 20);
    assert_eq!(undo_label(&rt).as_deref(), Some("Paste Notes"));
    press(&mut rt, ev(Key::Delete, rune::Mods::default()));
    assert_eq!(notes(&rt, &b, &r).len(), 10);
    assert_eq!(undo_label(&rt).as_deref(), Some("Delete Notes"));
    // Undo from outside clears the selection.
    rt.state.song.piano.selection = vec![0];
    rt.state.undo();
    run(&mut rt, 0.1);
    assert!(rt.state.song.piano.selection.is_empty());
}

#[test]
fn tool_keys_quantize_fit_zoom_and_space() {
    let (mut rt, b, r) = bass_rt();
    press(&mut rt, text_key("d"));
    assert_eq!(rt.state.song.piano.tool, PianoRollTool::Draw);
    press(&mut rt, text_key("e"));
    assert_eq!(rt.state.song.piano.tool, PianoRollTool::Eraser);
    press(&mut rt, text_key("b"));
    assert_eq!(rt.state.song.piano.tool, PianoRollTool::Draw);
    press(&mut rt, text_key("v"));
    assert_eq!(rt.state.song.piano.tool, PianoRollTool::Pointer);
    let px = rt.state.song.piano.vp.ppb;
    press(&mut rt, text_key("="));
    assert!((rt.state.song.piano.vp.ppb - px * 1.25).abs() < 1e-6);
    press(&mut rt, text_key("-"));
    assert!((rt.state.song.piano.vp.ppb - px).abs() < 1e-6);
    rt.state.pr_zoom_x(300.0, None);
    press(&mut rt, text_key("f"));
    assert!((rt.state.song.piano.vp.ppb - px).abs() < 1e-6, "fit");
    // Q quantizes (all notes; every note already on the grid → still one step).
    rt.state.svc.engine.edit(|d| {
        let mut ns = d.get_tracks().iter().find(|t| t.id == b).unwrap().regions[0]
            .notes
            .clone();
        ns[0].start_beat += 0.1;
        d.update_region_notes(&b, &r, ns);
    });
    press(&mut rt, text_key("q"));
    assert_eq!(undo_label(&rt).as_deref(), Some("Quantize Notes"));
    assert_eq!(notes(&rt, &b, &r)[0].start_beat.fract() * 4.0 % 1.0, 0.0);
    // Space plays from the cue, Space again stops.
    press(&mut rt, text_key(" "));
    assert_ne!(
        rt.state.song_state(),
        mdaw_engine::engine::TransportState::Stopped
    );
    press(&mut rt, text_key(" "));
    assert_eq!(
        rt.state.song_state(),
        mdaw_engine::engine::TransportState::Stopped
    );
}

#[test]
fn draw_tool_click_and_drag_sets_the_length() {
    let (mut rt, b, r) = bass_rt();
    rt.state.set_pr_tool(PianoRollTool::Draw);
    rt.state.set_pr_snap(0.25);
    let (x, y) = grid_pt(&rt, 8.6, 47.0);
    click_at(&mut rt, x, y);
    let ns = notes(&rt, &b, &r);
    let n = ns.last().unwrap();
    assert_eq!((n.note, n.start_beat, n.duration_beats), (47.0, 8.5, 1.0));
    assert_eq!(undo_label(&rt).as_deref(), Some("Draw Note"));
    // Drag sets the length and the next draw length.
    let ppb = rt.state.song.piano.vp.ppb as f32;
    run(&mut rt, 0.8);
    let (x, y) = grid_pt(&rt, 10.0, 48.0);
    drag(&mut rt, x + 1.0, y, 2.0 * ppb, 0.0);
    let n = notes(&rt, &b, &r).last().unwrap().clone();
    assert_eq!((n.start_beat, n.duration_beats), (10.0, 2.0));
    assert_eq!(rt.state.song.piano.prefs.draw_length, 2.0);
}

#[test]
fn pointer_gestures_select_move_duplicate_resize_double_click_and_band() {
    let (mut rt, b, r) = bass_rt();
    let n0 = notes(&rt, &b, &r);
    let ppb = rt.state.song.piano.vp.ppb as f32;
    let rh = rt.state.song.piano.vp.row_h as f32;
    // Click selects; Shift-click adds.
    let (x, y) = grid_pt(&rt, n0[0].start_beat + 0.1, n0[0].note);
    click_at(&mut rt, x, y);
    assert_eq!(rt.state.song.piano.selection, vec![0]);
    run(&mut rt, 0.8);
    let (x1, y1) = grid_pt(&rt, n0[3].start_beat + 0.1, n0[3].note);
    click_mods(&mut rt, x1, y1, shift());
    assert_eq!(rt.state.song.piano.selection, vec![0, 3]);
    run(&mut rt, 0.8);
    // Drag moves both: one beat right, one row up.
    drag(&mut rt, x, y, ppb, -rh);
    let ns = notes(&rt, &b, &r);
    assert_eq!(
        (ns[0].start_beat, ns[0].note),
        (n0[0].start_beat + 1.0, n0[0].note + 1.0)
    );
    assert_eq!(ns[3].start_beat, n0[3].start_beat + 1.0);
    assert_eq!(undo_label(&rt).as_deref(), Some("Move Notes"));
    rt.state.undo();
    run(&mut rt, 0.8);
    // Alt-drag duplicates.
    let (x, y) = grid_pt(&rt, n0[0].start_beat + 0.1, n0[0].note);
    drag_mods(&mut rt, x, y, 4.0 * ppb, 0.0, alt());
    assert_eq!(notes(&rt, &b, &r).len(), 11);
    assert_eq!(undo_label(&rt).as_deref(), Some("Duplicate Notes"));
    rt.state.undo();
    run(&mut rt, 0.8);
    // The end edge resizes (and sets the draw length).
    let n = &n0[0];
    let (ex, ey) = grid_pt(&rt, n.start_beat + n.duration_beats, n.note);
    drag(&mut rt, ex - 2.0, ey, ppb, 0.0);
    assert_eq!(notes(&rt, &b, &r)[0].duration_beats, n.duration_beats + 1.0);
    assert_eq!(undo_label(&rt).as_deref(), Some("Resize Note"));
    assert_eq!(
        rt.state.song.piano.prefs.draw_length,
        n.duration_beats + 1.0
    );
    run(&mut rt, 0.8);
    // Double-click on a note deletes it; on empty space adds one.
    let (x, y) = grid_pt(&rt, n0[1].start_beat + 0.05, n0[1].note);
    rt.pointer_move(x, y);
    for _ in 0..2 {
        rt.pointer_down(x, y);
        rt.frame(0.02);
        rt.pointer_up(x, y);
        rt.frame(0.02);
    }
    frames(&mut rt, 2);
    assert_eq!(notes(&rt, &b, &r).len(), 9);
    assert_eq!(undo_label(&rt).as_deref(), Some("Delete Note"));
    run(&mut rt, 0.8);
    let (x, y) = grid_pt(&rt, 14.1, 48.0);
    for _ in 0..2 {
        rt.pointer_down(x, y);
        rt.frame(0.02);
        rt.pointer_up(x, y);
        rt.frame(0.02);
    }
    frames(&mut rt, 2);
    assert_eq!(notes(&rt, &b, &r).len(), 10);
    assert_eq!(undo_label(&rt).as_deref(), Some("Draw Note"));
    run(&mut rt, 0.8);
    // A band over everything selects it; a plain click on empty space clears.
    let g = rect(&rt, "pr-grid");
    let (bx, by) = grid_pt(&rt, -0.0, 49.0);
    let (_, by2) = grid_pt(&rt, 0.0, 38.0);
    drag(&mut rt, bx + 2.0, by, g[2] - 10.0, by2 - by);
    assert!(rt.state.song.piano.selection.len() >= 9);
    run(&mut rt, 0.8);
    let (x, y) = grid_pt(&rt, 15.0, 49.0);
    click_at(&mut rt, x, y);
    assert!(rt.state.song.piano.selection.is_empty());
}

#[test]
fn eraser_drag_and_right_click_erase_escape_cancels() {
    let (mut rt, b, r) = bass_rt();
    let n0 = notes(&rt, &b, &r);
    rt.state.set_pr_tool(PianoRollTool::Eraser);
    let (x, y) = grid_pt(&rt, n0[0].start_beat + 0.1, n0[0].note);
    let (x2, _) = grid_pt(&rt, n0[2].start_beat + 0.1, n0[2].note);
    drag(&mut rt, x, y, x2 - x, 0.0);
    assert_eq!(notes(&rt, &b, &r).len(), 7);
    assert_eq!(undo_label(&rt).as_deref(), Some("Delete Notes"));
    // Escape mid-gesture cancels (nothing committed).
    let (x, y) = grid_pt(&rt, n0[4].start_beat + 0.1, n0[4].note);
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    frames(&mut rt, 2);
    assert!(rt.state.song.piano.draft.is_some());
    press(&mut rt, ev(Key::Escape, rune::Mods::default()));
    assert!(rt.state.song.piano.draft.is_none());
    assert!(
        rt.state.song.editing.is_some(),
        "Escape cancelled, didn't close"
    );
    rt.pointer_up(x, y);
    frames(&mut rt, 2);
    assert_eq!(notes(&rt, &b, &r).len(), 7);
    // Right-click erases one note.
    rt.state.set_pr_tool(PianoRollTool::Pointer);
    run(&mut rt, 0.8);
    right_click_at(&mut rt, x, y);
    assert_eq!(notes(&rt, &b, &r).len(), 6);
    assert_eq!(undo_label(&rt).as_deref(), Some("Delete Note"));
}

#[test]
fn drum_regions_paint_along_a_row() {
    let mut rt = song_rt();
    let d = tid(&rt, "Drums");
    let r = rid(&rt, &d, 0);
    rt.state.open_piano_roll(&d, &r);
    rt.invalidate();
    run(&mut rt, 0.6);
    let c = rt.state.piano_ctx().unwrap();
    assert!(crate::piano::is_drum_layout(&c.layout));
    assert!(has(&rt, "pr-drum-row-0"));
    assert!(!has(&rt, "pr-length-select"), "no Length on drums");
    let n = notes(&rt, &d, &r).len();
    rt.state.set_pr_tool(PianoRollTool::Draw);
    // Clap row (index 4), from beat 12 dragging one beat: 1/16 steps → 4 hits.
    let ppb = rt.state.song.piano.vp.ppb as f32;
    let pitch = c.layout.row_to_pitch(4.0);
    let (x, y) = grid_pt(&rt, 12.05, pitch);
    drag(&mut rt, x, y, 0.9 * ppb, 0.0);
    let after = notes(&rt, &d, &r);
    assert_eq!(after.len(), n + 4);
    assert_eq!(undo_label(&rt).as_deref(), Some("Draw Notes"));
    assert!(
        after[n..]
            .iter()
            .all(|x| x.is_drum && x.duration_beats == 0.25)
    );
}

#[test]
fn ruler_sets_the_cue_draws_and_toggles_the_loop() {
    let (mut rt, _b, _r) = bass_rt();
    let rr = rect(&rt, "pr-ruler");
    let vp = rt.state.song.piano.vp;
    let start = rt.state.piano_ctx().unwrap().start;
    // Below the strip: the cue (stopped), snapped to the grid.
    click_at(
        &mut rt,
        rr[0] + (2.1 * vp.ppb - vp.left) as f32,
        rr[1] + 22.0,
    );
    assert!((rt.state.song.piano.cue - (start + 2.0)).abs() < 1e-9);
    // A click on the strip toggles the loop.
    let on = rt.state.svc.engine.doc().is_loop_enabled();
    run(&mut rt, 0.8);
    click_at(&mut rt, rr[0] + 100.0, rr[1] + 4.0);
    assert_ne!(rt.state.svc.engine.doc().is_loop_enabled(), on);
    // Dragging on the strip draws a loop (and switches it on).
    rt.state.svc.engine.edit(|d| d.set_loop_enabled(false));
    run(&mut rt, 0.8);
    let x0 = rr[0] + (4.0 * vp.ppb - vp.left) as f32 + 1.0;
    drag(&mut rt, x0, rr[1] + 4.0, (4.0 * vp.ppb) as f32, 0.0);
    let m = rt.state.svc.engine.doc().get_loop_markers().unwrap();
    assert_eq!((m.start_beat, m.end_beat), (start + 4.0, start + 8.0));
    assert!(rt.state.svc.engine.doc().is_loop_enabled());
}

#[test]
fn velocity_lane_set_offset_and_ramp() {
    let (mut rt, b, r) = bass_rt();
    let n0 = notes(&rt, &b, &r);
    let lane = rect(&rt, "pr-velocity");
    let vp = rt.state.song.piano.vp;
    let sx = |beat: f64| lane[0] + (beat * vp.ppb - vp.left) as f32;
    let vy = |v: f32| lane[1] + crate::piano::VEL_PAD_TOP + (1.0 - v) * (92.0 - 16.0);
    // Drag a stem to set its velocity (readout 0–127 while dragging).
    let x = sx(n0[0].start_beat) + 1.0;
    drag(&mut rt, x, vy(0.8), 0.0, vy(0.5) - vy(0.8));
    assert!((notes(&rt, &b, &r)[0].velocity - 0.5).abs() < 0.02);
    assert_eq!(undo_label(&rt).as_deref(), Some("Edit Velocity"));
    // A stem of a multi-selection offsets them all.
    rt.state.song.piano.selection = vec![0, 1];
    rt.invalidate();
    run(&mut rt, 0.8);
    let before: Vec<f64> = notes(&rt, &b, &r).iter().map(|n| n.velocity).collect();
    let y0 = vy(before[0] as f32);
    drag(&mut rt, x, y0, 0.0, -(76.0 * 0.1));
    let after: Vec<f64> = notes(&rt, &b, &r).iter().map(|n| n.velocity).collect();
    assert!((after[0] - before[0] - 0.1).abs() < 0.02 && (after[1] - before[1] - 0.1).abs() < 0.02);
    // Empty space (or Alt anywhere) draws a ramp across the notes in range.
    rt.state.song.piano.selection.clear();
    rt.invalidate();
    run(&mut rt, 0.8);
    let xa = sx(-0.0) + 2.0;
    let xb = sx(16.0);
    drag_mods(&mut rt, xa, vy(0.2), xb - xa, vy(1.0) - vy(0.2), alt());
    let v: Vec<f64> = notes(&rt, &b, &r).iter().map(|n| n.velocity).collect();
    assert!(v[0] < v[v.len() - 1], "ramp up: {v:?}");
    // Tooltip.
    let e = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("pr-velocity"))
        .unwrap();
    assert_eq!(
        e.help.as_deref(),
        Some("Drag a stem to set velocity · Drag empty space (or Alt+drag) to draw a ramp")
    );
}

#[test]
fn toolbar_selects_quantize_state_zoom_rows_and_shortcuts() {
    let (mut rt, b, r) = bass_rt();
    click_settle(&mut rt, "pr-grid-select");
    for o in SNAP_OPTIONS {
        assert!(has(&rt, &format!("pr-grid-menu-{}", o.id)), "{}", o.id);
    }
    click(&mut rt, "pr-grid-menu-off");
    assert_eq!(rt.state.song.piano.prefs.snap, 0.0);
    assert!(texts(&rt).contains(&"grid off".to_owned()));
    // Quantize is disabled with the grid off.
    click(&mut rt, "pr-quantize");
    assert_eq!(undo_depth(&rt), 0);
    // Length: a custom value adds its own item.
    rt.state.set_pr_draw_length(0.3);
    rt.invalidate();
    run(&mut rt, 0.3);
    click_settle(&mut rt, "pr-length-select");
    assert!(has(&rt, "pr-length-menu-custom"));
    click(&mut rt, "pr-length-menu-1/8");
    assert_eq!(rt.state.song.piano.prefs.draw_length, 0.5);
    // Tools.
    click(&mut rt, "pr-tool-draw");
    assert_eq!(rt.state.song.piano.tool, PianoRollTool::Draw);
    // Follow, zoom, rows, fit.
    click(&mut rt, "pr-follow");
    assert!(!rt.state.song.piano.prefs.follow);
    let px = rt.state.song.piano.vp.ppb;
    click(&mut rt, "pr-zoom-in");
    assert!((rt.state.song.piano.vp.ppb - px * 1.25).abs() < 1e-6);
    click(&mut rt, "pr-zoom-out");
    assert!((rt.state.song.piano.vp.ppb - px).abs() < 1e-6);
    let rh = rt.state.song.piano.vp.row_h;
    click(&mut rt, "pr-rows-taller");
    assert_eq!(rt.state.song.piano.vp.row_h, rh + 2.0);
    click(&mut rt, "pr-rows-shorter");
    assert_eq!(rt.state.song.piano.vp.row_h, rh);
    // Status: one selected note.
    rt.state.song.piano.selection = vec![0];
    rt.invalidate();
    frames(&mut rt, 2);
    let n = notes(&rt, &b, &r)[0].clone();
    let st = texts_of(&rt, "pr-status");
    assert!(
        st.starts_with("E2 · ")
            && st.ends_with(&format!("vel {}", (n.velocity * 127.0).round() as i64)),
        "{st}"
    );
    // Shortcuts popover: 23 rows.
    click_settle(&mut rt, "pr-shortcuts");
    assert!(texts(&rt).contains(&"Piano roll shortcuts".to_owned()));
    assert_eq!(crate::ui::piano::toolbar::SHORTCUTS.len(), 23);
    assert!(texts(&rt).contains(&"Esc".to_owned()));
}

#[test]
fn wheel_scrolls_and_zooms_like_the_ts() {
    let (mut rt, _b, _r) = bass_rt();
    let g = rect(&rt, "pr-grid");
    let (x, y) = (g[0] + 200.0, g[1] + 100.0);
    // On the ruler the wheel scrolls horizontally, on the keys vertically.
    let left = rt.state.song.piano.vp.left;
    let (rx, ry) = at(&rt, "pr-ruler");
    wheel_mods(&mut rt, rx, ry, 0.0, -40.0, rune::Mods::default());
    assert!(rt.state.song.piano.vp.left > left);
    run(&mut rt, 1.0); // the next wheel is a new gesture
    let top = rt.state.song.piano.vp.top;
    let (kx, ky) = at(&rt, "pr-keys");
    wheel_mods(&mut rt, kx, ky, 0.0, -40.0, rune::Mods::default());
    assert!(rt.state.song.piano.vp.top > top);
    run(&mut rt, 1.0);
    let vp = rt.state.song.piano.vp;
    // Plain wheel scrolls the grid.
    wheel_mods(&mut rt, x, y, 0.0, -48.0, rune::Mods::default());
    assert!(rt.state.song.piano.vp.top > vp.top);
    // ⌘ + wheel zooms X at the pointer, Alt + wheel zooms Y.
    run(&mut rt, 1.0);
    wheel_mods(&mut rt, x, y, 0.0, 100.0, cmd());
    assert!(rt.state.song.piano.vp.ppb > vp.ppb);
    let rh = rt.state.song.piano.vp.row_h;
    run(&mut rt, 1.0);
    wheel_mods(&mut rt, x, y, 0.0, 200.0, alt());
    assert!(rt.state.song.piano.vp.row_h > rh);
}

#[test]
fn key_column_auditions_and_lights_selected_pitches() {
    let (mut rt, _b, _r) = bass_rt();
    let k = rect(&rt, "pr-keys");
    drag(&mut rt, k[0] + 20.0, k[1] + 100.0, 0.0, 60.0);
    assert!(rt.state.song.piano.key_press.is_none(), "released on up");
    let e = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("pr-keys"))
        .unwrap();
    assert_eq!(
        e.help.as_deref(),
        Some("Click or drag across the keys to audition")
    );
}

#[test]
fn song_dock_pads_keys_bank_and_collapse() {
    let mut rt = song_rt();
    assert!(has(&rt, "song-dock"));
    assert!(has(&rt, "dock-pads") && has(&rt, "dock-keys"));
    click(&mut rt, "dock-bank-B");
    assert!(rt.state.song.dock.bank_b);
    assert_eq!(
        rt.state
            .svc
            .settings
            .borrow()
            .get_item("micro-daw:dock-pad-bank"),
        Some("B")
    );
    click(&mut rt, "song-dock-pads");
    assert!(!has(&rt, "dock-pads"));
    click(&mut rt, "song-dock-collapse");
    assert!(!has(&rt, "dock-keys"), "collapse hides both");
    click(&mut rt, "song-dock-collapse");
    assert!(has(&rt, "dock-pads") && has(&rt, "dock-keys"));
    // The keyboard plays through the router at velocity 0.7 and releases on up.
    let (x, y) = at(&rt, "key-60");
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    frames(&mut rt, 2);
    assert_eq!(rt.state.active_notes(), vec![60]);
    rt.pointer_up(x, y);
    frames(&mut rt, 2);
    assert!(rt.state.active_notes().is_empty());
    // Opening the piano roll hides the dock (and would release a held key).
    let b = tid(&rt, "Bass");
    let r = rid(&rt, &b, 0);
    rt.state.open_piano_roll(&b, &r);
    rt.invalidate();
    frames(&mut rt, 2);
    assert!(!has(&rt, "song-dock"));
}
