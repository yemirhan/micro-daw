//! `inventory/views.md` §2 command palette, §10 export dialog, §11 template picker, the
//! controller setup dialog (`main-packs-midi-project.md` §3.3 learn flows) and the global ⌘K /
//! ⌘E / ⌘I shortcuts.

use rune::Key;

use super::harness::*;
use crate::app::{AppMode, DawView};
use crate::views::Dialog;
use mdaw_model::controller::LearnState;

fn cmd_key(c: char) -> rune::KeyEvent {
    ev(Key::Command(c), cmd())
}

fn type_text(rt: &mut rune::Runtime<crate::App>, t: &str) {
    for c in t.chars() {
        rt.key(text_key(&c.to_string()));
    }
    frames(rt, 2);
}

#[test]
fn palette_opens_filters_runs_and_closes() {
    let mut rt = rt();
    press(&mut rt, cmd_key('k'));
    assert_eq!(rt.state.p9.dialog, Some(Dialog::Palette));
    let t = texts(&rt);
    for s in [
        "Project",
        "Navigation",
        "New Project",
        "navigate",
        "run",
        "close",
        "toggle",
    ] {
        assert!(t.iter().any(|x| x == s), "{s} missing");
    }
    // Typing filters (label + keywords); Enter runs the highlighted command and closes.
    type_text(&mut rt, "preferences");
    let items = crate::ui::p9::dialogs::shown_order(crate::ui::p9::dialogs::palette_items(
        rt.state.mode,
        &rt.state.p9.palette.query.text,
    ));
    assert_eq!(items[0].id, "open-settings");
    press(&mut rt, ev(Key::Enter, Default::default()));
    assert_eq!(rt.state.mode, AppMode::Settings);
    assert!(rt.state.p9.dialog.is_none());
    // DAW-only commands are hidden in other modes.
    let ids: Vec<String> = crate::ui::p9::dialogs::palette_items(AppMode::Settings, "")
        .into_iter()
        .map(|c| c.id)
        .collect();
    assert!(!ids.contains(&"add-synth-track".to_owned()));
    assert!(ids.contains(&"controller-setup".to_owned()));
    // No results.
    press(&mut rt, cmd_key('k'));
    type_text(&mut rt, "qqqqzz");
    assert!(texts(&rt).iter().any(|x| x == "No results found."));
    // Escape closes; ⌘K toggles.
    press(&mut rt, ev(Key::Escape, Default::default()));
    assert!(rt.state.p9.dialog.is_none());
    press(&mut rt, cmd_key('k'));
    press(&mut rt, cmd_key('k'));
    assert!(rt.state.p9.dialog.is_none());
}

#[test]
fn palette_arrows_move_and_commands_act() {
    let mut rt = rt();
    press(&mut rt, cmd_key('k'));
    type_text(&mut rt, "song view");
    press(&mut rt, ev(Key::Down, Default::default()));
    press(&mut rt, ev(Key::Up, Default::default()));
    press(&mut rt, ev(Key::Enter, Default::default()));
    assert_eq!(rt.state.view, DawView::Song);
    // Add Drum Track adds a track (one undo step).
    let n = rt.state.svc.engine.doc().get_tracks().len();
    press(&mut rt, cmd_key('k'));
    type_text(&mut rt, "add drum track");
    press(&mut rt, ev(Key::Enter, Default::default()));
    assert_eq!(rt.state.svc.engine.doc().get_tracks().len(), n + 1);
    assert_eq!(undo_label(&rt).as_deref(), Some("Add Track"));
    // No preset commands: they picked the live sound, which left the UI.
    assert!(crate::ui::p9::dialogs::palette_items(AppMode::Daw, "rhodes").is_empty());
}

#[test]
fn export_dialog_counts_tracks_and_writes_wav_and_midi() {
    let mut rt = song_rt();
    press(&mut rt, cmd_key('e'));
    assert_eq!(rt.state.p9.dialog, Some(Dialog::Export));
    assert!(
        texts(&rt)
            .iter()
            .any(|x| x == "4 of 4 tracks will be included (muted tracks are skipped).")
    );
    for s in [
        "WAV audio",
        "16-bit PCM stereo, rendered offline",
        "MIDI file",
        "Standard MIDI; drums on channel 10",
    ] {
        assert!(texts(&rt).iter().any(|x| x == s), "{s}");
    }
    let dir = std::env::temp_dir().join(format!("mdaw-export-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let wav = dir.join("untitled.wav");
    rt.state
        .export_to(crate::views::ExportKind::Wav, wav.clone());
    frames(&mut rt, 2);
    let bytes = std::fs::read(&wav).unwrap();
    assert_eq!(&bytes[..4], b"RIFF");
    assert_eq!(&bytes[8..12], b"WAVE");
    assert!(bytes.len() > 44 + 48_000, "{}", bytes.len());
    let mid = dir.join("untitled.mid");
    rt.state
        .export_to(crate::views::ExportKind::Midi, mid.clone());
    frames(&mut rt, 2);
    assert_eq!(&std::fs::read(&mid).unwrap()[..4], b"MThd");
    assert!(rt.state.p9.export.running.is_none() && rt.state.p9.export.error.is_none());
    // Escape closes; ⌘E toggles.
    press(&mut rt, ev(Key::Escape, Default::default()));
    assert!(rt.state.p9.dialog.is_none());
    // Nothing to export: both disabled, the subtitle says so.
    let ids: Vec<String> = rt.state.tracks().iter().map(|t| t.id.clone()).collect();
    for id in ids {
        rt.state.svc.engine.edit(|d| d.remove_track(&id));
    }
    press(&mut rt, cmd_key('e'));
    assert!(
        texts(&rt)
            .iter()
            .any(|x| x == "Nothing to export yet. Add a track first.")
    );
    let a11y = rt.accessibility_json().to_string();
    assert!(a11y.contains("WAV audio"));
}

#[test]
fn template_picker_filters_and_loads_a_template() {
    let mut rt = rt();
    rt.state.open_dialog(Dialog::Templates);
    rt.invalidate();
    frames(&mut rt, 2);
    let t = texts(&rt);
    for s in [
        "New from template",
        "Start with tracks, sounds and a groove already set up.",
        "Blank 4-Track",
        "Ambient Pad",
        "120 BPM",
    ] {
        assert!(t.iter().any(|x| x == s), "{s} missing");
    }
    click(&mut rt, "template-tab-jazz");
    assert_eq!(rt.state.p9.template_filter, Some("jazz"));
    assert!(has(&rt, "template-jazz-trio") && !has(&rt, "template-rock-song"));
    click(&mut rt, "template-jazz-trio");
    assert!(rt.state.p9.dialog.is_none());
    // Phase 10: `newProjectFromTemplate` asks where to save the new project first.
    let (kind, o) = rt.file_dialog().expect("save dialog");
    assert_eq!(
        (kind, o.file_name.as_deref()),
        (rune::FileDialogKind::Save, Some("Untitled.mdaw"))
    );
    let f = std::env::temp_dir().join(format!("mdaw-tpl-{}.mdaw", uuid::Uuid::new_v4()));
    rt.resolve_file_dialog(Some(f.clone())).unwrap();
    frames(&mut rt, 2);
    assert!(f.is_file());
    let doc = rt.state.svc.engine.doc();
    assert_eq!(doc.get_bpm(), 130.0);
    let names: Vec<&str> = doc.get_tracks().iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["Drums", "Walking Bass", "Electric Piano"]);
    // A template is a fresh project: not dirty.
    assert!(!rt.state.dirty());
}

#[test]
fn controller_dialog_learn_flows_and_escape() {
    let mut rt = rt();
    click_settle(&mut rt, "controller-setup");
    assert_eq!(rt.state.p9.dialog, Some(Dialog::Controller));
    let t = texts(&rt);
    for s in [
        "Controller setup",
        "Waiting for MIDI… hit a pad or turn a knob.",
        "Learn pads (hit 1–16)",
        "Any channel",
        "Add knob",
        "Reset to default",
        "Done",
    ] {
        assert!(t.iter().any(|x| x == s), "{s} missing");
    }
    // The wizard: pad 1 waits; Backspace / Back stays at 0; Skip moves on.
    click_settle(&mut rt, "learn-pads");
    assert_eq!(
        rt.state.controller_learn_state(),
        LearnState::Pad {
            pad: 0,
            sequence: true
        }
    );
    assert!(texts(&rt).iter().any(|x| x == "Hit pad 1"));
    click_settle(&mut rt, "learn-skip");
    assert_eq!(
        rt.state.controller_learn_state(),
        LearnState::Pad {
            pad: 1,
            sequence: true
        }
    );
    press(&mut rt, ev(Key::Backspace, Default::default()));
    assert_eq!(
        rt.state.controller_learn_state(),
        LearnState::Pad {
            pad: 0,
            sequence: true
        }
    );
    // A note-on is consumed by the learn: pad 1 gets note 60.
    rt.state
        .svc
        .with_router(|r, _| r.mapping.handle_note_on(60, 1.0, 1));
    let note = rt
        .state
        .svc
        .with_router(|r, _| r.mapping.get_profile().pad_map.notes[0]);
    assert_eq!(note, Some(60));
    // Esc cancels a learn first, the second Esc closes.
    press(&mut rt, ev(Key::Escape, Default::default()));
    assert_eq!(rt.state.controller_learn_state(), LearnState::Idle);
    assert_eq!(rt.state.p9.dialog, Some(Dialog::Controller));
    // A single pad; then a knob row; add knob learns into a new row.
    click_settle(&mut rt, "ctl-pad-5");
    assert_eq!(
        rt.state.controller_learn_state(),
        LearnState::Pad {
            pad: 5,
            sequence: false
        }
    );
    click_settle(&mut rt, "learn-cancel");
    let n = rt
        .state
        .svc
        .with_router(|r, _| r.mapping.get_profile().cc_mappings.len());
    click_settle(&mut rt, "add-knob");
    assert!(matches!(
        rt.state.controller_learn_state(),
        LearnState::Knob { index: Some(i), .. } if i == n as i64
    ));
    // A CC is bound to the row and learn ends.
    rt.state
        .svc
        .with_router(|r, _| r.mapping.handle_cc(20, 64.0, 1));
    assert_eq!(rt.state.controller_learn_state(), LearnState::Idle);
    let last = rt
        .state
        .svc
        .with_router(|r, _| r.mapping.get_profile().cc_mappings.last().cloned());
    assert_eq!(last.unwrap().cc, 20);
    // Remove a row; reset restores the default profile.
    click_settle(&mut rt, "knob-remove-0");
    let n2 = rt
        .state
        .svc
        .with_router(|r, _| r.mapping.get_profile().cc_mappings.len());
    assert_eq!(n2, n);
    click_settle(&mut rt, "ctl-reset");
    let p = rt
        .state
        .svc
        .with_router(|r, _| r.mapping.get_profile().clone());
    assert_eq!(p, mdaw_model::controller::create_default_profile());
    // Pad channel select.
    click_settle(&mut rt, "pad-channel");
    click_settle(&mut rt, "pad-ch-10");
    let ch = rt
        .state
        .svc
        .with_router(|r, _| r.mapping.get_profile().pad_map.channel);
    assert_eq!(ch, Some(10));
    // Done closes (and cancels a learn).
    click_settle(&mut rt, "learn-pads");
    click_settle(&mut rt, "ctl-done");
    assert!(rt.state.p9.dialog.is_none());
    assert_eq!(rt.state.controller_learn_state(), LearnState::Idle);
}

#[test]
fn a_dialog_owns_the_keyboard() {
    let mut rt = rt();
    rt.state.open_dialog(Dialog::Templates);
    // B would toggle the browser; Space would play.
    press(&mut rt, text_key("b"));
    press(&mut rt, text_key(" "));
    assert!(!rt.state.browser_open);
    assert!(!rt.state.playing());
    press(&mut rt, ev(Key::Escape, Default::default()));
    assert!(rt.state.p9.dialog.is_none());
}
