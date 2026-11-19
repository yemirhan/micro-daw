//! `inventory/views.md` §5 — TrackHeader (+ TrackToggle, PanDial, the automation menu, the
//! instrument popover) and GroupHeader.

use rune::Key;

use super::harness::*;
use super::t_song_arr::texts_of;
use crate::song::TRACK_H;
use mdaw_model::{AutomationParameter, InstrumentType};

#[test]
fn header_size_strip_and_drum_chip() {
    let mut rt = song_rt();
    let d = tid(&rt, "Drums");
    assert_eq!(rect(&rt, &format!("track-{d}"))[3], TRACK_H);
    let chip = format!("beat-chip-{d}");
    let e = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some(&chip))
        .unwrap();
    assert_eq!(
        e.help.as_deref(),
        Some("Active drum track: the Beat view edits this one")
    );
    // A second drum track: its chip / name make it active.
    click(&mut rt, "add-drums");
    let d2 = rt.state.tracks().last().unwrap().id.clone();
    run(&mut rt, 0.3);
    let e = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some(&format!("beat-chip-{d2}")));
    let _ = e;
    rt.state.set_active_drum_track(&d);
    rt.invalidate();
    frames(&mut rt, 2);
    let e = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some(&format!("beat-chip-{d2}")))
        .unwrap();
    assert_eq!(
        e.help.as_deref(),
        Some("Edit this drum track in the Beat view")
    );
    click(&mut rt, &format!("beat-chip-{d2}"));
    assert_eq!(
        rt.state.svc.engine.doc().get_active_drum_track_id(),
        Some(d2.clone())
    );
    click(&mut rt, &format!("track-name-{d}"));
    assert_eq!(
        rt.state.svc.engine.doc().get_active_drum_track_id(),
        Some(d)
    );
}

#[test]
fn fx_button_toggles_one_drawer_at_a_time() {
    let mut rt = song_rt();
    let b = tid(&rt, "Bass");
    let l = tid(&rt, "Lead");
    click(&mut rt, &format!("fx-{b}"));
    assert_eq!(rt.state.song.fx_track, Some(b.clone()));
    assert!(has(&rt, "track-fx"));
    assert_eq!(rect(&rt, "track-fx")[2], 336.0);
    click(&mut rt, &format!("fx-{l}"));
    assert_eq!(rt.state.song.fx_track, Some(l.clone()));
    click(&mut rt, &format!("fx-{l}"));
    assert_eq!(rt.state.song.fx_track, None);
}

#[test]
fn automation_menu_adds_removes_and_clears_lanes() {
    let mut rt = song_rt();
    let b = tid(&rt, "Bass");
    click_settle(&mut rt, &format!("automation-{b}"));
    assert!(has(&rt, "automation-menu"));
    assert!(texts(&rt).contains(&"Automate".to_owned()));
    for p in [
        "Volume",
        "Pan",
        "Reverb",
        "Delay",
        "Chorus",
        "Distortion",
        "Filter",
        "EQ Low",
        "EQ Mid",
        "EQ High",
    ] {
        assert!(texts(&rt).contains(&p.to_owned()), "{p}");
    }
    assert!(!has(&rt, "automate-clear"), "Clear all only with lanes");
    click(&mut rt, "automate-volume");
    click(&mut rt, "automate-pan");
    let lanes = rt.state.find_track(&b).unwrap().automation.clone().unwrap();
    assert_eq!(lanes.len(), 2);
    assert!(lanes.iter().all(|l| l.visible));
    assert_eq!(undo_label(&rt).as_deref(), Some("Add Automation Lane"));
    // The track row grows by 48 per visible lane.
    run(&mut rt, 0.5);
    assert!(has(&rt, &format!("auto-{b}-volume")));
    // Clicking an existing lane removes it (never just hides it).
    click(&mut rt, "automate-pan");
    let lanes = rt.state.find_track(&b).unwrap().automation.clone().unwrap();
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].parameter, AutomationParameter::Volume);
    assert_eq!(undo_label(&rt).as_deref(), Some("Remove Automation Lane"));
    run(&mut rt, 0.5);
    click(&mut rt, "automate-clear");
    assert!(
        rt.state
            .find_track(&b)
            .unwrap()
            .automation
            .clone()
            .unwrap_or_default()
            .is_empty()
    );
    assert!(!has(&rt, "automation-menu"), "Clear all closes the menu");
}

#[test]
fn delete_button_removes_the_track() {
    let mut rt = song_rt();
    let l = tid(&rt, "Lead");
    click(&mut rt, &format!("delete-{l}"));
    assert!(rt.state.find_track(&l).is_none());
    assert_eq!(undo_label(&rt).as_deref(), Some("Remove Track"));
}

#[test]
fn instrument_popover_search_vibes_and_choices() {
    let mut rt = song_rt();
    let b = tid(&rt, "Bass");
    click_settle(&mut rt, &format!("instrument-{b}"));
    assert!(has(&rt, "instrument-menu"));
    assert!(has(&rt, "instrument-drums"));
    assert!(has(&rt, "vibe-all"));
    // Search.
    rt.state.song.instrument_search = rune::TextEdit::new("zzzzqqq");
    rt.invalidate();
    frames(&mut rt, 2);
    assert!(has(&rt, "instrument-empty"));
    assert!(texts(&rt).contains(&"No sounds found.".to_owned()));
    rt.state.song.instrument_search = rune::TextEdit::new("drum kit");
    rt.invalidate();
    frames(&mut rt, 2);
    assert!(has(&rt, "instrument-drums"));
    rt.state.song.instrument_search = rune::TextEdit::new("");
    rt.invalidate();
    frames(&mut rt, 2);
    // A vibe chip filters the groups (not Drums).
    click(&mut rt, "vibe-Chill");
    assert_eq!(
        rt.state.song.vibe,
        Some(mdaw_engine::synth::presets::PresetVibe::Chill)
    );
    let presets = mdaw_engine::synth::bank::synth_presets();
    let chill = presets
        .iter()
        .position(|p| p.meta().vibe == mdaw_engine::synth::presets::PresetVibe::Chill)
        .unwrap();
    let other = presets
        .iter()
        .position(|p| p.meta().vibe != mdaw_engine::synth::presets::PresetVibe::Chill)
        .unwrap();
    assert!(has(&rt, &format!("instrument-preset-{chill}")));
    assert!(!has(&rt, &format!("instrument-preset-{other}")));
    click(&mut rt, "vibe-Chill");
    assert_eq!(rt.state.song.vibe, None, "the active chip toggles off");
    // Picking a preset sets {synth, index} and closes.
    run(&mut rt, 0.3);
    click(&mut rt, &format!("instrument-preset-{chill}"));
    let t = rt.state.find_track(&b).unwrap().clone();
    assert_eq!(
        (t.instrument.kind, t.instrument.preset_index),
        (InstrumentType::Synth, chill as i64)
    );
    assert_eq!(undo_label(&rt).as_deref(), Some("Change Instrument"));
    assert!(!has(&rt, "instrument-menu"));
    click_settle(&mut rt, &format!("instrument-{b}"));
    click(&mut rt, "instrument-drums");
    let t = rt.state.find_track(&b).unwrap().clone();
    assert_eq!(
        (t.instrument.kind, t.instrument.preset_index),
        (InstrumentType::Drums, 0)
    );
}

#[test]
fn audio_tracks_show_audio_and_no_arm() {
    let mut rt = song_rt();
    let audio = mdaw_model::SampleRef {
        id: "s1".into(),
        name: "Break".into(),
        path: "/b.wav".into(),
        duration_seconds: 2.0,
        sample_rate: 44100.0,
        channels: 2,
    };
    rt.state
        .svc
        .engine
        .edit(|d| d.import_audio_sample(&audio))
        .unwrap();
    let a = rt.state.tracks().last().unwrap().id.clone();
    rt.invalidate();
    frames(&mut rt, 3);
    // An audio track arms like the others (its input is heard and recorded) and shows its
    // input where the others show their instrument.
    assert!(has(&rt, &format!("arm-{a}")));
    assert!(!has(&rt, &format!("instrument-{a}")));
    assert!(has(&rt, &format!("input-{a}")));
    assert!(texts(&rt).contains(&"In 1".to_owned()));
}

#[test]
fn arm_mute_solo_toggles() {
    let mut rt = song_rt();
    let b = tid(&rt, "Bass");
    let l = tid(&rt, "Lead");
    let help = |rt: &rune::Runtime<crate::app::App>, k: &str| {
        rt.elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some(k))
            .unwrap()
            .help
    };
    assert_eq!(
        help(&rt, &format!("arm-{b}")).as_deref(),
        Some("Arm: play this track from the keys and record it")
    );
    click(&mut rt, &format!("arm-{b}"));
    assert_eq!(
        rt.state.svc.engine.doc().get_armed_track_id(),
        Some(b.as_str())
    );
    assert_eq!(
        help(&rt, &format!("arm-{b}")).as_deref(),
        Some("Disarm track")
    );
    // Each track arms on its own: arming another one keeps this one armed.
    click(&mut rt, &format!("arm-{l}"));
    assert_eq!(
        rt.state.svc.engine.doc().get_armed_track_id(),
        Some(l.as_str())
    );
    assert_eq!(
        rt.state.svc.engine.doc().armed_track_ids(),
        vec![b.as_str(), l.as_str()]
    );
    click(&mut rt, &format!("arm-{l}"));
    assert_eq!(
        rt.state.svc.engine.doc().get_armed_track_id(),
        Some(b.as_str())
    );
    click(&mut rt, &format!("arm-{b}"));
    assert_eq!(rt.state.svc.engine.doc().get_armed_track_id(), None);
    click(&mut rt, &format!("mute-{b}"));
    assert!(rt.state.find_track(&b).unwrap().muted);
    assert_eq!(
        help(&rt, &format!("mute-{b}")).as_deref(),
        Some("Unmute track")
    );
    click(&mut rt, &format!("solo-{b}"));
    assert!(rt.state.find_track(&b).unwrap().solo);
    assert_eq!(
        help(&rt, &format!("solo-{b}")).as_deref(),
        Some("Unsolo track")
    );
    // A toggle click doesn't reach anything under it (no selection change).
    assert!(rt.state.song.selection.is_empty());
}

#[test]
fn volume_slider_and_readout() {
    let mut rt = song_rt();
    let b = tid(&rt, "Bass");
    assert_eq!(texts_of(&rt, &format!("vol-readout-{b}")), "-6");
    let r = rect(&rt, &format!("vol-{b}"));
    // Drag to the far left → −40; far right → 0 (steps of 1).
    drag(&mut rt, r[0] + r[2] / 2.0, r[1] + 7.0, -400.0, 0.0);
    assert_eq!(rt.state.find_track(&b).unwrap().volume, -40.0);
    drag(&mut rt, r[0] + 10.0, r[1] + 7.0, 600.0, 0.0);
    assert_eq!(rt.state.find_track(&b).unwrap().volume, 0.0);
    assert_eq!(texts_of(&rt, &format!("vol-readout-{b}")), "0");
    // The readout's + sign above 0 (`track.volume > 0`).
    rt.state.set_track_volume(&b, 3.0);
    rt.invalidate();
    frames(&mut rt, 2);
    assert_eq!(texts_of(&rt, &format!("vol-readout-{b}")), "+3");
}

#[test]
fn pan_dial_drag_double_click_keys_and_label() {
    let mut rt = song_rt();
    let b = tid(&rt, "Bass");
    let k = format!("pan-{b}");
    let (x, y) = at(&rt, &k);
    // 80 points per unit, up = right.
    drag(&mut rt, x, y, 0.0, -40.0);
    assert!((rt.state.find_track(&b).unwrap().pan - 0.5).abs() < 1e-9);
    let e = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some(&k))
        .unwrap();
    assert_eq!(e.help.as_deref(), Some("Pan R50"));
    drag(&mut rt, x, y, 0.0, 400.0);
    assert_eq!(rt.state.find_track(&b).unwrap().pan, -1.0, "clamped");
    run(&mut rt, 0.8);
    double_click(&mut rt, &k);
    assert_eq!(rt.state.find_track(&b).unwrap().pan, 0.0);
    // Arrow keys ±0.05 on the focused dial.
    // (the double click focused it)
    press(&mut rt, ev(Key::Up, rune::Mods::default()));
    assert!((rt.state.find_track(&b).unwrap().pan - 0.05).abs() < 1e-9);
    press(&mut rt, ev(Key::Left, rune::Mods::default()));
    press(&mut rt, ev(Key::Left, rune::Mods::default()));
    assert!((rt.state.find_track(&b).unwrap().pan + 0.05).abs() < 1e-9);
    assert_eq!(crate::song::pan_label(-0.05), "L5");
    assert_eq!(crate::song::pan_label(0.004), "C");
}

#[test]
fn group_header_collapse_rename_mute_solo_ungroup() {
    let mut rt = song_rt();
    let d = tid(&rt, "Drums");
    let b = tid(&rt, "Bass");
    let g = rt
        .state
        .svc
        .engine
        .edit(|doc| doc.create_group("Rhythm", &[d.clone(), b.clone()]));
    rt.invalidate();
    run(&mut rt, 0.3);
    assert_eq!(rect(&rt, &format!("group-{}", g.id))[3], 28.0);
    assert!(texts(&rt).contains(&"2".to_owned()), "the track count");
    // Collapse hides the group's tracks.
    click(&mut rt, &format!("group-collapse-{}", g.id));
    assert!(rt.state.svc.engine.doc().get_groups()[0].collapsed);
    run(&mut rt, 0.3);
    assert!(!has(&rt, &format!("track-{d}")));
    click(&mut rt, &format!("group-collapse-{}", g.id));
    run(&mut rt, 0.3);
    assert!(has(&rt, &format!("track-{d}")));
    // Mute: all muted → unmute all, else mute all; solo: any solo → unsolo.
    click(&mut rt, &format!("group-mute-{}", g.id));
    assert!(rt.state.find_track(&d).unwrap().muted && rt.state.find_track(&b).unwrap().muted);
    click(&mut rt, &format!("group-mute-{}", g.id));
    assert!(!rt.state.find_track(&d).unwrap().muted);
    rt.state.toggle_track_solo(&d);
    rt.invalidate();
    frames(&mut rt, 2);
    click(&mut rt, &format!("group-solo-{}", g.id));
    assert!(!rt.state.find_track(&d).unwrap().solo && !rt.state.find_track(&b).unwrap().solo);
    // Double-click the name: Enter commits (trimmed), Escape cancels, empty ignored.
    run(&mut rt, 0.8);
    double_click(&mut rt, &format!("group-name-{}", g.id));
    assert!(has(&rt, "group-rename"));
    // The name is selected: typing replaces it.
    rt.paste(" Beat ");
    frames(&mut rt, 2);
    press(&mut rt, ev(Key::Enter, rune::Mods::default()));
    assert_eq!(rt.state.svc.engine.doc().get_groups()[0].name, "Beat");
    assert_eq!(undo_label(&rt).as_deref(), Some("Rename Group"));
    run(&mut rt, 0.8);
    double_click(&mut rt, &format!("group-name-{}", g.id));
    rt.paste("Nope");
    frames(&mut rt, 2);
    press(&mut rt, ev(Key::Escape, rune::Mods::default()));
    assert_eq!(rt.state.svc.engine.doc().get_groups()[0].name, "Beat");
    assert!(rt.state.song.group_rename.is_none());
    // Ungroup.
    click(&mut rt, &format!("group-ungroup-{}", g.id));
    assert!(rt.state.svc.engine.doc().get_groups().is_empty());
    assert_eq!(undo_label(&rt).as_deref(), Some("Remove Group"));
}

#[test]
fn cmd_g_groups_every_track() {
    let mut rt = song_rt();
    press(&mut rt, ev(Key::Command('g'), cmd()));
    let g = rt.state.svc.engine.doc().get_groups().to_vec();
    assert_eq!(g.len(), 1);
    assert_eq!((g[0].name.as_str(), g[0].track_ids.len()), ("Group", 4));
}
