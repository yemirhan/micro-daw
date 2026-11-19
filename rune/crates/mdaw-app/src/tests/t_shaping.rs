//! The pad editor's sound-shaping row: tabs, knobs, the draggable amp envelope, undo, copy /
//! paste and the layout at the default window size.

use mdaw_model::types::PadFilterType;
use rune::{Key, Mods};

use super::harness::*;
use crate::ui::beat::shaping::ShapeTab;

#[test]
fn tabs_show_their_section() {
    let mut r = rt();
    assert_eq!(r.state.beat.shape_tab, ShapeTab::Amp);
    for k in [
        "knob-attack",
        "knob-hold",
        "knob-amp-decay",
        "knob-sustain",
        "knob-decay",
        "amp-env",
    ] {
        assert!(has(&r, k), "{k}");
    }
    assert!(!has(&r, "knob-filter"));
    click(&mut r, "shape-tab-1");
    for k in [
        "filter-type",
        "knob-filter",
        "knob-resonance",
        "knob-filter-env",
        "knob-filter-env-decay",
        "filter-env-curve",
    ] {
        assert!(has(&r, k), "{k}");
    }
    click(&mut r, "shape-tab-2");
    assert!(has(&r, "knob-pitch-env") && has(&r, "knob-pitch-env-decay"));
    click(&mut r, "shape-tab-3");
    assert!(has(&r, "knob-drive") && has(&r, "drive-curve"));
    assert!(!has(&r, "amp-env"));
}

#[test]
fn shaping_knobs_edit_the_pad_in_one_undo_step() {
    let mut r = rt();
    assert!(pad(&r, 0).shape.is_neutral());
    click(&mut r, "knob-sustain");
    press(&mut r, ev(Key::Home, Mods::default()));
    assert_eq!(pad(&r, 0).shape.amp_sustain, 0.0);
    assert_eq!(undo_label(&r).as_deref(), Some("Edit pad"));
    // Drag the attack knob up 100 px: half way → 2·0.5³ = 0.25 s.
    let (x, y) = at(&r, "knob-attack");
    drag(&mut r, x, y, 0.0, -100.0);
    assert!((pad(&r, 0).shape.amp_attack - 0.25).abs() < 1e-9);
    r.state.undo();
    frames(&mut r, 2);
    assert_eq!(pad(&r, 0).shape.amp_attack, 0.0, "the drag undoes at once");

    click(&mut r, "shape-tab-1");
    click(&mut r, "filter-type-1");
    assert_eq!(pad(&r, 0).shape.filter_type, PadFilterType::Hp);
    click(&mut r, "knob-filter-env");
    press(&mut r, ev(Key::End, Mods::default()));
    assert_eq!(pad(&r, 0).shape.filter_env, 6.0);
    click(&mut r, "shape-tab-2");
    click(&mut r, "knob-pitch-env");
    press(&mut r, ev(Key::Right, Mods::default()));
    // tick = max(1, 72 × 0.01) = 1 st.
    assert_eq!(pad(&r, 0).shape.pitch_env, 1.0);
    click(&mut r, "shape-tab-3");
    click(&mut r, "knob-drive");
    press(&mut r, ev(Key::End, Mods::default()));
    assert_eq!(pad(&r, 0).shape.drive, 1.0);
    // The texts follow.
    let texts: Vec<String> = r.elements().iter().filter_map(|e| e.text.clone()).collect();
    assert!(texts.iter().any(|t| t == "100%"), "{texts:?}");
}

#[test]
fn the_amp_envelope_handles_drag() {
    let mut r = rt();
    let [x0, y0, w, h] = rect(&r, "amp-env");
    let g = crate::ui::beat::shaping::env_geom(&pad(&r, 0).shape, pad(&r, 0).decay);
    // The decay / sustain point: drag it down to the bottom → sustain 0.
    let [hx, hy] = g.handles[2];
    drag(&mut r, x0 + hx, y0 + hy, 0.0, h);
    let s = pad(&r, 0).shape;
    assert_eq!(s.amp_sustain, 0.0);
    assert!(r.state.edit_group.is_none(), "release closed the edit");
    assert_eq!(undo_label(&r).as_deref(), Some("Edit pad"));
    // The release point: drag it to the right end → the pad's decay (release) grows.
    let before = pad(&r, 0).decay;
    let g = crate::ui::beat::shaping::env_geom(&pad(&r, 0).shape, before);
    let [hx, hy] = g.handles[3];
    drag(&mut r, x0 + hx, y0 + hy, w - hx, 0.0);
    assert!(
        pad(&r, 0).decay > before,
        "{} → {}",
        before,
        pad(&r, 0).decay
    );
    // Attack and hold are both 0: their points are stacked at the start, and the outer one
    // (hold) moves first.
    let g = crate::ui::beat::shaping::env_geom(&pad(&r, 0).shape, pad(&r, 0).decay);
    assert_eq!(g.handles[0], g.handles[1]);
    let [hx, hy] = g.handles[1];
    drag(&mut r, x0 + hx, y0 + hy, 15.0, 0.0);
    assert!(pad(&r, 0).shape.amp_hold > 0.0);
    assert_eq!(pad(&r, 0).shape.amp_attack, 0.0);
    // Now the attack point stands alone: drag it right.
    let g = crate::ui::beat::shaping::env_geom(&pad(&r, 0).shape, pad(&r, 0).decay);
    let [hx, hy] = g.handles[0];
    drag(&mut r, x0 + hx, y0 + hy, 10.0, 0.0);
    assert!(pad(&r, 0).shape.amp_attack > 0.0);
}

#[test]
fn copy_paste_carries_the_shaping_and_clears_it() {
    let mut r = rt();
    r.state
        .edit_pad(0, serde_json::json!({ "drive": 0.5, "ampSustain": 0.3 }));
    r.state.end_pad_edit();
    r.state.copy_pad(0);
    r.state.paste_pad(3);
    frames(&mut r, 2);
    assert_eq!(pad(&r, 3).shape, pad(&r, 0).shape);
    // An unshaped pad pasted over a shaped one clears its shaping.
    r.state.copy_pad(5);
    r.state.paste_pad(3);
    frames(&mut r, 2);
    assert!(pad(&r, 3).shape.is_neutral());
}

#[test]
fn the_shaping_row_fits_the_editor_at_the_default_size() {
    let mut r = rt();
    for tab in 0..4 {
        click(&mut r, &format!("shape-tab-{tab}"));
        run(&mut r, 0.5);
        let ed = rect(&r, "pad-editor");
        let row = rect(&r, "shape-row");
        assert!(
            row[1] + row[3] <= ed[1] + ed[3] + 0.5,
            "tab {tab}: row {row:?} outside the editor {ed:?}"
        );
        assert!(
            row[0] + row[2] <= ed[0] + ed[2] + 0.5,
            "tab {tab}: too wide"
        );
        // The waveform keeps its minimum height.
        assert!(rect(&r, "trim")[3] >= 96.0 - 0.5, "tab {tab}");
    }
}
