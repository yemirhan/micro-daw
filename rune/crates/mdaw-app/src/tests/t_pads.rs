//! PadGrid / Pad / PadThumb / count-in overlay / drops.

use mdaw_model::doc::LoopSettingsPatch;
use rune::{Key, Mods};

use super::harness::*;
use crate::app::{Menu, PadHold};

fn record_no_count_in(r: &mut rune::Runtime<crate::App>) {
    r.state.set_loop_settings(LoopSettingsPatch {
        count_in_bars: Some(0),
        ..Default::default()
    });
    r.state.toggle_record();
    run(r, 0.3);
}

#[test]
fn grid_rows_follow_the_layout() {
    let mut r = rt();
    // 4x4: pad 12 (B5) top-left, pad 0 (A1) bottom-left.
    let (b5, a1) = (rect(&r, "pad-12"), rect(&r, "pad-0"));
    assert!(b5[1] < a1[1] && (b5[0] - a1[0]).abs() < 1.0);
    click(&mut r, "pad-layout-1");
    run(&mut r, 1.0);
    // 2x8: [4..7, 12..15] over [0..3, 8..11]; a gap before column 4.
    let (a5, a1, b1, a4) = (
        rect(&r, "pad-4"),
        rect(&r, "pad-0"),
        rect(&r, "pad-8"),
        rect(&r, "pad-3"),
    );
    assert!(a5[1] < a1[1]);
    assert!((b1[1] - a1[1]).abs() < 1.0);
    let gap = b1[0] - (a4[0] + a4[2]);
    assert!(gap > 12.0, "bank gap: 6 + 8, got {gap}");
}

#[test]
fn pointer_down_selects_and_hits_with_velocity_from_the_click_height() {
    let mut r = rt();
    record_no_count_in(&mut r);
    let rc = rect(&r, "pad-1");
    // Top quarter: velocity round((1 - 0.7 * 0.25) * 100) / 100 = 0.83 (± the click's point).
    let (x, y) = (rc[0] + rc[2] / 2.0, rc[1] + rc[3] * 0.25);
    r.pointer_move(x, y);
    r.pointer_down(x, y);
    frames(&mut r, 2);
    assert_eq!(r.state.selected_pad(), 1);
    assert_eq!(r.state.beat.hold, Some(PadHold::Pointer(1)));
    r.pointer_up(x, y);
    frames(&mut r, 2);
    assert_eq!(r.state.beat.hold, None, "released exactly once");
    run(&mut r, 0.2);
    r.state.toggle_record();
    frames(&mut r, 3);
    let n = notes(&r);
    assert_eq!(n.len(), 1, "{n:?}");
    assert_eq!(n[0].0, 1);
    assert!((n[0].2 - 0.83).abs() < 0.011, "velocity {}", n[0].2);
}

#[test]
fn a_hit_flashes_the_pad() {
    let mut r = rt();
    click(&mut r, "pad-5");
    run(&mut r, 0.05);
    // The engine reported the hit (a PadHit event) and the light was lit at 0.35 + 0.6 v.
    let (p, peak) = r.state.last_flash.expect("flashed");
    assert_eq!(p, 5);
    let rc = rect(&r, "pad-5");
    let _ = rc;
    assert!(peak > 0.35 && peak <= 0.95, "{peak}");
}

#[test]
fn enter_and_space_on_a_focused_pad_hold_and_release() {
    let mut r = rt();
    click(&mut r, "pad-6");
    press(&mut r, ev(Key::Enter, Mods::default()));
    assert_eq!(r.state.beat.hold, Some(PadHold::Key(6)));
    assert_eq!(r.state.selected_pad(), 6);
    r.repeat_key(ev(Key::Enter, Mods::default()));
    frames(&mut r, 1);
    release(&mut r, ev(Key::Enter, Mods::default()));
    assert_eq!(r.state.beat.hold, None);
}

#[test]
fn right_click_selects_and_opens_the_context_menu_at_the_pointer() {
    let mut r = rt();
    right_click(&mut r, "pad-9");
    assert_eq!(r.state.selected_pad(), 9);
    assert!(matches!(r.state.menu, Some(Menu::Pad { pad: 9, .. })));
    assert!(has(&r, "pad-menu"));
    let m = rect(&r, "pad-menu");
    assert!((m[2] - 208.0).abs() < 1.0, "208 wide");
    assert!(
        m[0] >= 4.0 && m[1] + m[3] <= 900.0 - 3.0,
        "inside the window"
    );
    // Escape closes.
    press(&mut r, ev(Key::Escape, Mods::default()));
    assert!(r.state.menu.is_none());
}

#[test]
fn pad_labels_and_a11y() {
    let r = rt();
    let e = r
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("pad-0"))
        .unwrap();
    assert_eq!(e.label.as_deref(), Some("Pad A1: Kick"));
    let texts: Vec<String> = r.elements().iter().filter_map(|e| e.text.clone()).collect();
    assert!(
        texts.iter().any(|t| t == "C2"),
        "hardware note label of A1 (MPK default map)"
    );
}

#[test]
fn empty_pad_reads_empty() {
    let mut r = rt();
    r.state.clear_pad(3);
    frames(&mut r, 2);
    let e = r
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("pad-3"))
        .unwrap();
    assert_eq!(e.label.as_deref(), Some("Pad A4, empty"));
}

#[test]
fn count_in_overlay_while_counting() {
    let mut r = rt();
    click(&mut r, "record");
    run(&mut r, 0.6);
    assert!(r.state.count_in() > 0);
    assert!(has(&r, "count-in-overlay"));
    let texts: Vec<String> = r.elements().iter().filter_map(|e| e.text.clone()).collect();
    assert!(texts.iter().any(|t| t == "COUNT-IN"));
    run(&mut r, 2.0);
    assert_eq!(r.state.count_in(), 0);
    assert!(!has(&r, "count-in-overlay"));
    assert!(has(&r, "rec-dot"), "recording dot in the LCD");
}

#[test]
fn a_file_dropped_on_a_pad_is_assigned_and_selected() {
    let mut r = rt();
    let (x, y) = at(&r, "pad-7");
    r.drop_files(
        vec![std::path::PathBuf::from("/samples/My Snare.WAV")],
        Some((x, y)),
    );
    frames(&mut r, 3);
    let p = pad(&r, 7);
    let s = p.sample.unwrap();
    assert_eq!(s.name, "My Snare");
    assert!(
        matches!(s.source, mdaw_model::types::PadSampleSource::File { ref path, .. } if path == "/samples/My Snare.WAV")
    );
    assert_eq!(r.state.selected_pad(), 7);
}

#[test]
fn key_hint_caps_show_with_the_toggle() {
    let mut r = rt();
    let has_cap =
        |r: &rune::Runtime<crate::App>| r.elements().iter().any(|e| e.text.as_deref() == Some("Z"));
    assert!(!has_cap(&r));
    click(&mut r, "key-hints");
    assert!(has_cap(&r));
}
