//! Phase 9 review findings (`docs/rune-port/review-phase-9.md`): one test (or more) per finding,
//! driven through the real view where the finding is about the UI.

use mdaw_model::controller::LearnState;
use rune::Key;

use super::harness::*;
use super::t_p9_samples::wav;
use crate::app::AppMode;
use crate::views::{Dialog, SamplesTab};

fn none() -> rune::Mods {
    Default::default()
}

fn samples_rt() -> rune::Runtime<crate::App> {
    let mut rt = rt();
    rt.state.svc.save_headless = true;
    rt.state.switch_mode(AppMode::Samples);
    rt.invalidate();
    frames(&mut rt, 3);
    rt
}

fn focused_leaf(rt: &rune::Runtime<crate::App>) -> String {
    rt.state
        .focus_seen
        .as_deref()
        .map(|k| k.rsplit('/').next().unwrap_or(k).to_owned())
        .unwrap_or_default()
}

// --- 1 palette arrows ---------------------------------------------------------------------------

#[test]
fn palette_three_downs_select_the_fourth_row_and_enter_runs_it() {
    let mut rt = rt();
    press(&mut rt, ev(Key::Command('k'), cmd()));
    frames(&mut rt, 2);
    rt.paste("switch");
    frames(&mut rt, 2);
    let rows = crate::ui::p9::dialogs::palette_rows(&rt.state.p9.palette, rt.state.mode);
    assert!(
        rows.len() > 4,
        "{:?}",
        rows.iter().map(|c| &c.id).collect::<Vec<_>>()
    );
    for _ in 0..3 {
        press(&mut rt, ev(Key::Down, none()));
        frames(&mut rt, 1);
    }
    // Before: every key reset the selection to 0 (arrows never moved).
    assert_eq!(rt.state.p9.palette.sel, 3);
    // The highlighted row is the fourth one shown.
    let on = format!("cmd-{}", rows[3].id);
    assert!(
        rt.accessibility_json().to_string().contains(&on),
        "{on} on screen"
    );
    let target = rows[3].id.clone();
    press(&mut rt, ev(Key::Enter, none()));
    frames(&mut rt, 2);
    assert!(rt.state.p9.dialog.is_none());
    let mode = rt.state.mode;
    match target.as_str() {
        "switch-daw" | "switch-beat" | "switch-song" | "toggle-view" => {
            assert_eq!(mode, AppMode::Daw)
        }
        "switch-samples" => assert_eq!(mode, AppMode::Samples),
        "switch-dev" => assert_eq!(mode, AppMode::Dev),
        "browse-packs" => assert_eq!(mode, AppMode::Packs),
        "open-settings" => assert_eq!(mode, AppMode::Settings),
        other => panic!("row 3 is {other}: add its effect here"),
    }
    assert_ne!(
        target, rows[0].id,
        "not the first row (before: Enter ran row 0)"
    );
}

#[test]
fn palette_arrows_scroll_the_selected_row_into_view() {
    let mut rt = rt();
    press(&mut rt, ev(Key::Command('k'), cmd()));
    run(&mut rt, 0.3);
    for _ in 0..20 {
        press(&mut rt, ev(Key::Down, none()));
        frames(&mut rt, 1);
    }
    run(&mut rt, 0.5);
    assert_eq!(rt.state.p9.palette.sel, 20);
    let rows = crate::ui::p9::dialogs::palette_rows(&rt.state.p9.palette, rt.state.mode);
    let list = rect(&rt, "palette-list");
    let row = rect(&rt, &format!("cmd-{}", rows[20].id));
    assert!(
        row[1] >= list[1] - 0.5 && row[1] + row[3] <= list[1] + list[3] + 0.5,
        "row {row:?} inside the list {list:?}"
    );
}

#[test]
fn palette_typing_resets_the_selection_but_arrows_do_not() {
    let mut rt = rt();
    press(&mut rt, ev(Key::Command('k'), cmd()));
    frames(&mut rt, 2);
    press(&mut rt, ev(Key::Down, none()));
    press(&mut rt, ev(Key::Down, none()));
    frames(&mut rt, 1);
    assert_eq!(rt.state.p9.palette.sel, 2);
    press(&mut rt, text_key("m"));
    frames(&mut rt, 2);
    assert_eq!(rt.state.p9.palette.sel, 0, "an edit selects the first row");
}

// --- 17 palette ranking ------------------------------------------------------------------------

#[test]
fn command_score_matches_cmdk() {
    use crate::ui::p9::dialogs::command_score;
    // Values printed by cmdk 1.1.1's own `commandScore` (node_modules/cmdk/dist) in node.
    let cases: &[(&str, &str, f64)] = &[
        ("Tempo ", "temp", 0.989901),
        ("Play / Stop ", "ps", 0.88993098710109),
        ("Switch to DAW mode daw beat", "switch", 0.989901),
        ("Export as WAV wav audio render bounce", "wav", 0.891),
        ("New from Template template starter", "temp", 0.891),
        ("Toggle Metronome click metronome tempo", "temp", 0.891),
        ("Grand Piano piano keys acoustic", "temp", 0.0),
        ("Record to Track (Song) ", "rts", 0.712657447128),
        ("a-b_c d", "ABD", 0.80975702429919),
        ("Save As ", "sa", 0.989901),
        ("Clear Pattern ", "cp", 0.8908218089100001),
    ];
    for (v, q, want) in cases {
        let got = command_score(v, q);
        assert!((got - want).abs() < 1e-12, "{v} / {q}: {got} vs {want}");
    }
}

// --- 2 pack reinstall reloads audio regions ----------------------------------------------------

/// A pack folder with a valid `pack.json` and one WAV, in the app's packs folder.
fn write_pack(rt: &rune::Runtime<crate::App>, id: &str, secs: f64) -> String {
    let dir = std::path::PathBuf::from(rt.state.svc.engine.cache().packs_dir().unwrap()).join(id);
    std::fs::create_dir_all(&dir).unwrap();
    let src = wav("hit.wav", secs, 1);
    let bytes = std::fs::read(&src).unwrap();
    let file = dir.join("hit.wav");
    std::fs::write(&file, &bytes).unwrap();
    let sha = "0".repeat(64); // not checked when listing installed packs
    let manifest = serde_json::json!({
        "id": id, "name": "Review Pack", "tagline": "t", "description": "d", "tags": ["x"],
        "author": "a", "source": "https://github.com/example/repo", "license": "CC0-1.0",
        "color": "#f80", "version": 1,
        "files": [{ "path": "hit.wav", "url": "https://raw.githubusercontent.com/o/r/c0ffee0000000000000000000000000000000000/hit.wav", "size": bytes.len(), "sha256": sha }],
        "kits": [{ "id": "main", "name": "Main", "pads": [{ "pad": 0, "file": "hit.wav" }] }]
    });
    let meta = serde_json::json!({ "id": id, "version": 1, "installedAt": "2026-11-19T00:00:00.000Z", "manifest": manifest });
    std::fs::write(dir.join("pack.json"), meta.to_string()).unwrap();
    file.to_string_lossy().into_owned()
}

fn settle(rt: &mut rune::Runtime<crate::App>) {
    for _ in 0..3 {
        let _ = rt
            .state
            .svc
            .packs
            .settle(&mut rt.state.svc.engine, std::time::Duration::from_secs(5));
        frames(rt, 2);
    }
}

#[test]
fn uninstalling_and_reinstalling_a_pack_reloads_the_audio_region_in_place() {
    let mut rt = song_rt();
    let path = write_pack(&rt, "review-pack", 0.5);
    rt.state.svc.packs.refresh_installed();
    settle(&mut rt);
    assert!(
        rt.state
            .svc
            .packs
            .snapshot()
            .store
            .installed
            .iter()
            .any(|p| p.id == "review-pack"),
        "listed as installed"
    );
    // An audio region of the project plays the pack's file.
    rt.state
        .p9
        .samples
        .import_for_song(std::slice::from_ref(&path));
    frames(&mut rt, 3);
    let region = rt
        .state
        .svc
        .engine
        .doc()
        .get_tracks()
        .iter()
        .flat_map(|t| t.regions.iter())
        .find_map(|r| r.audio.clone())
        .expect("audio region");
    let id = region.sample_id.clone();
    let frames_before = rt.state.p9.samples.buffer(&id).unwrap().frames();
    assert!(!rt.state.svc.engine.is_sample_unavailable(&id));
    // Uninstall (the folder goes away): the list changes, the pack's buffers are invalidated
    // and the store re-reads the sample: unreadable → unavailable, audio kept.
    rt.state.svc.packs.uninstall("review-pack");
    settle(&mut rt);
    assert!(
        rt.state.svc.engine.is_sample_unavailable(&id),
        "before: the store was never told (DirectFileLoader), the region stayed 'available'"
    );
    assert_eq!(
        rt.state.p9.samples.buffer(&id).unwrap().frames(),
        frames_before
    );
    run(&mut rt, 0.2);
    assert!(texts(&rt).iter().all(|t| !t.is_empty()), "the view rebuilt");
    // Reinstall a longer take: the sample is reloaded in place (same id), available again,
    // its metadata follows the new audio and the region players get it.
    write_pack(&rt, "review-pack", 1.0);
    rt.state.svc.packs.refresh_installed();
    settle(&mut rt);
    assert!(!rt.state.svc.engine.is_sample_unavailable(&id));
    let b = rt.state.p9.samples.buffer(&id).unwrap();
    assert_eq!(b.frames(), 48_000, "the 1 s take");
    assert_eq!(rt.state.p9.samples.get(&id).unwrap().duration_seconds, 1.0);
    assert!(
        rt.state.p9.samples.rev(&id) >= 1,
        "peaks of the old audio dropped"
    );
}

#[test]
fn a_failed_region_load_is_retried() {
    let mut rt = song_rt();
    let dir = std::env::temp_dir().join(format!("mdaw-retry-{}", uuid::Uuid::new_v4()));
    let path = dir.join("later.wav").to_string_lossy().into_owned();
    let regions = vec![("sample-later".to_owned(), path.clone())];
    rt.state.p9.samples.ensure_region_samples(&regions, 0.0);
    let _ = rt
        .state
        .p9
        .samples
        .poll(&mut rt.state.svc.settings.borrow_mut());
    rt.state.p9.samples.ensure_region_samples(&regions, 0.0);
    assert!(rt.state.p9.samples.buffer("sample-later").is_none());
    // The file appears; before the retry delay nothing happens, after it the load runs again
    // (before: `requested` kept the id forever).
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(wav("x.wav", 0.25, 1), &path).unwrap();
    rt.state.p9.samples.ensure_region_samples(&regions, 1.0);
    let _ = rt
        .state
        .p9
        .samples
        .poll(&mut rt.state.svc.settings.borrow_mut());
    assert!(rt.state.p9.samples.buffer("sample-later").is_none());
    rt.state
        .p9
        .samples
        .ensure_region_samples(&regions, crate::samples::REGION_RETRY + 0.1);
    let _ = rt
        .state
        .p9
        .samples
        .poll(&mut rt.state.svc.settings.borrow_mut());
    assert!(rt.state.p9.samples.buffer("sample-later").is_some());
}

// --- 3 / 9 Packs mount and unmount -------------------------------------------------------------

#[test]
fn opening_packs_from_the_sidebar_asks_for_the_catalog() {
    let mut rt = rt();
    assert!(rt.state.svc.packs.snapshot().store.catalog.is_none());
    click(&mut rt, "mode-packs");
    frames(&mut rt, 2);
    assert_eq!(rt.state.mode, AppMode::Packs);
    settle(&mut rt);
    // Before: nothing asked for it (the tests preloaded it): "Loading…" forever.
    assert!(rt.state.svc.packs.snapshot().store.catalog.is_some());
    assert!(has(&rt, "pack-card-uzu-drumkit"), "remote cards shown");
}

#[test]
fn leaving_packs_stops_the_groove_and_the_sound_preview() {
    let mut rt = rt();
    click(&mut rt, "mode-packs");
    settle(&mut rt);
    click(&mut rt, "pack-card-builtin:tr808");
    click(&mut rt, "kit-preview-tr808");
    frames(&mut rt, 3);
    assert!(rt.state.p9.kit_preview.playing.is_some());
    assert!(rt.state.svc.engine.is_preview_playing());
    rt.state.switch_mode(AppMode::Daw);
    frames(&mut rt, 2);
    // Before: the 808 groove kept playing over the DAW.
    assert!(rt.state.p9.kit_preview.playing.is_none());
    assert!(!rt.state.svc.engine.is_preview_playing());
    // A sound preview too; and a groove still rendering is dropped (its token is stale).
    rt.state.switch_mode(AppMode::Packs);
    frames(&mut rt, 2);
    rt.state.play_pack_sound("builtin:tr808", "kick");
    frames(&mut rt, 3);
    assert!(rt.state.svc.engine.is_preview_playing());
    rt.state.preview_kit("builtin:tr808", "tr808");
    rt.state.switch_mode(AppMode::Daw);
    frames(&mut rt, 3);
    assert!(!rt.state.svc.engine.is_preview_playing());
    assert!(rt.state.p9.kit_preview.playing.is_none());
    assert!(rt.state.p9.kit_preview.take_ready(rt.state.now).is_none());
}

// --- 4 dialog replacement ----------------------------------------------------------------------

#[test]
fn replacing_the_controller_dialog_cancels_its_learn() {
    let mut rt = rt();
    click_settle(&mut rt, "controller-setup");
    click_settle(&mut rt, "learn-pads");
    assert!(matches!(
        rt.state.controller_learn_state(),
        LearnState::Pad { .. }
    ));
    let before = rt
        .state
        .svc
        .with_router(|r, _| r.mapping.get_profile().pad_map.notes[0]);
    press(&mut rt, ev(Key::Command('k'), cmd()));
    frames(&mut rt, 2);
    assert_eq!(rt.state.p9.dialog, Some(Dialog::Palette));
    press(&mut rt, ev(Key::Escape, none()));
    frames(&mut rt, 2);
    assert!(rt.state.p9.dialog.is_none());
    // Before: the learn stayed active and the next note remapped pad 1.
    assert_eq!(rt.state.controller_learn_state(), LearnState::Idle);
    rt.state
        .svc
        .with_router(|r, _| r.mapping.handle_note_on(61, 1.0, 1));
    let after = rt
        .state
        .svc
        .with_router(|r, _| r.mapping.get_profile().pad_map.notes[0]);
    assert_eq!(after, before);
}

#[test]
fn a_running_export_is_not_replaced_by_another_dialog() {
    let mut rt = rt();
    rt.state.open_dialog(Dialog::Export);
    rt.state.p9.export.running = Some(crate::views::ExportKind::Wav);
    rt.state.open_dialog(Dialog::Palette);
    assert_eq!(rt.state.p9.dialog, Some(Dialog::Export));
    rt.state.p9.export.running = None;
    rt.state.open_dialog(Dialog::Palette);
    assert_eq!(rt.state.p9.dialog, Some(Dialog::Palette));
}

// --- 12 dialog names ---------------------------------------------------------------------------

#[test]
fn the_modal_dialogs_are_named_by_their_titles() {
    let mut rt = rt();
    for (d, name) in [
        (Dialog::Palette, "Command Palette"),
        (Dialog::Export, "Export"),
        (Dialog::Templates, "New from template"),
        (Dialog::Controller, "Controller setup"),
    ] {
        rt.state.open_dialog(d);
        rt.invalidate();
        run(&mut rt, 0.3);
        let a11y = rt.accessibility_json();
        let found = find_node(&a11y, &|n| {
            n["role"] == "dialog" && n["name"].as_str() == Some(name)
        });
        assert!(found, "a dialog named {name}: {a11y}");
        rt.state.close_dialog();
        frames(&mut rt, 2);
    }
}

fn find_node(v: &serde_json::Value, f: &dyn Fn(&serde_json::Value) -> bool) -> bool {
    if f(v) {
        return true;
    }
    match v {
        serde_json::Value::Array(a) => a.iter().any(|x| find_node(x, f)),
        serde_json::Value::Object(o) => o.values().any(|x| find_node(x, f)),
        _ => false,
    }
}

// --- 8 top-bar instrument selector -------------------------------------------------------------

#[test]
fn the_instrument_selector_is_focusable_and_its_search_picks_with_the_keys() {
    let mut rt = rt();
    assert!(
        find_node(&rt.accessibility_json(), &|n| {
            n["name"].as_str() == Some("Choose instrument sound")
                && n["actions"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|x| x == "focus"))
        }),
        "the trigger takes focus"
    );
    click_settle(&mut rt, "instrument");
    assert_eq!(focused_leaf(&rt), "sound-search");
    let results = crate::ui::p9::rack::instrument_results(&rt.state);
    assert!(results.len() > 3);
    press(&mut rt, ev(Key::Down, none()));
    press(&mut rt, ev(Key::Down, none()));
    press(&mut rt, ev(Key::Up, none()));
    assert_eq!(rt.state.p9.instrument_active, 1);
    press(&mut rt, ev(Key::Enter, none()));
    run(&mut rt, 0.4);
    // Before: Down / Enter did nothing in the search.
    assert!(rt.state.menu.is_none());
    assert_eq!(rt.state.p9.preset_cache.borrow().2, results[1]);
    assert_eq!(focused_leaf(&rt), "instrument", "focus back on the trigger");
    // Typing selects the first match again.
    click_settle(&mut rt, "instrument");
    press(&mut rt, ev(Key::Down, none()));
    press(&mut rt, text_key("p"));
    assert_eq!(rt.state.p9.instrument_active, 0);
}

// --- 10 / 11 trim and rename drafts ------------------------------------------------------------

#[test]
fn typed_trims_apply_at_once_and_show_formatted_after_blur() {
    let mut rt = samples_rt();
    click(&mut rt, "gen-generate");
    frames(&mut rt, 3);
    click(&mut rt, "sample-tab-edit");
    frames(&mut rt, 2);
    // Finding 11: the auto-selected sample shows its range (before: blank fields).
    assert_eq!(rt.state.p9.trim_start_edit.text, "0.00");
    assert_eq!(rt.state.p9.trim_end_edit.text, "1.00");
    click(&mut rt, "trim-start");
    frames(&mut rt, 1);
    assert_eq!(focused_leaf(&rt), "trim-start");
    rt.state.p9.trim_start_edit.replace_text("");
    rt.invalidate();
    frames(&mut rt, 1);
    rt.paste("0.3");
    frames(&mut rt, 2);
    // Applied while typing (TS `onChange`), the draft kept as typed.
    let e = rt.state.p9.samples.selected_entry().unwrap().clone();
    assert!((e.trim_start() - 0.3).abs() < 1e-9, "{}", e.trim_start());
    assert_eq!(rt.state.p9.trim_start_edit.text, "0.3");
    // The audition plays the new range right away (no stale value).
    click(&mut rt, "sample-audition");
    assert!((rt.state.p9.samples.audition.unwrap().duration - 0.7).abs() < 1e-9);
    click(&mut rt, "sample-audition");
    // Blur: the field shows the applied value formatted.
    click(&mut rt, "sample-waveform");
    frames(&mut rt, 2);
    assert_eq!(rt.state.p9.trim_start_edit.text, "0.30");
}

#[test]
fn the_rename_commits_on_blur_and_before_an_action() {
    let mut rt = samples_rt();
    click(&mut rt, "gen-generate");
    frames(&mut rt, 3);
    click(&mut rt, "sample-tab-edit");
    click(&mut rt, "sample-rename-button");
    frames(&mut rt, 2);
    rt.state
        .p9
        .rename
        .as_mut()
        .unwrap()
        .replace_text("Lead tone");
    rt.invalidate();
    frames(&mut rt, 1);
    // A click elsewhere blurs the field: it commits (before: the draft was dropped).
    click(&mut rt, "sample-waveform");
    frames(&mut rt, 2);
    assert!(rt.state.p9.rename.is_none());
    assert_eq!(
        rt.state.p9.samples.selected_entry().unwrap().sample.name,
        "Lead tone"
    );
    // A pending draft is committed before "Send to arrangement" reads the sample.
    click(&mut rt, "sample-rename-button");
    frames(&mut rt, 1);
    rt.state.p9.rename.as_mut().unwrap().replace_text("Lead 2");
    rt.state.send_to_arrangement();
    let t = rt
        .state
        .svc
        .engine
        .doc()
        .get_tracks()
        .last()
        .unwrap()
        .clone();
    assert_eq!(t.name, "Lead 2");
}

#[test]
fn a_selected_sample_change_resyncs_the_fields_but_keeps_a_draft() {
    let mut rt = samples_rt();
    click(&mut rt, "gen-generate");
    frames(&mut rt, 3);
    click(&mut rt, "sample-tab-edit");
    frames(&mut rt, 2);
    // A handle drag (not the field) updates both fields.
    rt.state.set_trim(true, 0.6);
    frames(&mut rt, 2);
    assert_eq!(rt.state.p9.trim_end_edit.text, "0.60");
    // While End is being typed in, a trim change elsewhere leaves the draft alone.
    click(&mut rt, "trim-end");
    frames(&mut rt, 1);
    rt.state.p9.trim_end_edit.replace_text("");
    rt.invalidate();
    frames(&mut rt, 1);
    rt.paste("0.4");
    frames(&mut rt, 2);
    rt.state.set_trim(false, 0.1);
    frames(&mut rt, 2);
    assert_eq!(rt.state.p9.trim_end_edit.text, "0.4");
    assert_eq!(rt.state.p9.trim_start_edit.text, "0.10");
}

// --- 13 slider geometry ------------------------------------------------------------------------

#[test]
fn sliders_put_the_thumb_inside_the_track_and_fill_the_range() {
    let mut rt = samples_rt();
    for (amp, label) in [(0.0, "low"), (0.5, "mid"), (1.0, "high")] {
        rt.state.p9.signal.amplitude = amp;
        rt.invalidate();
        run(&mut rt, 1.5); // layout springs settle
        let key = "gen-level";
        let r = rect(&rt, key);
        let th = rect(&rt, &format!("{key}-thumb"));
        let range = rect(&rt, &format!("{key}-range"));
        let f = amp as f32;
        let want = crate::ui::song::headers::vol_thumb_center(f, r[2]);
        let centre = th[0] + th[2] / 2.0 - r[0];
        assert!(
            (centre - want).abs() < 0.6,
            "{label}: thumb {centre} vs {want}"
        );
        // The orange range runs from the left edge up to the thumb.
        assert!((range[0] - r[0]).abs() < 0.6, "{label}");
        assert!(
            (range[2] - f * (r[2] - 12.0)).abs() < 0.6,
            "{label}: range {}",
            range[2]
        );
        // Pressing the thumb keeps the value (one geometry for paint and input).
        let (x, y) = (th[0] + th[2] / 2.0, th[1] + th[3] / 2.0);
        click_at(&mut rt, x, y);
        assert!(
            (rt.state.p9.signal.amplitude - amp).abs() < 0.011,
            "{label}: {}",
            rt.state.p9.signal.amplitude
        );
    }
}

// --- 15 settings writes ------------------------------------------------------------------------

#[test]
fn a_slider_drag_writes_the_settings_file_once() {
    let dir = std::env::temp_dir().join(format!("mdaw-st-{}", uuid::Uuid::new_v4()));
    let st = mdaw_model::settings::SettingsStore::open(&dir);
    let mut app = crate::App::with_settings(crate::services::RunMode::Headless, st);
    crate::scenes::started(&mut app);
    let mut rt = runtime(app);
    rt.state.svc.save_headless = true;
    rt.state.switch_mode(AppMode::Settings);
    run(&mut rt, 0.3);
    let file = dir.join(mdaw_model::settings::SettingsStore::FILE_NAME);
    let key = "default-master-volume".to_owned();
    let r = rect(&rt, &key);
    let (x, y) = (r[0] + 8.0, r[1] + r[3] / 2.0);
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    frames(&mut rt, 1);
    let before = std::fs::read_to_string(&file).unwrap_or_default();
    for i in 1..20 {
        rt.pointer_move(x + i as f32 * 4.0, y);
        frames(&mut rt, 1);
    }
    // Held while dragging (before: one file write per move).
    assert!(rt.state.svc.settings.borrow().is_deferred());
    assert_eq!(std::fs::read_to_string(&file).unwrap_or_default(), before);
    rt.pointer_up(x + 80.0, y);
    frames(&mut rt, 2);
    assert!(!rt.state.svc.settings.borrow().is_deferred());
    let after = std::fs::read_to_string(&file).unwrap();
    assert_ne!(after, before, "written when the drag ended");
    assert!(after.contains(mdaw_model::settings::SETTINGS_STORAGE_KEY));
}

// --- 16 audition across tabs -------------------------------------------------------------------

#[test]
fn switching_to_generate_keeps_the_audition_playing() {
    let mut rt = samples_rt();
    rt.state.p9.signal.duration = 2.0;
    click(&mut rt, "gen-generate");
    frames(&mut rt, 3);
    click(&mut rt, "sample-tab-edit");
    click(&mut rt, "sample-audition");
    assert!(rt.state.svc.engine.is_audition_playing());
    click(&mut rt, "sample-tab-generate");
    run(&mut rt, 0.3);
    assert!(crate::signal_preview::mounted());
    // Before: the signal synth replaced the preview slot and cut the audition while the button
    // still said Stop. Now both run (as in the TS: separate nodes).
    assert!(rt.state.svc.engine.is_audition_playing());
    assert!(rt.state.p9.samples.audition.is_some());
    click(&mut rt, "sample-tab-edit");
    frames(&mut rt, 2);
    assert!(texts(&rt).iter().any(|t| t == "Stop"));
}

// --- 7 waveform work off the UI thread ---------------------------------------------------------

#[test]
fn the_audition_moves_its_cursor_without_rebuilding_the_view() {
    let mut rt = samples_rt();
    click(&mut rt, "gen-generate");
    frames(&mut rt, 3);
    click(&mut rt, "sample-tab-edit");
    frames(&mut rt, 2);
    click(&mut rt, "sample-audition");
    frames(&mut rt, 2);
    let x0 = rt.state.p9.audition_x.get()[0];
    let now = rt.state.now + 0.25;
    // Mid-audition, a frame's phase-9 work asks for no rebuild (before: every frame).
    assert!(!rt.state.p9_poll(now));
    let x1 = rt.state.p9.audition_x.get()[0];
    assert!(x1 > x0, "the cursor moved: {x0} → {x1}");
    let w = rt.state.p9.waveform_w.get();
    let p = rt.state.p9.samples.audition_progress(now) as f32;
    assert!(p > 0.2 && (x1 - p * w).abs() < 1.0, "{x1} vs {}", p * w);
    let _ = SamplesTab::Info;
}

/// Before/after timings of the per-rebuild waveform work (`cargo test --release -p mdaw-app
/// waveform_timings -- --nocapture --ignored`): the Edit tab scanned the whole buffer per rebuild
/// and the audition copied its range; now a rebuild reads cached tiles and the audition starts at
/// an offset.
#[test]
#[ignore = "timings, run by hand"]
fn waveform_timings() {
    use std::time::Instant;
    let mut rt = samples_rt();
    let path = wav("long.wav", 60.0, 2);
    rt.state.p9.samples.import_files(&[path]);
    frames(&mut rt, 3);
    rt.state.p9.tab = Some(SamplesTab::Info);
    rt.invalidate();
    frames(&mut rt, 3);
    let id = rt.state.p9.samples.selected.clone().unwrap();
    let b = rt.state.p9.samples.buffer(&id).unwrap();
    let w = rt.state.p9.waveform_w.get().ceil() as usize;
    let t = Instant::now();
    for _ in 0..10 {
        std::hint::black_box(crate::samples::column_peaks(&b, w));
    }
    let scan = t.elapsed().as_secs_f64() * 100.0;
    let t = Instant::now();
    for _ in 0..10 {
        let (s, e) = (0usize, b.frames());
        let c: Vec<Vec<f32>> = b
            .channels
            .iter()
            .map(|c| c[s..e].iter().map(|x| x * 2.0).collect())
            .collect();
        std::hint::black_box(c);
    }
    let copy = t.elapsed().as_secs_f64() * 100.0;
    let t = Instant::now();
    for _ in 0..10 {
        rt.invalidate();
        rt.frame(0.0);
    }
    let frame = t.elapsed().as_secs_f64() * 100.0;
    let t = Instant::now();
    rt.state.toggle_audition();
    let start = t.elapsed().as_secs_f64() * 1000.0;
    println!(
        "60 s stereo, {w} columns: old per-rebuild scan {scan:.2} ms, old audition copy {copy:.2} ms; \
         new Edit-tab rebuild frame {frame:.2} ms, new audition start {start:.3} ms"
    );
}

/// Finding 17: "temp" typed one key at a time shows what Electron's cmdk shows (the
/// `palette-search` screenshot): groups in their own order, items by score inside each.
#[test]
fn palette_temp_rows_match_cmdk() {
    let mut rt = rt();
    rt.state.open_dialog(Dialog::Palette);
    for q in ["t", "te", "tem", "temp"] {
        rt.state.p9.palette.query = rune::TextEdit::new(q);
        let _ = crate::ui::p9::dialogs::palette_sync(&mut rt.state);
    }
    let rows = crate::ui::p9::dialogs::palette_rows(&rt.state.p9.palette, rt.state.mode);
    let ids: Vec<&str> = rows.iter().take(6).map(|c| c.id.as_str()).collect();
    // Electron (docs/parity/phase-9/palette-search-electron.png): Project: New from Template;
    // Navigation: Switch to Samples, Switch to Developer, Browse Packs; View: Song View, Toggle
    // Sample Browser. Before: a TRANSPORT group second and the Navigation rows missing.
    assert_eq!(
        ids,
        [
            "new-from-template",
            "switch-samples",
            "switch-dev",
            "browse-packs",
            "switch-song",
            "toggle-browser"
        ]
    );
}
