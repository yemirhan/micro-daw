//! StepSequencer: paint / erase drags, Alt-drag and wheel velocity, keys, labels, header.

use rune::{Key, Mods};

use super::harness::*;

/// Window point of the centre of cell (pad, step).
fn cell(r: &rune::Runtime<crate::App>, pad: usize, step: usize) -> (f32, f32) {
    let g = rect(r, "step-grid");
    let (cw, rh) = r.state.steps_geometry.get();
    let row = 15 - pad;
    (
        g[0] + (step as f32 + 0.5) * cw,
        g[1] + (row as f32 + 0.5) * rh,
    )
}

#[test]
fn geometry_matches_the_ts() {
    let r = rt();
    let (cw, rh) = r.state.steps_geometry.get();
    let b = rect(&r, "steps-box");
    assert_eq!(cw, (b[2] / 32.0).floor().max(18.0));
    assert_eq!(rh, (b[3] / 16.0).floor().max(14.0));
    let labels = rect(&r, "row-label-15");
    assert_eq!(labels[2], 132.0, "label column 132 px");
}

#[test]
fn click_toggles_and_drag_paints_then_erases_as_one_step_each() {
    let mut r = rt();
    let (x, y) = cell(&r, 0, 0);
    click_at(&mut r, x, y);
    assert_eq!(notes(&r).len(), 1);
    assert!(
        (notes(&r)[0].2 - 0.8).abs() < 1e-9,
        "engine default velocity 0.8"
    );
    assert_eq!(undo_label(&r).as_deref(), Some("Paint steps"));
    // Paint across steps 2..5 of pad 1.
    let (x0, y0) = cell(&r, 1, 2);
    let (x1, _) = cell(&r, 1, 5);
    drag(&mut r, x0, y0, x1 - x0, 0.0);
    assert_eq!(notes(&r).iter().filter(|n| n.0 == 1).count(), 4);
    r.state.undo();
    frames(&mut r, 2);
    assert_eq!(
        notes(&r).iter().filter(|n| n.0 == 1).count(),
        0,
        "the paint drag is one undo step"
    );
    // A drag starting on a filled cell erases.
    let (x, y) = cell(&r, 0, 0);
    drag(&mut r, x, y, 0.0, 0.0);
    assert!(notes(&r).is_empty());
    assert_eq!(undo_label(&r).as_deref(), Some("Erase steps"));
    // Pointer down on another row selects that pad.
    assert_eq!(r.state.selected_pad(), 0);
    let (x, y) = cell(&r, 9, 3);
    click_at(&mut r, x, y);
    assert_eq!(r.state.selected_pad(), 9);
}

#[test]
fn alt_drag_sets_velocity_and_alt_click_on_empty_does_nothing() {
    let mut r = rt();
    r.state.toggle_step(2, 4, 0.25);
    frames(&mut r, 2);
    let (x, y) = cell(&r, 2, 4);
    hold_mods(
        &mut r,
        Mods {
            alt: true,
            ..Mods::default()
        },
    );
    drag(&mut r, x, y, 0.0, 60.0); // 60 px down of 120 = −0.5
    assert!((notes(&r)[0].2 - 0.3).abs() < 1e-9, "{:?}", notes(&r));
    assert_eq!(undo_label(&r).as_deref(), Some("Step velocity"));
    let (x, y) = cell(&r, 3, 4);
    click_at(&mut r, x, y);
    hold_mods(&mut r, Mods::default());
    assert_eq!(
        notes(&r).len(),
        1,
        "Alt-click on an empty cell adds nothing"
    );
}

#[test]
fn wheel_nudges_velocity_on_filled_cells_as_a_burst() {
    let mut r = rt();
    r.state.toggle_step(0, 8, 0.25);
    frames(&mut r, 2);
    let (x, y) = cell(&r, 0, 8);
    wheel(&mut r, x, y, 10.0);
    frames(&mut r, 1);
    wheel(&mut r, x, y, 10.0);
    frames(&mut r, 1);
    assert!((notes(&r)[0].2 - 0.9).abs() < 1e-9, "+0.05 twice");
    run(&mut r, 0.5);
    assert_eq!(undo_label(&r).as_deref(), Some("Step velocity"));
    r.state.undo();
    frames(&mut r, 2);
    assert!(
        (notes(&r)[0].2 - 0.8).abs() < 1e-9,
        "the burst undoes at once"
    );
    // Over an empty cell the wheel does nothing.
    let (x, y) = cell(&r, 5, 8);
    wheel(&mut r, x, y, 10.0);
    frames(&mut r, 2);
    assert_eq!(notes(&r).len(), 1);
}

#[test]
fn keyboard_moves_toggles_sets_velocity_and_announces() {
    let mut r = rt();
    let (x, y) = cell(&r, 0, 0);
    click_at(&mut r, x, y); // focus + toggle on
    press(&mut r, ev(Key::Right, Mods::default()));
    assert_eq!(r.state.beat.step_focus.step, 1);
    assert!(r.state.beat.step_kb_focus);
    press(&mut r, ev(Key::Right, shift()));
    assert_eq!(
        r.state.beat.step_focus.step, 4,
        "Shift+→ jumps to the next beat line"
    );
    press(&mut r, ev(Key::Up, Mods::default()));
    assert_eq!(r.state.beat.step_focus.pad, 1);
    assert_eq!(
        r.state.selected_pad(),
        1,
        "a move to another row selects that pad"
    );
    press(&mut r, text_key(" "));
    assert!(
        notes(&r)
            .iter()
            .any(|n| n.0 == 1 && (n.1 - 1.0).abs() < 1e-9)
    );
    assert_eq!(
        r.state.beat.step_announce,
        "Snare, step 5, on, velocity 80%"
    );
    press(&mut r, text_key("-"));
    assert!(
        notes(&r)
            .iter()
            .any(|n| n.0 == 1 && (n.2 - 0.7).abs() < 1e-9)
    );
    r.repeat_key(text_key(" "));
    frames(&mut r, 2);
    assert!(
        notes(&r).iter().any(|n| n.0 == 1),
        "toggle ignores auto-repeat"
    );
    press(&mut r, ev(Key::End, Mods::default()));
    assert_eq!(r.state.beat.step_focus.step, 31);
    press(&mut r, ev(Key::Home, cmd()));
    assert_eq!(
        (r.state.beat.step_focus.pad, r.state.beat.step_focus.step),
        (15, 0)
    );
}

#[test]
fn row_labels_select_and_step_size_changes_steps() {
    let mut r = rt();
    click(&mut r, "row-label-12");
    assert_eq!(r.state.selected_pad(), 12);
    click(&mut r, "step-size-2");
    let texts: Vec<String> = r.elements().iter().filter_map(|e| e.text.clone()).collect();
    assert!(texts.iter().any(|t| t == "64 steps · 2 bars"), "{texts:?}");
}

#[test]
fn clear_is_disabled_without_notes_and_clears() {
    let mut r = rt();
    let disabled = |r: &rune::Runtime<crate::App>| {
        r.elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("clear-pattern"))
            .unwrap()
            .opacity
            < 0.9
    };
    assert!(disabled(&r));
    r.state.toggle_step(0, 0, 0.25);
    run(&mut r, 0.6);
    assert!(!disabled(&r));
    click(&mut r, "clear-pattern");
    assert!(notes(&r).is_empty());
}

#[test]
fn loop_hint_and_loop_pattern() {
    let mut r = rt();
    assert!(!has(&r, "loop-hint"));
    // Markers that differ from the pattern (starter: markers on the 2-bar pattern).
    r.state.svc.engine.edit(|d| {
        d.set_loop_enabled(true);
        d.set_loop_markers(0.0, 4.0);
    });
    frames(&mut r, 2);
    assert!(has(&r, "loop-hint"));
    let texts: Vec<String> = r.elements().iter().filter_map(|e| e.text.clone()).collect();
    assert!(
        texts
            .iter()
            .any(|t| t == "Loop is 1 bar, pattern is 2 bars"),
        "{texts:?}"
    );
    click(&mut r, "loop-pattern");
    assert_eq!(undo_label(&r).as_deref(), Some("Loop pattern"));
    assert!(!has(&r, "loop-hint"));
}

#[test]
fn playhead_moves_in_whole_steps_while_playing() {
    let mut r = rt();
    assert_eq!(r.state.playhead(), None);
    r.state.toggle_play();
    run(&mut r, 0.4); // 120 BPM: 0.4 s = beat 0.8 → step 3
    let s = r.state.playhead().unwrap();
    assert!((2..=4).contains(&s), "{s}");
    let x = r.state.live.playhead_px.get()[0];
    assert_eq!(x, s as f32 * r.state.steps_geometry.get().0);
    r.state.toggle_play();
    run(&mut r, 0.1);
    assert_eq!(r.state.playhead(), None);
}

#[test]
fn recording_tints_the_grid() {
    let mut r = rt();
    r.state
        .set_loop_settings(mdaw_model::doc::LoopSettingsPatch {
            count_in_bars: Some(0),
            ..Default::default()
        });
    r.state.toggle_record();
    run(&mut r, 0.3);
    let pose = r.presented_pose("step-sequencer").unwrap();
    assert!(
        !pose.shadows().is_empty(),
        "red inset top shadow while recording"
    );
}
