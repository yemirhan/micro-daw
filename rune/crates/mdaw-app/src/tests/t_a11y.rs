//! Accessibility semantics (phase-7 review finding 5): the range controls carry ARIA's
//! `aria-valuemin / -max / -now / -valuetext` (Rune's `range` + `value`) and the increment /
//! decrement actions an assistive client uses; the selected pad reads as selected
//! (`aria-selected`), not checked. Asserted on `accessibility_json()` as `--tree` writes it.

use rune::accesskit::Action;
use rune::{Key, Mods};
use serde_json::Value;

use super::harness::*;

/// The accessibility node keyed `key` (depth-first).
fn node(j: &Value, key: &str) -> Option<Value> {
    if j.get("key").and_then(Value::as_str) == Some(key) {
        return Some(j.clone());
    }
    for c in j
        .get("children")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(n) = node(c, key) {
            return Some(n);
        }
    }
    None
}

fn a11y(r: &rune::Runtime<crate::App>, key: &str) -> Value {
    node(&r.accessibility_json(), key).unwrap_or_else(|| panic!("no a11y node {key}"))
}

/// `[value, min, max]` (and the step when there is one).
fn range(n: &Value) -> Vec<f64> {
    n["range"]
        .as_array()
        .unwrap_or_else(|| panic!("no range: {n}"))
        .iter()
        .filter_map(Value::as_f64)
        .collect()
}

fn strs(n: &Value, field: &str) -> Vec<String> {
    n[field]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect()
}

fn has_step_actions(n: &Value) {
    let actions = strs(n, "actions");
    for a in ["increment", "decrement"] {
        assert!(actions.iter().any(|x| x == a), "{a} missing: {n}");
    }
}

#[test]
fn knobs_report_range_value_text_and_step_actions() {
    let mut r = rt();
    let gain = a11y(&r, "knob-gain");
    assert_eq!(gain["role"], "slider");
    assert_eq!(gain["name"], "Gain");
    // `aria-valuetext={display}` (fmtDb), `aria-valuemin/max/now`.
    assert_eq!(gain["value"], "0.0dB");
    assert_eq!(&range(&gain)[..3], &[0.0, -60.0, 12.0]);
    has_step_actions(&gain);
    // The swing knob's text is the percentage the TS asserts (`aria-valuetext="50%"`).
    let swing = a11y(&r, "knob-swing");
    assert_eq!(swing["value"], "50%");
    assert_eq!(&range(&swing)[..3], &[0.0, 0.0, 0.75]);
    // Increment = one keyboard tick (max(step, 1% of the range) = 0.72 dB → snapped to 0.5),
    // one undo step like a key press.
    assert!(r.accessibility_action("knob-gain", Action::Increment, None));
    frames(&mut r, 2);
    assert_eq!(pad(&r, 0).gain_db, 0.5);
    assert_eq!(undo_label(&r).as_deref(), Some("Edit pad"));
    assert_eq!(a11y(&r, "knob-gain")["value"], "+0.5dB");
    assert!(r.accessibility_action("knob-swing", Action::Increment, None));
    frames(&mut r, 2);
    assert_eq!(a11y(&r, "knob-swing")["value"], "51%"); // +0.01 swing = +0.5 %
}

#[test]
fn trim_handles_bpm_and_volume_are_ranges_with_actions() {
    let mut r = rt();
    // Trim start: 0 … duration (3 decimals), valuetext formatSeconds.
    let start = a11y(&r, "trim-start");
    assert_eq!(start["role"], "slider");
    assert_eq!(start["value"], "0ms");
    let rg = range(&start);
    assert_eq!(rg[0], 0.0);
    assert!((rg[2] - 0.759).abs() < 0.002, "{rg:?}");
    has_step_actions(&start);
    assert!(r.accessibility_action("trim-start", Action::Increment, None));
    frames(&mut r, 2);
    assert!((pad(&r, 0).start_sec - 0.001).abs() < 1e-9);
    // BPM: a spinbutton 40 … 240 that steps by 1.
    let bpm = a11y(&r, "bpm");
    assert_eq!(bpm["role"], "spin-button");
    assert_eq!(range(&bpm), vec![120.0, 40.0, 240.0, 1.0]);
    assert_eq!(bpm["value"], "120");
    assert!(r.accessibility_action("bpm", Action::Increment, None));
    frames(&mut r, 2);
    assert_eq!(r.state.svc.engine.doc().get_bpm(), 121.0);
    // Output volume: −40 … 0 dB by 1, "N dB"; also from the keyboard (Radix Slider keys).
    let db0 = r.state.svc.engine.get_master_volume_db();
    let vol = a11y(&r, "volume");
    assert_eq!(&range(&vol)[1..], &[-40.0, 0.0, 1.0]);
    assert_eq!(vol["value"], format!("{} dB", db0.round() as i64));
    has_step_actions(&vol);
    click(&mut r, "volume");
    let db1 = r.state.svc.engine.get_master_volume_db();
    press(&mut r, ev(Key::Left, Mods::default()));
    assert_eq!(r.state.svc.engine.get_master_volume_db(), db1 - 1.0);
    press(&mut r, ev(Key::Home, Mods::default()));
    assert_eq!(r.state.svc.engine.get_master_volume_db(), -40.0);
    let saved = mdaw_model::settings::read_master_volume(&r.state.svc.settings.borrow());
    assert_eq!(saved, -40.0, "the keyboard change is persisted like a drag");
}

#[test]
fn chop_sensitivity_is_a_keyboard_slider_that_fills_its_row() {
    let mut r = rt();
    click(&mut r, "chop-button");
    run(&mut r, 0.3);
    let n = a11y(&r, "chop-sensitivity");
    assert_eq!(range(&n), vec![0.5, 0.0, 1.0, 0.01]);
    assert_eq!(n["value"], "0.5");
    // `flex-1`: the track takes the rest of the 232-point row after the label.
    let w = rect(&r, "chop-sensitivity")[2];
    assert!(w > 150.0, "slider width {w}");
    click(&mut r, "chop-sensitivity");
    let before = match r.state.beat.chop {
        crate::app::ChopUi::Transients { sensitivity } => sensitivity,
        _ => unreachable!(),
    };
    press(&mut r, ev(Key::Right, Mods::default()));
    let after = match r.state.beat.chop {
        crate::app::ChopUi::Transients { sensitivity } => sensitivity,
        _ => unreachable!(),
    };
    assert!((after - (before + 0.01)).abs() < 1e-9, "{before} → {after}");
    press(&mut r, ev(Key::End, Mods::default()));
    assert!(matches!(
        r.state.beat.chop,
        crate::app::ChopUi::Transients { sensitivity } if sensitivity == 1.0
    ));
}

#[test]
fn the_selected_pad_reads_selected_not_checked() {
    let mut r = rt();
    r.state.select_pad(5);
    r.invalidate();
    frames(&mut r, 2);
    let sel = a11y(&r, "pad-5");
    assert_eq!(sel["role"], "grid-cell");
    assert!(
        strs(&sel, "states").iter().any(|s| s == "selected"),
        "{sel}"
    );
    assert!(sel.get("checked").is_none(), "{sel}");
    let other = a11y(&r, "pad-4");
    assert!(!strs(&other, "states").iter().any(|s| s == "selected"));
}

#[test]
fn progress_ring_reports_its_percentage() {
    fn view(_: &(), _: &mut rune::Cx<()>) -> rune::El {
        rune::row()
            .child(
                crate::ui::beat::packs::progress_ring(Some(0.426), 16.0, "Downloading").key("ring"),
            )
            .child(crate::ui::beat::packs::progress_ring(None, 16.0, "Waiting").key("spin"))
    }
    let mut r = rune::Runtime::new((), view, (200.0, 100.0), rune::Env::new(true, 1.0));
    r.frame(0.0);
    let j = r.accessibility_json();
    let ring = node(&j, "ring").unwrap();
    assert_eq!(ring["role"], "progress-indicator");
    assert_eq!(&range(&ring)[..3], &[43.0, 0.0, 100.0]);
    assert!(
        node(&j, "spin").unwrap().get("range").is_none(),
        "indeterminate"
    );
}

/// Phase-8 review #6: the Song view's range controls follow the same pattern (header volume,
/// header pan, mixer faders) and regions read as selected, not checked.
#[test]
fn song_volume_pan_and_faders_are_keyboard_ranges_and_regions_read_selected() {
    let mut r = song_rt();
    let b = crate::scenes_song::track_id(&r.state, "Bass");
    let vol_key = format!("vol-{b}");
    let v0 = r.state.find_track(&b).unwrap().volume;
    let vol = a11y(&r, &vol_key);
    assert_eq!(vol["role"], "slider");
    assert_eq!(vol["name"], "Track volume");
    assert_eq!(range(&vol), vec![v0, -40.0, 0.0, 1.0]);
    assert_eq!(vol["value"], format!("{} dB", crate::song::js_num(v0)));
    has_step_actions(&vol);
    assert!(r.accessibility_action(&vol_key, Action::Decrement, None));
    frames(&mut r, 2);
    assert_eq!(r.state.find_track(&b).unwrap().volume, v0 - 1.0);
    // Keyboard: focused by a press, arrows ±1, Home / End the ends.
    let (x, y) = at(&r, &vol_key);
    click_at(&mut r, x, y);
    press(&mut r, ev(Key::Right, Mods::default()));
    let v1 = r.state.find_track(&b).unwrap().volume;
    press(&mut r, ev(Key::Left, Mods::default()));
    assert_eq!(r.state.find_track(&b).unwrap().volume, v1 - 1.0);
    press(&mut r, ev(Key::Home, Mods::default()));
    assert_eq!(r.state.find_track(&b).unwrap().volume, -40.0);
    press(&mut r, ev(Key::End, Mods::default()));
    assert_eq!(r.state.find_track(&b).unwrap().volume, 0.0);

    let pan_key = format!("pan-{b}");
    let pan = a11y(&r, &pan_key);
    let p0 = r.state.find_track(&b).unwrap().pan;
    assert_eq!(range(&pan), vec![p0, -1.0, 1.0, 0.05]);
    assert_eq!(pan["value"], crate::song::pan_label(p0));
    has_step_actions(&pan);
    assert!(r.accessibility_action(&pan_key, Action::Increment, None));
    frames(&mut r, 2);
    assert!((r.state.find_track(&b).unwrap().pan - (p0 + 0.05)).abs() < 1e-9);
    click(&mut r, &pan_key);
    press(&mut r, ev(Key::End, Mods::default()));
    assert_eq!(r.state.find_track(&b).unwrap().pan, 1.0);
    assert_eq!(a11y(&r, &pan_key)["value"], "R100");
    press(&mut r, ev(Key::Home, Mods::default()));
    assert_eq!(r.state.find_track(&b).unwrap().pan, -1.0);

    // Mixer fader.
    r.state.song.show_mixer = true;
    r.invalidate();
    run(&mut r, 0.3);
    let fk = format!("fader-{b}");
    let f = a11y(&r, &fk);
    assert_eq!(f["role"], "slider");
    assert_eq!(range(&f), vec![0.0, -40.0, 0.0, 1.0]);
    assert_eq!(f["value"], "0 dB");
    has_step_actions(&f);
    assert!(r.accessibility_action(&fk, Action::Decrement, None));
    frames(&mut r, 2);
    assert_eq!(r.state.find_track(&b).unwrap().volume, -1.0);
    assert_eq!(a11y(&r, &fk)["value"], "-1 dB");
    r.state.song.show_mixer = false;
    r.invalidate();
    run(&mut r, 0.3);

    // A selected region: `aria-selected`, no checked state.
    let reg = r.state.find_track(&b).unwrap().regions[0].id.clone();
    click(&mut r, &format!("region-{reg}"));
    let n = a11y(&r, &format!("region-{reg}"));
    assert!(strs(&n, "states").iter().any(|s| s == "selected"), "{n}");
    assert!(n.get("checked").is_none(), "{n}");
}
