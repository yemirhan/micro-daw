//! PadEditor: knobs (one undo step per gesture), play mode, reverse, choke, trim handles,
//! audition, chop panel.

use mdaw_model::types::PadPlayMode;
use rune::{Key, Mods};

use super::harness::*;
use crate::app::Menu;

#[test]
fn gain_knob_drag_is_one_undo_step_and_formats() {
    let mut r = rt();
    let (x, y) = at(&r, "knob-gain");
    // Range 72 dB over 200 px: 50 px down = −18 dB.
    drag(&mut r, x, y, 0.0, 50.0);
    assert_eq!(pad(&r, 0).gain_db, -18.0);
    assert!(r.state.edit_group.is_none(), "pointer up closed the group");
    assert_eq!(undo_label(&r).as_deref(), Some("Edit pad"));
    let texts: Vec<String> = r.elements().iter().filter_map(|e| e.text.clone()).collect();
    assert!(texts.iter().any(|t| t == "-18.0dB"));
    r.state.undo();
    frames(&mut r, 2);
    assert_eq!(pad(&r, 0).gain_db, 0.0, "the whole drag undoes at once");
}

#[test]
fn shift_drag_is_four_times_finer() {
    let mut r = rt();
    let (x, y) = at(&r, "knob-pitch");
    hold_mods(&mut r, shift());
    drag(&mut r, x, y, 0.0, -100.0);
    hold_mods(&mut r, Mods::default());
    // 48 st over 800 px: 100 px = 6 st.
    assert_eq!(pad(&r, 0).pitch, 6.0);
}

#[test]
fn knob_wheel_double_click_and_keys() {
    let mut r = rt();
    let (x, y) = at(&r, "knob-pan");
    wheel(&mut r, x, y, 5.0);
    frames(&mut r, 2);
    // tick = max(0.01, 2 × 0.015) = 0.03.
    assert!((pad(&r, 0).pan - 0.03).abs() < 1e-9);
    assert!(
        r.state.edit_group.is_some(),
        "wheel has no onChangeEnd: the group waits for 700 ms idle"
    );
    run(&mut r, 0.8);
    assert!(r.state.edit_group.is_none());
    double_click(&mut r, "knob-pan");
    assert_eq!(pad(&r, 0).pan, 0.0);
    click(&mut r, "knob-pan");
    press(&mut r, ev(Key::Right, Mods::default()));
    assert!(
        (pad(&r, 0).pan - 0.02).abs() < 1e-9,
        "max(step, range × 0.01) = 0.02"
    );
    press(&mut r, ev(Key::End, Mods::default()));
    assert_eq!(pad(&r, 0).pan, 1.0);
    press(&mut r, ev(Key::Backspace, Mods::default()));
    assert_eq!(pad(&r, 0).pan, 0.0, "Delete / Backspace = default");
}

#[test]
fn decay_and_filter_knobs_write_log_values() {
    let mut r = rt();
    // Cutoff lives in the Filter section, release (the pad's decay) in the Amp section.
    click(&mut r, "shape-tab-1");
    click(&mut r, "knob-filter");
    press(&mut r, ev(Key::Home, Mods::default()));
    assert_eq!(pad(&r, 0).filter_cutoff, 20.0);
    press(&mut r, ev(Key::End, Mods::default()));
    assert_eq!(pad(&r, 0).filter_cutoff, 20000.0, "t ≥ 0.999 = open");
    click(&mut r, "shape-tab-0");
    click(&mut r, "knob-decay");
    press(&mut r, ev(Key::End, Mods::default()));
    assert!((pad(&r, 0).decay - 4.0).abs() < 1e-9);
    assert_eq!(crate::ui::beat::editor::filter_text(1.0), "Open");
    assert_eq!(crate::ui::beat::editor::filter_text(0.5), "632");
}

#[test]
fn play_mode_reverse_and_choke() {
    let mut r = rt();
    click(&mut r, "play-mode-1");
    assert_eq!(pad(&r, 0).mode, PadPlayMode::Gate);
    assert_eq!(undo_label(&r).as_deref(), Some("Edit pad"));
    assert!(
        r.state.edit_group.is_none(),
        "discrete controls close their step at once"
    );
    click(&mut r, "reverse");
    assert!(pad(&r, 0).reverse);
    click(&mut r, "choke");
    assert_eq!(r.state.menu, Some(Menu::Choke));
    let item = r
        .elements()
        .into_iter()
        .rfind(|e| e.text.as_deref() == Some("7"))
        .map(|e| e.path)
        .unwrap();
    let (x, y) = r.find(&item).unwrap();
    click_at(&mut r, x, y);
    assert_eq!(pad(&r, 0).choke_group, 7);
}

#[test]
fn trim_handles_drag_keys_and_double_click_reset() {
    let mut r = rt();
    let w = rect(&r, "trim-wave");
    let dur = 0.759; // the factory kick (Electron shows 759 ms)
    // Drag the start handle to 25%.
    drag(
        &mut r,
        w[0] + 1.0,
        w[1] + w[3] / 2.0,
        w[2] * 0.25 - 1.0,
        0.0,
    );
    let p = pad(&r, 0);
    assert!((p.start_sec - dur * 0.25).abs() < 0.01, "{}", p.start_sec);
    assert_eq!(undo_label(&r).as_deref(), Some("Edit pad"));
    // Drag the end handle to the right edge: null (full length).
    drag(
        &mut r,
        w[0] + w[2] - 1.0,
        w[1] + w[3] / 2.0,
        -w[2] * 0.25,
        0.0,
    );
    assert!(pad(&r, 0).end_sec.is_some());
    drag(
        &mut r,
        w[0] + w[2] * 0.75,
        w[1] + w[3] / 2.0,
        w[2] * 0.3,
        0.0,
    );
    assert_eq!(pad(&r, 0).end_sec, None, "at the end → null");
    // Keys on the start handle: ±0.001 × max(1, duration) s.
    let before = pad(&r, 0).start_sec;
    click(&mut r, "trim-start");
    press(&mut r, ev(Key::Right, Mods::default()));
    assert!((pad(&r, 0).start_sec - (before + 0.001)).abs() < 1e-9);
    // Double-click resets the trim.
    double_click(&mut r, "trim-wave");
    let p = pad(&r, 0);
    assert_eq!((p.start_sec, p.end_sec), (0.0, None));
}

#[test]
fn trim_patch_rules() {
    use crate::ui::beat::editor::{handle_patch, nudge_patch};
    let f = |v: &serde_json::Value| v.as_f64().unwrap();
    assert!((f(&handle_patch(true, 0.5, 0.0, 0.4, 1.0)["startSec"]) - 0.395).abs() < 1e-12);
    assert!((f(&handle_patch(false, 0.1, 0.2, 1.0, 1.0)["endSec"]) - 0.205).abs() < 1e-12);
    assert!(handle_patch(false, 0.99995, 0.2, 1.0, 1.0)["endSec"].is_null());
    assert!((f(&nudge_patch(true, 1.0, true, 0.0, 1.0, 2.0)["startSec"]) - 0.02).abs() < 1e-12);
}

#[test]
fn header_shows_trim_readout_and_audition_plays() {
    let mut r = rt();
    let texts: Vec<String> = r.elements().iter().filter_map(|e| e.text.clone()).collect();
    assert!(
        texts.iter().any(|t| t == "0ms – 759ms"),
        "{:?}",
        texts
            .iter()
            .filter(|t| t.contains("ms"))
            .collect::<Vec<_>>()
    );
    r.state.last_flash = None;
    click(&mut r, "audition");
    run(&mut r, 0.05);
    assert_eq!(
        r.state.last_flash.map(|f| f.0),
        Some(0),
        "audition = drumHit (heard + flashed)"
    );
}

#[test]
fn chop_panel_slices_and_applies_as_one_undo_step() {
    let mut r = rt();
    click(&mut r, "chop-button");
    assert_eq!(r.state.menu, Some(Menu::Chop));
    click(&mut r, "chop-mode-2"); // 8 equal slices
    assert_eq!(r.state.beat.slices.len(), 8);
    let texts: Vec<String> = r.elements().iter().filter_map(|e| e.text.clone()).collect();
    assert!(
        texts
            .iter()
            .any(|t| t.starts_with("8 slices → pads A1–A8.")),
        "{texts:?}"
    );
    click(&mut r, "chop-apply");
    assert!(r.state.menu.is_none());
    assert_eq!(undo_label(&r).as_deref(), Some("Chop to pads"));
    assert_eq!(pad(&r, 1).sample.unwrap().name, "Kick 2");
    assert_eq!(pad(&r, 7).sample.unwrap().name, "Kick 8");
    let (a, b) = (pad(&r, 0), pad(&r, 1));
    assert!(a.end_sec.is_some() && (b.start_sec - a.end_sec.unwrap()).abs() < 1e-9);
    r.state.undo();
    frames(&mut r, 2);
    assert_eq!(
        pad(&r, 1).sample.unwrap().name,
        "Snare",
        "one undo restores every pad"
    );
}

#[test]
fn empty_pad_editor_text() {
    let mut r = rt();
    r.state.clear_pad(0);
    frames(&mut r, 3);
    let texts: Vec<String> = r.elements().iter().filter_map(|e| e.text.clone()).collect();
    assert!(texts.iter().any(|t| t == "Empty pad"));
    assert!(
        texts
            .iter()
            .any(|t| t == "Drop a sample on a pad, or press Enter in the browser.")
    );
}
