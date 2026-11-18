//! Focus, unmount and quit lifecycles (phase-7 review findings 3, 4, 6, 10, 11, 12): what the
//! TS does in `blur`, effect cleanups, `beforeunload` / `pagehide` and `before-quit`.

use mdaw_engine::engine::TransportState;
use rune::{Key, Mods};

use super::harness::*;
use crate::app::{AppMode, DawView, PadHold};

/// Pads held through the router (`drumPress` without its `drumRelease` yet) + synth notes.
fn router_notes(r: &rune::Runtime<crate::App>) -> usize {
    r.state.svc.held_pads().len() + r.state.active_notes().len()
}

fn enter_hold_on_a1(r: &mut rune::Runtime<crate::App>) {
    click(r, "pad-0"); // focus (the pointer press is released on pointer up)
    assert_eq!(r.state.beat.hold, None);
    r.key(ev(Key::Enter, Mods::default()));
    frames(r, 2);
    assert_eq!(r.state.beat.hold, Some(PadHold::Key(0)));
    assert_eq!(router_notes(r), 1, "the drum press is held");
}

#[test]
fn an_enter_held_pad_is_released_when_the_beat_view_goes() {
    let mut r = rt();
    enter_hold_on_a1(&mut r);
    // Switch to Song while Enter is still down (the top bar's switch; Tab inside the pad grid
    // moves focus, as in the TS): the Beat view unmounts.
    r.state.view = DawView::Song;
    r.invalidate();
    frames(&mut r, 2);
    assert_eq!(r.state.beat.hold, None, "released on unmount");
    assert_eq!(router_notes(&r), 0, "drum release sent");
    // The key-up after the switch finds nothing; later presses are not blocked.
    release(&mut r, ev(Key::Enter, Mods::default()));
    r.state.view = DawView::Beat;
    r.invalidate();
    frames(&mut r, 2);
    click(&mut r, "pad-2");
    r.key(ev(Key::Enter, Mods::default()));
    frames(&mut r, 2);
    assert_eq!(r.state.beat.hold, Some(PadHold::Key(2)));
    release(&mut r, ev(Key::Enter, Mods::default()));
    assert_eq!(r.state.beat.hold, None);
    // Leaving the DAW mode (⌘2) unmounts it as well.
    enter_hold_on_a1(&mut r);
    press(&mut r, ev(Key::Command('2'), cmd()));
    assert_eq!(r.state.mode, AppMode::Samples);
    assert_eq!(r.state.beat.hold, None);
    assert_eq!(router_notes(&r), 0);
}

#[test]
fn an_enter_held_pad_is_released_when_focus_leaves_it() {
    let mut r = rt();
    enter_hold_on_a1(&mut r);
    click(&mut r, "bpm"); // focus moves to the BPM field
    assert_eq!(r.state.beat.hold, None);
    assert_eq!(router_notes(&r), 0);
}

#[test]
fn a_typed_bpm_commits_when_its_field_loses_focus() {
    let mut r = rt();
    click(&mut r, "bpm");
    assert!(r.state.beat.bpm_edit.is_some());
    assert_eq!(r.state.focus_seen.as_deref(), Some("bpm-input"));
    r.state.beat.bpm_edit = Some(rune::TextEdit::new("98"));
    r.invalidate();
    frames(&mut r, 1);
    // Click away (a pad): the field blurs, `onBlur={commit}`.
    click(&mut r, "pad-3");
    assert!(r.state.beat.bpm_edit.is_none());
    assert_eq!(r.state.svc.engine.doc().get_bpm(), 98.0);
    // An invalid draft is discarded on blur, the tempo kept.
    click(&mut r, "bpm");
    r.state.beat.bpm_edit = Some(rune::TextEdit::new("."));
    r.invalidate();
    frames(&mut r, 1);
    click(&mut r, "pad-4");
    assert!(r.state.beat.bpm_edit.is_none());
    assert_eq!(r.state.svc.engine.doc().get_bpm(), 98.0);
}

#[test]
fn shutdown_flushes_the_pending_autosave_and_stops_everything() {
    let mut r = rt();
    r.state.svc.save_headless = true;
    let key = mdaw_model::settings::ARRANGEMENT_STORAGE_KEY;
    let saved = |r: &rune::Runtime<crate::App>| {
        r.state
            .svc
            .settings
            .borrow()
            .get_item(key)
            .map(str::to_owned)
    };
    // The debounce writes 300 ms after a change.
    r.state.set_bpm(101.0);
    frames(&mut r, 2);
    assert!(r.state.svc.save_pending());
    run(&mut r, 0.4);
    assert!(!r.state.svc.save_pending());
    assert!(saved(&r).unwrap().contains("101"));
    // A change, then quit before the debounce: the save is flushed at once.
    r.state.set_bpm(133.0);
    r.state.toggle_play();
    frames(&mut r, 2);
    assert_eq!(r.state.transport(), TransportState::Playing);
    assert!(r.state.svc.save_pending());
    r.state.shutdown();
    assert!(!r.state.svc.save_pending());
    let json = saved(&r).unwrap();
    let loaded = mdaw_model::doc::ArrangementDoc::new();
    let mut loaded = loaded;
    assert!(matches!(
        loaded.load_from_local_storage(Some(&json)),
        Ok(Some(_))
    ));
    assert_eq!(
        loaded.get_bpm(),
        133.0,
        "the last change is in the flushed save"
    );
    assert_eq!(r.state.transport(), TransportState::Stopped);
    assert_eq!(router_notes(&r), 0);
    // Runs once (close and quit hooks both call it).
    r.state.set_bpm(90.0);
    r.state.shutdown();
    assert!(r.state.svc.save_pending(), "the second call is a no-op");
}

#[test]
fn a_typed_bpm_is_committed_by_the_quit_flush() {
    let mut r = rt();
    r.state.svc.save_headless = true;
    click(&mut r, "bpm");
    r.state.beat.bpm_edit = Some(rune::TextEdit::new("77"));
    r.state.shutdown();
    let json = r
        .state
        .svc
        .settings
        .borrow()
        .get_item(mdaw_model::settings::ARRANGEMENT_STORAGE_KEY)
        .map(str::to_owned)
        .unwrap();
    let mut d = mdaw_model::doc::ArrangementDoc::new();
    let _ = d.load_from_local_storage(Some(&json));
    assert_eq!(d.get_bpm(), 77.0);
}

#[test]
fn closing_the_browser_stops_its_preview() {
    let mut r = rt();
    let d = std::env::temp_dir().join(format!("mdaw-preview-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("Kick.wav"), b"RIFF").unwrap();
    press(&mut r, text_key("b"));
    r.state.browser_add_root(&d.to_string_lossy());
    r.state.browser.settle(std::time::Duration::from_secs(5));
    r.invalidate();
    run(&mut r, 0.2);
    // Start a preview, and close the browser before its decode finished.
    let path = d.join("Kick.wav").to_string_lossy().into_owned();
    r.state.browser_preview(&path);
    assert!(r.state.svc.packs.snapshot().previewing.is_some());
    press(&mut r, text_key("b"));
    assert!(!r.state.browser_open);
    assert_eq!(r.state.browser.preview, None);
    assert!(r.state.svc.packs.snapshot().previewing.is_none());
    // The pending decode's result is dropped (its token is stale): nothing starts later.
    let _ = r
        .state
        .svc
        .packs
        .settle(&mut r.state.svc.engine, std::time::Duration::from_secs(5));
    run(&mut r, 0.2);
    assert!(r.state.svc.packs.snapshot().previewing.is_none());
    // Leaving the DAW mode with the browser open stops it too.
    press(&mut r, text_key("b"));
    r.state.browser_preview(&path);
    press(&mut r, ev(Key::Command('3'), cmd()));
    assert!(r.state.svc.packs.snapshot().previewing.is_none());
}

#[test]
fn the_dock_keyboard_glides_and_releases_when_the_pointer_leaves() {
    let mut r = rt();
    click(&mut r, "dock-collapse");
    run(&mut r, 0.3);
    let c4 = rect(&r, "key-60");
    let d4 = rect(&r, "key-62");
    // Low on the keys: white keys only (the black keys are 76 of 112 points tall).
    let y = c4[1] + c4[3] - 10.0;
    let (x0, x1) = (c4[0] + c4[2] / 2.0, d4[0] + d4[2] / 2.0);
    r.pointer_move(x0, y);
    r.pointer_down(x0, y);
    frames(&mut r, 2);
    assert_eq!(r.state.active_notes(), vec![60]);
    // Slide to D4: C4 released, D4 pressed (glissando).
    for i in 1..=6 {
        r.pointer_move(x0 + (x1 - x0) * i as f32 / 6.0, y);
        r.frame(DT);
    }
    frames(&mut r, 1);
    assert_eq!(r.state.active_notes(), vec![62]);
    assert_eq!(r.state.dock_note, Some(62));
    // Off the keyboard (below it): released, and coming back does not play again.
    r.pointer_move(x1, y + 60.0);
    frames(&mut r, 2);
    assert!(r.state.active_notes().is_empty());
    r.pointer_move(x1, y);
    frames(&mut r, 2);
    assert!(
        r.state.active_notes().is_empty(),
        "the gesture ended on leave"
    );
    r.pointer_up(x1, y);
    frames(&mut r, 2);
    // A press, then collapsing the dock (unmount) releases it.
    r.pointer_move(x0, y);
    r.pointer_down(x0, y);
    frames(&mut r, 2);
    assert_eq!(r.state.active_notes(), vec![60]);
    r.state.dock_open = false;
    r.invalidate();
    frames(&mut r, 2);
    assert!(r.state.active_notes().is_empty());
    r.pointer_up(x0, y);
    frames(&mut r, 2);
}

#[test]
fn a_missing_pack_requests_the_catalog_by_itself() {
    use mdaw_model::types::{PadSample, PadSampleSource};
    let mut r = rt();
    assert!(r.state.svc.packs.snapshot().store.catalog.is_none());
    let mut kit = r.state.kit();
    kit.pads[0].sample = Some(PadSample {
        name: "P0".into(),
        source: PadSampleSource::Pack {
            pack_id: "uzu-drumkit".into(),
            file: "0.wav".into(),
        },
    });
    r.state.load_kit(&kit);
    // The installed list arrives first ("missing" needs it), then the frame asks for the
    // catalog (`useCatalogWhile(missing.length > 0)`).
    let _ = r
        .state
        .svc
        .packs
        .settle(&mut r.state.svc.engine, std::time::Duration::from_secs(5));
    frames(&mut r, 2);
    let _ = r
        .state
        .svc
        .packs
        .settle(&mut r.state.svc.engine, std::time::Duration::from_secs(5));
    frames(&mut r, 2);
    assert!(
        r.state.svc.packs.snapshot().store.catalog.is_some(),
        "catalog loaded without a manual load_catalog"
    );
}
