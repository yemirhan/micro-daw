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

/// Press (down + up) at `(x, y)`; returns the titlebar action the down asked the window for.
fn titlebar_press(r: &mut rune::Runtime<App>, x: f32, y: f32) -> Option<rune::TitlebarAction> {
    r.pointer_move(x, y);
    r.pointer_down(x, y);
    let action = r.take_titlebar_action();
    r.pointer_up(x, y);
    r.frame(0.01);
    action
}

#[test]
fn the_topbar_is_the_titlebar_band_and_its_empty_space_drags_and_double_click_zooms() {
    use rune::TitlebarAction::{DoubleClick, Drag};
    let mut r = rt();
    // The window's titlebar is the topbar row: the traffic lights centre in its 44 points.
    let bar = rect(&r, "topbar");
    assert_eq!(crate::titlebar().height(), bar[3]);
    // An empty spot of the centre column (its content sits in the middle).
    let centre = rect(&r, "topbar-centre");
    let (x, y) = (centre[0] + 6.0, bar[1] + bar[3] / 2.0);
    assert_eq!(
        titlebar_press(&mut r, x, y),
        Some(Drag),
        "a press moves the window"
    );
    assert_eq!(
        titlebar_press(&mut r, x, y),
        Some(DoubleClick),
        "a double click takes the system's titlebar action"
    );
    run(&mut r, 1.0);
    // Controls keep their presses: no drag, no zoom.
    let (fx, fy) = at(&r, "fullscreen");
    assert_eq!(titlebar_press(&mut r, fx, fy), None);
    run(&mut r, 1.0);
    let (vx, vy) = at(&r, "view-switch");
    assert_eq!(titlebar_press(&mut r, vx, vy), None);
    assert_eq!(titlebar_press(&mut r, vx, vy), None);
}

/// Select triggers (`combobox`) whose content (a chevron, a label) reaches outside their box.
fn overflowing_selects(r: &rune::Runtime<App>) -> Vec<String> {
    let els = r.elements();
    let mut out = Vec::new();
    for t in els.iter().filter(|e| e.role == "combobox" && !e.exiting) {
        let [x, y, w, h] = t.rect;
        if w <= 0.0 {
            continue;
        }
        let prefix = format!("{}/", t.path);
        for c in els.iter().filter(|c| c.path.starts_with(&prefix)) {
            let [cx, cy, cw, ch] = c.rect;
            if cw <= 0.0 || ch <= 0.0 {
                continue;
            }
            if cx < x - 0.5 || cy < y - 0.5 || cx + cw > x + w + 0.5 || cy + ch > y + h + 0.5 {
                out.push(format!(
                    "{} ({:?}) > {:?} at {:?} in {:?}",
                    t.key.as_deref().unwrap_or(&t.path),
                    t.label,
                    c.text.as_deref().unwrap_or(&c.path),
                    c.rect,
                    t.rect
                ));
            }
        }
    }
    out
}

#[test]
fn select_triggers_keep_their_chevron_and_label_inside_the_pill() {
    let mut bad = Vec::new();
    for width in [1360.0, 1440.0, 1600.0] {
        // Beat view: instrument ("KEYS Drawbar Organ"), MIDI input, choke, kit menus.
        let mut r = rt();
        r.state.preset = ("Drawbar Organ".into(), "Keys");
        r.set_viewport((width, 900.0), 1.0);
        r.invalidate();
        run(&mut r, 1.0);
        bad.extend(
            overflowing_selects(&r)
                .into_iter()
                .map(|b| format!("beat {width}: {b}")),
        );
        // Song view (toolbar, transport, track headers, dock) and its piano roll.
        let mut r = song_rt();
        r.state.preset = ("Drawbar Organ".into(), "Keys");
        r.set_viewport((width, 900.0), 1.0);
        r.invalidate();
        run(&mut r, 1.0);
        bad.extend(
            overflowing_selects(&r)
                .into_iter()
                .map(|b| format!("song {width}: {b}")),
        );
        let b = tid(&r, "Bass");
        let id = rid(&r, &b, 0);
        r.state.open_piano_roll(&b, &id);
        r.invalidate();
        run(&mut r, 1.0);
        bad.extend(
            overflowing_selects(&r)
                .into_iter()
                .map(|b| format!("piano {width}: {b}")),
        );
    }
    assert!(
        bad.is_empty(),
        "overflowing select triggers:\n{}",
        bad.join("\n")
    );
}

/// Slider thumbs (round, filled descendants of a `slider`) that a filled element painted after
/// them covers: e.g. a track segment drawn over half the thumb.
fn covered_thumbs(r: &rune::Runtime<App>) -> Vec<String> {
    let els = r.elements();
    let mut out = Vec::new();
    let filled = |e: &rune::ElementInfo| e.fill.a > 0.0 && e.rect[2] > 0.0 && e.rect[3] > 0.0;
    for s in els.iter().filter(|e| e.role == "slider" && !e.exiting) {
        let prefix = format!("{}/", s.path);
        let inner: Vec<_> = els.iter().filter(|e| e.path.starts_with(&prefix)).collect();
        for (i, t) in inner.iter().enumerate() {
            let [x, y, w, h] = t.rect;
            let round = filled(t) && (w - h).abs() < 0.5 && w >= 8.0 && t.radius >= w / 2.0 - 1.5;
            if !round {
                continue;
            }
            let own = format!("{}/", t.path);
            for c in inner[i + 1..]
                .iter()
                .filter(|c| filled(c) && !c.path.starts_with(&own))
            {
                let [cx, cy, cw, ch] = c.rect;
                let overlap = (x + w).min(cx + cw) - x.max(cx) > 0.5
                    && (y + h).min(cy + ch) - y.max(cy) > 0.5;
                if overlap {
                    out.push(format!("{}: {} over {}", s.path, c.path, t.path));
                }
            }
        }
    }
    out
}

#[test]
fn slider_thumbs_paint_over_their_tracks() {
    let mut r = song_rt();
    let b = tid(&r, "Bass");
    let id = rid(&r, &b, 0);
    r.state.open_piano_roll(&b, &id);
    r.invalidate();
    run(&mut r, 1.0);
    assert!(has(&r, "pr-zoom-thumb"));
    let mut bad = covered_thumbs(&r);
    // The track headers' volume, the mixer, the topbar's output volume, the Beat view's.
    bad.extend(covered_thumbs(&song_rt()));
    bad.extend(covered_thumbs(&rt()));
    assert!(bad.is_empty(), "covered slider thumbs:\n{}", bad.join("\n"));
    // The thumb sits on the end of the fill (centre = 6 + f·(w − 12), as the drag maps it).
    let (s, t) = (rect(&r, "pr-zoom-slider"), rect(&r, "pr-zoom-thumb"));
    assert!(t[0] >= s[0] - 0.01 && t[0] + t[2] <= s[0] + s[2] + 0.01);
    assert_eq!(t[1] + t[3] / 2.0, s[1] + s[3] / 2.0, "vertically centred");
}
