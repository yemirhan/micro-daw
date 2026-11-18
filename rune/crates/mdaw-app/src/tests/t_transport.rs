//! BeatTransport: play / record, LCD, BPM field, loop / record settings, swing, note repeat.

use mdaw_engine::engine::TransportState;
use mdaw_model::types::{NoteRepeatRate, QuantizeGrid, SwingGrid};
use rune::{Key, Mods};

use super::harness::*;
use crate::app::Menu;

fn bpm(r: &rune::Runtime<crate::App>) -> f64 {
    r.state.svc.engine.doc().get_bpm()
}

fn ls(r: &rune::Runtime<crate::App>) -> mdaw_model::types::LoopRecordSettings {
    r.state.svc.engine.doc().get_loop_settings()
}

#[test]
fn play_button_plays_the_pattern_and_stops() {
    let mut r = rt();
    click(&mut r, "play");
    run(&mut r, 0.3);
    assert_eq!(r.state.transport(), TransportState::Playing);
    let e = r
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("play"))
        .unwrap();
    assert_eq!(e.label.as_deref(), Some("Stop"));
    click(&mut r, "play");
    run(&mut r, 0.1);
    assert_eq!(r.state.transport(), TransportState::Stopped);
}

#[test]
fn record_button_arms_counts_in_and_stop_ends_the_take() {
    let mut r = rt();
    click(&mut r, "record");
    run(&mut r, 0.2);
    assert!(r.state.loop_recording());
    let e = r
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("record"))
        .unwrap();
    assert_eq!(e.label.as_deref(), Some("Stop recording"));
    // Play/Stop reads "Stop" while armed; pressing it stops the take and the transport.
    click(&mut r, "play");
    run(&mut r, 0.1);
    assert!(!r.state.loop_recording());
    assert_eq!(r.state.transport(), TransportState::Stopped);
}

#[test]
fn loop_position_follows_the_transport_and_resets() {
    let mut r = rt();
    assert_eq!(r.state.live.loop_pos.get().as_ref(), "1.1");
    r.state.toggle_play();
    run(&mut r, 0.6); // 120 BPM: past beat 2
    assert_eq!(r.state.live.loop_pos.get().as_ref(), "1.2");
    run(&mut r, 1.6); // past beat 5 → bar 2
    assert_eq!(r.state.live.loop_pos.get().as_ref(), "2.1");
    r.state.toggle_play();
    run(&mut r, 0.1);
    assert_eq!(r.state.live.loop_pos.get().as_ref(), "1.1");
}

#[test]
fn bpm_wheel_drag_click_to_type_and_keys() {
    let mut r = rt();
    let (x, y) = at(&r, "bpm");
    wheel(&mut r, x, y, 10.0);
    frames(&mut r, 2);
    assert_eq!(bpm(&r), 121.0, "wheel up = +1");
    wheel(&mut r, x, y, -10.0);
    frames(&mut r, 2);
    assert_eq!(bpm(&r), 120.0);
    // Vertical drag: 2 px per BPM (up = faster), clamped 40..240.
    drag(&mut r, x, y, 0.0, -20.0);
    assert_eq!(bpm(&r), 130.0);
    drag(&mut r, x, y, 0.0, 2000.0);
    assert_eq!(bpm(&r), 40.0);
    // A click without movement types a value: Enter commits (clamped + rounded).
    click(&mut r, "bpm");
    assert!(r.state.beat.bpm_edit.is_some());
    r.state.beat.bpm_edit = Some(rune::TextEdit::new("98.6"));
    r.state.commit_bpm();
    frames(&mut r, 2);
    assert_eq!(bpm(&r), 99.0);
    // Invalid text is discarded.
    r.state.beat.bpm_edit = Some(rune::TextEdit::new("."));
    r.state.commit_bpm();
    assert_eq!(bpm(&r), 99.0);
    // Arrow keys in the field: ±1, ±10 with Shift.
    click(&mut r, "bpm");
    press(&mut r, ev(Key::Up, shift()));
    assert_eq!(bpm(&r), 109.0);
    click(&mut r, "bpm");
    press(&mut r, ev(Key::Down, Mods::default()));
    assert_eq!(bpm(&r), 108.0);
    // Escape cancels the draft.
    click(&mut r, "bpm");
    press(&mut r, ev(Key::Escape, Mods::default()));
    assert!(r.state.beat.bpm_edit.is_none());
    assert_eq!(bpm(&r), 108.0);
}

#[test]
fn bpm_clamp_matches_min_max() {
    use crate::ui::beat::transport::clamp_bpm;
    assert_eq!(clamp_bpm(10.0), 40.0);
    assert_eq!(clamp_bpm(999.0), 240.0);
    assert_eq!(clamp_bpm(120.4), 120.0);
}

#[test]
fn loop_settings_controls() {
    let mut r = rt();
    assert_eq!(
        (
            ls(&r).bars,
            ls(&r).quantize,
            ls(&r).count_in_bars,
            ls(&r).overdub,
            ls(&r).metronome
        ),
        (2, QuantizeGrid::Q16, 1, true, true)
    );
    click(&mut r, "bars-3");
    assert_eq!(ls(&r).bars, 8);
    click(&mut r, "quantize");
    assert_eq!(r.state.menu, Some(Menu::Quantize));
    let item = r
        .elements()
        .into_iter()
        .find(|e| e.text.as_deref() == Some("1/8T"))
        .map(|e| e.path)
        .unwrap();
    let (x, y) = r.find(&item).unwrap();
    click_at(&mut r, x, y);
    assert_eq!(ls(&r).quantize, QuantizeGrid::Q8T);
    assert!(r.state.menu.is_none());
    click(&mut r, "count-in-bars-0");
    assert_eq!(ls(&r).count_in_bars, 0);
    click(&mut r, "record-mode-1");
    assert!(!ls(&r).overdub);
    click(&mut r, "metronome");
    assert!(!ls(&r).metronome);
}

#[test]
fn swing_knob_and_grid() {
    let mut r = rt();
    let (x, y) = at(&r, "knob-swing");
    // 200 px = full range (0..0.75): 40 px up = 0.15.
    drag(&mut r, x, y, 0.0, -40.0);
    assert!((ls(&r).swing - 0.15).abs() < 1e-9, "{}", ls(&r).swing);
    let texts: Vec<String> = r.elements().iter().filter_map(|e| e.text.clone()).collect();
    assert!(
        texts.iter().any(|t| t == "58%"),
        "50 + 0.15 × 50 = 57.5 → 58%"
    );
    double_click(&mut r, "knob-swing");
    assert_eq!(ls(&r).swing, 0.0, "double-click resets");
    wheel(&mut r, x, y, 5.0);
    frames(&mut r, 2);
    assert!(
        (ls(&r).swing - 0.01).abs() < 1e-9,
        "wheel: max(step, range × 0.015) = 0.01125 → snapped 0.01"
    );
    assert_eq!(ls(&r).swing_grid, SwingGrid::S16);
    click(&mut r, "swing-grid");
    assert_eq!(ls(&r).swing_grid, SwingGrid::S8);
}

#[test]
fn note_repeat_toggle_and_rate() {
    let mut r = rt();
    click(&mut r, "note-repeat");
    assert!(r.state.repeat.enabled);
    click(&mut r, "repeat-rate");
    let item = r
        .elements()
        .into_iter()
        .rfind(|e| e.text.as_deref() == Some("1/32"))
        .map(|e| e.path)
        .unwrap();
    let (x, y) = r.find(&item).unwrap();
    click_at(&mut r, x, y);
    assert_eq!(r.state.repeat.rate, NoteRepeatRate::R32);
}

#[test]
fn transport_section_captions_follow_the_width() {
    let mut r = rt();
    // Browser closed at 1440: the strip is ≥1280 wide → captions and group labels.
    assert!(texts(&r).iter().any(|t| t == "Loop") && texts(&r).iter().any(|t| t == "Bars"));
    press(&mut r, text_key("b"));
    run(&mut r, 0.6);
    // Browser open: ~1100 → neither.
    assert!(!texts(&r).iter().any(|t| t == "Bars"));
}
