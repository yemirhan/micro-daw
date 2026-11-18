//! Start screen, shell, global shortcuts, persisted preferences, computer-keyboard pads, dock.

use mdaw_engine::engine::TransportState;
use mdaw_model::settings as store;
use rune::{Key, Mods};

use super::harness::*;
use crate::app::{App, AppMode, DawView, StartPhase};

#[test]
fn start_screen_starts_on_click_or_any_plain_key_but_not_tab_escape_or_chords() {
    // Click anywhere.
    let mut r = runtime(App::headless());
    assert!(has(&r, "start-button"));
    click_at(&mut r, 50.0, 300.0);
    assert!(matches!(r.state.start, StartPhase::Exiting { .. }));
    run(&mut r, 0.5);
    assert_eq!(r.state.start, StartPhase::Done);
    assert!(has(&r, "beat-view") && !has(&r, "start-button"));
    // Keys: Tab / Escape / a chord don't start; a letter does.
    let mut r = runtime(App::headless());
    press(&mut r, ev(Key::Tab, Mods::default()));
    press(&mut r, ev(Key::Escape, Mods::default()));
    press(&mut r, ev(Key::Command('k'), cmd()));
    assert_eq!(r.state.start, StartPhase::Idle);
    press(&mut r, text_key("a"));
    assert_ne!(r.state.start, StartPhase::Idle);
}

#[test]
fn start_screen_texts() {
    let r = runtime(App::headless());
    let texts: Vec<String> = r.elements().iter().filter_map(|e| e.text.clone()).collect();
    for t in [
        "Start session",
        "Click anywhere or press any key. This turns on audio and MIDI.",
        "micro",
        "daw",
    ] {
        assert!(texts.iter().any(|x| x == t), "missing {t}: {texts:?}");
    }
    assert!(
        texts
            .iter()
            .any(|x| x.starts_with("Audio engine · Standby"))
    );
}

#[test]
fn space_toggles_pattern_play_and_ignores_repeat() {
    let mut r = rt();
    press(&mut r, text_key(" "));
    run(&mut r, 0.2);
    assert_eq!(r.state.transport(), TransportState::Playing);
    assert!(
        r.state.svc.engine.is_pattern_playback(),
        "beat Space = playPattern"
    );
    r.repeat_key(text_key(" "));
    frames(&mut r, 2);
    assert_eq!(
        r.state.transport(),
        TransportState::Playing,
        "auto-repeat ignored"
    );
    press(&mut r, text_key(" "));
    run(&mut r, 0.1);
    assert_eq!(r.state.transport(), TransportState::Stopped);
}

#[test]
fn m_toggles_the_loop_metronome_and_n_note_repeat() {
    let mut r = rt();
    let before = r.state.svc.engine.doc().get_loop_settings().metronome;
    press(&mut r, text_key("m"));
    assert_eq!(
        r.state.svc.engine.doc().get_loop_settings().metronome,
        !before
    );
    assert!(!r.state.repeat.enabled);
    press(&mut r, text_key("n"));
    assert!(r.state.repeat.enabled);
    press(&mut r, text_key("n"));
    assert!(!r.state.repeat.enabled);
}

#[test]
fn shift_backspace_clears_the_pattern_and_cmd_z_undoes() {
    let mut r = rt();
    r.state.toggle_step(0, 0, 0.25);
    r.state.toggle_step(1, 4, 0.25);
    frames(&mut r, 2);
    assert_eq!(notes(&r).len(), 2);
    press(&mut r, ev(Key::Backspace, shift()));
    assert!(notes(&r).is_empty());
    press(&mut r, ev(Key::Command('z'), cmd()));
    assert_eq!(notes(&r).len(), 2, "undo restores");
    press(
        &mut r,
        ev(
            Key::Command('z'),
            Mods {
                shift: true,
                ..cmd()
            },
        ),
    );
    assert!(notes(&r).is_empty(), "redo clears again");
}

#[test]
fn tab_switches_view_b_toggles_browser_and_both_persist_or_not_like_the_ts() {
    let mut r = rt();
    assert!(!r.state.browser_open);
    press(&mut r, text_key("b"));
    assert!(r.state.browser_open);
    assert!(has(&r, "browser"));
    assert!(store::read_browser_open(&r.state.svc.settings.borrow()));
    press(&mut r, ev(Key::Tab, Mods::default()));
    assert_eq!(r.state.view, DawView::Song);
    press(&mut r, ev(Key::Tab, Mods::default()));
    assert_eq!(r.state.view, DawView::Beat);
}

#[test]
fn single_key_shortcuts_are_skipped_while_a_menu_is_open() {
    let mut r = rt();
    click(&mut r, "kit-menu");
    assert!(r.state.menu.is_some());
    press(&mut r, text_key("n"));
    assert!(!r.state.repeat.enabled);
}

#[test]
fn mode_shortcuts_and_sidebar() {
    let mut r = rt();
    press(&mut r, ev(Key::Command('2'), cmd()));
    assert_eq!(r.state.mode, AppMode::Samples);
    press(&mut r, ev(Key::Command('1'), cmd()));
    assert_eq!(r.state.mode, AppMode::Daw);
    click(&mut r, "mode-packs");
    assert_eq!(r.state.mode, AppMode::Packs);
    click(&mut r, "mode-daw");
    click(&mut r, "sidebar-trigger");
    assert!(r.state.sidebar_open);
}

#[test]
fn preferences_read_with_fallbacks_and_write_on_change() {
    let mut s = store::SettingsStore::in_memory();
    s.set_item(store::BEAT_PAD_LAYOUT_KEY, "2x8").unwrap();
    s.set_item(store::BEAT_STEP_GRID_KEY, "bogus").unwrap();
    s.set_item(store::BEAT_KEY_HINTS_KEY, "on").unwrap();
    let app = App::with_settings(crate::services::RunMode::Headless, s);
    assert_eq!(
        app.beat.layout,
        mdaw_model::beat_helpers::PadLayout::TwoByEight
    );
    assert_eq!(
        app.beat.grid,
        mdaw_model::beat_helpers::StepGrid::S16,
        "invalid → default"
    );
    assert!(app.beat.show_keys);
    let mut r = rt();
    click(&mut r, "pad-layout-1");
    assert_eq!(
        store::read_beat_pad_layout(&r.state.svc.settings.borrow()),
        "2x8"
    );
    click(&mut r, "step-size-0");
    assert_eq!(
        store::read_beat_step_grid(&r.state.svc.settings.borrow()),
        "1/8"
    );
    click(&mut r, "key-hints");
    assert!(store::read_beat_key_hints(&r.state.svc.settings.borrow()));
}

#[test]
fn computer_keys_play_pads_record_and_release() {
    let mut r = rt();
    r.state
        .set_loop_settings(mdaw_model::doc::LoopSettingsPatch {
            count_in_bars: Some(0),
            quantize: Some(mdaw_model::types::QuantizeGrid::Q16),
            ..Default::default()
        });
    // R starts loop recording (no count-in).
    press(&mut r, text_key("r"));
    run(&mut r, 0.3);
    assert!(r.state.loop_recording());
    // Z = pad 0 (A1), velocity 0.85; held, auto-repeat ignored, released on key up.
    press(&mut r, text_key("z"));
    assert_eq!(
        r.state.held_keys.values().copied().collect::<Vec<_>>(),
        vec![0]
    );
    r.repeat_key(text_key("z"));
    frames(&mut r, 1);
    release(&mut r, text_key("z"));
    assert!(r.state.held_keys.is_empty());
    // Shift + a pad key passes through; T is pad 11 (R is record).
    press(&mut r, ev(Key::Text("X".into()), shift()));
    assert!(r.state.held_keys.is_empty());
    press(&mut r, text_key("t"));
    release(&mut r, text_key("t"));
    run(&mut r, 0.3);
    press(&mut r, text_key("r"));
    run(&mut r, 0.2);
    assert!(!r.state.loop_recording());
    let n = notes(&r);
    assert!(
        n.iter()
            .any(|(p, _, v)| *p == 0 && (*v - 0.85).abs() < 1e-6),
        "{n:?}"
    );
    assert!(n.iter().any(|(p, _, _)| *p == 11), "{n:?}");
    assert_eq!(n.len(), 2, "one hit per press: {n:?}");
}

#[test]
fn window_blur_releases_held_keys() {
    let mut r = rt();
    press(&mut r, text_key("a"));
    assert_eq!(r.state.held_keys.len(), 1);
    r.window_focus(false);
    frames(&mut r, 2);
    assert!(r.state.held_keys.is_empty());
}

#[test]
fn pad_keys_are_ignored_while_typing_in_a_field() {
    let mut r = rt();
    click(&mut r, "kit-menu");
    click(&mut r, "kit-name");
    press(&mut r, text_key("z"));
    assert!(r.state.held_keys.is_empty());
}

#[test]
fn dock_expands_and_routing_mode_changes() {
    let mut r = rt();
    assert!(!has(&r, "dock-keys"), "collapsed by default");
    click(&mut r, "dock-collapse");
    assert!(has(&r, "dock-keys"));
    click(&mut r, "routing-2");
    assert_eq!(
        r.state.svc.midi.host.shared().lock().router.routing_mode(),
        mdaw_midi::RoutingMode::Split
    );
    // A key on the on-screen keyboard plays the live synth through the router.
    let (x, y) = at(&r, "key-60");
    r.pointer_move(x, y);
    r.pointer_down(x, y);
    frames(&mut r, 2);
    assert_eq!(r.state.active_notes(), vec![60]);
    r.pointer_up(x, y);
    frames(&mut r, 2);
    assert!(r.state.active_notes().is_empty());
}

#[test]
fn undo_redo_buttons_and_dirty_dot() {
    let mut r = rt();
    assert!(!has(&r, "dirty-dot"));
    r.state.toggle_step(0, 0, 0.25);
    frames(&mut r, 2);
    assert!(has(&r, "dirty-dot"));
    click(&mut r, "undo");
    assert!(notes(&r).is_empty());
    click(&mut r, "redo");
    assert_eq!(notes(&r).len(), 1);
}

#[test]
fn starter_project_has_one_active_drums_track() {
    let r = rt();
    let doc = r.state.svc.engine.doc();
    let id = doc.get_active_drum_track_id().expect("starter drum track");
    assert_eq!(
        doc.get_tracks().iter().find(|t| t.id == id).unwrap().name,
        "Drums"
    );
    assert!(!doc.undo_manager().can_undo());
}

/// Review finding 15: R records by its physical key (`e.code === 'KeyR'`), like the pad keys:
/// on a layout where the key under R types another letter it still records, and a layout's
/// "r" on another key does not. Keys with no physical origin fall back to the character.
#[test]
fn the_record_shortcut_is_the_physical_r_key() {
    use crate::keys::is_record_key;
    assert!(is_record_key(Some("KeyR"), &text_key("p"))); // Dvorak: R types "p"
    assert!(!is_record_key(Some("KeyO"), &text_key("r"))); // Dvorak's "r" is on KeyO
    assert!(is_record_key(None, &text_key("R")));
    assert!(!is_record_key(None, &text_key("t")));
}
