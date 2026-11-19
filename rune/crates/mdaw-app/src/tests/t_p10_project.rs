//! Phase 10: the project service and the window lifecycle (`ProjectManager.ts`, `useProject.ts`,
//! `main.ts`'s close guard), driven through the view, keys and file dialogs like a user would.

use std::path::{Path, PathBuf};

use mdaw_model::settings as store;
use rune::{FileDialogKind, Key, Mods, Shell};

use super::harness::*;
use crate::app::{App, AppMode};
use crate::project::ProjectOp;

fn tmp_dir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("mdaw-p10-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn p(dir: &Path, name: &str) -> String {
    dir.join(name).to_string_lossy().into_owned()
}

/// Makes the project dirty the way a user would (a BPM change through the action).
fn edit(r: &mut rune::Runtime<App>) {
    let bpm = r.state.svc.engine.doc().get_bpm();
    r.update(|s, _| s.set_bpm(bpm + 1.0));
    frames(r, 2);
    assert!(r.state.dirty());
}

fn answer(r: &mut rune::Runtime<App>, kind: FileDialogKind, path: Option<&str>) {
    let (k, _) = r.file_dialog().expect("a file dialog is open");
    assert_eq!(k, kind);
    r.resolve_file_dialog(path.map(PathBuf::from)).unwrap();
    frames(r, 2);
}

fn close(r: &mut rune::Runtime<App>) {
    r.shell_request = None;
    r.update(|s, tx| s.on_window_close(tx));
    frames(r, 2);
}

fn closed(r: &rune::Runtime<App>) -> bool {
    // Off macOS the guard's pass quits (`window-all-closed`); on macOS the window just goes.
    r.state.project.window_closes == 1
        && (cfg!(target_os = "macos") || r.shell_request == Some(Shell::Quit))
}

fn saved_bpm(path: &str) -> f64 {
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    v["arrangement"]["bpm"].as_f64().unwrap()
}

#[test]
fn title_follows_the_name_and_the_dirty_marker() {
    let mut r = rt();
    assert_eq!(r.state.project.title, "Untitled - Micro DAW");
    edit(&mut r);
    assert_eq!(r.state.project.title, "Untitled * - Micro DAW");
    assert!(has(&r, "dirty-dot"));
    // The top bar shows the name; clicking it saves (Save As without a path).
    click(&mut r, "project");
    let dir = tmp_dir();
    let f = p(&dir, "Beat One.mdaw");
    let (_, o) = r.file_dialog().unwrap();
    assert_eq!(o.title, "Save Project");
    assert_eq!(o.file_name.as_deref(), Some("Untitled.mdaw"));
    assert_eq!(
        o.filters,
        vec![("Micro DAW Project".to_owned(), vec!["mdaw".to_owned()])]
    );
    answer(&mut r, FileDialogKind::Save, Some(&f));
    assert_eq!(r.state.project.title, "Beat One - Micro DAW");
    assert!(!has(&r, "dirty-dot"));
    assert!(r.state.project_name() == "Beat One");
    // Windows paths name the project too (Electron bug fixed in both apps: split on '/' only).
    assert_eq!(
        mdaw_model::migrate::project_name(Some(r"C:\Songs\Demo.mdaw")),
        "Demo"
    );
}

#[test]
fn a_clean_project_closes_without_asking() {
    let mut r = rt();
    close(&mut r);
    assert!(r.state.project.unsaved.is_none());
    assert!(closed(&r));
}

#[test]
fn dirty_close_asks_and_cancel_keeps_the_window() {
    let mut r = rt();
    edit(&mut r);
    close(&mut r);
    assert_eq!(r.state.project.unsaved, Some(ProjectOp::Close));
    assert!(has(&r, "unsaved"));
    let msg = r.state.project_name();
    assert_eq!(msg, "Untitled");
    assert!(r.shell_request.is_none(), "the close is held");
    click(&mut r, "unsaved-cancel");
    assert!(r.state.project.unsaved.is_none());
    assert!(r.shell_request.is_none());
    assert!(r.state.dirty());
    // The guard is armed again: the next close asks again (Escape = Cancel).
    close(&mut r);
    assert!(has(&r, "unsaved"));
    press(&mut r, ev(Key::Escape, Mods::default()));
    assert!(!has(&r, "unsaved"));
    assert!(r.shell_request.is_none());
}

#[test]
fn dirty_close_dont_save_closes_without_writing() {
    let mut r = rt();
    edit(&mut r);
    close(&mut r);
    click(&mut r, "unsaved-dont-save");
    // `project:force-close`: the window is asked to close and the next close passes.
    assert_eq!(r.shell_request, Some(Shell::Close));
    assert!(r.state.project.force_close);
    r.shell_request = None;
    r.update(|s, tx| s.on_window_close(tx));
    assert!(closed(&r));
    assert_eq!(r.state.project.writes, 0);
}

#[test]
fn dirty_close_save_writes_then_closes_and_cancelling_the_dialog_keeps_it() {
    let dir = tmp_dir();
    let mut r = rt();
    edit(&mut r);
    close(&mut r);
    // Save without a path → the save dialog; cancelling it cancels the close.
    click(&mut r, "unsaved-save");
    answer(&mut r, FileDialogKind::Save, None);
    assert!(r.shell_request.is_none());
    assert!(!r.state.project.force_close);
    assert!(r.state.dirty());
    // Again, Enter (the default button) = Save, and a path this time.
    close(&mut r);
    press(&mut r, ev(Key::Enter, Mods::default()));
    let f = p(&dir, "closing.mdaw");
    answer(&mut r, FileDialogKind::Save, Some(&f));
    assert_eq!(r.shell_request, Some(Shell::Close));
    assert_eq!(saved_bpm(&f), 121.0);
    assert!(!r.state.dirty());
    // With a path, Save writes at once.
    r.shell_request = None;
    r.update(|s, _| s.project.force_close = false);
    edit(&mut r);
    close(&mut r);
    click(&mut r, "unsaved-save");
    assert!(r.file_dialog().is_none());
    assert_eq!(r.shell_request, Some(Shell::Close));
    assert_eq!(saved_bpm(&f), 122.0);
}

#[test]
fn save_and_save_as_keys_write_the_project_file() {
    let dir = tmp_dir();
    let mut r = rt();
    edit(&mut r);
    press(&mut r, ev(Key::Command('s'), cmd()));
    let a = p(&dir, "a.mdaw");
    answer(&mut r, FileDialogKind::Save, Some(&a));
    let raw = std::fs::read_to_string(&a).unwrap();
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(v["version"], "1.0.0");
    assert!(v["audioConfig"]["effectParams"]["reverbWet"].is_number());
    assert_eq!(v["audioConfig"]["masterVolume"], -12.0);
    let created = v["createdAt"].as_str().unwrap().to_owned();
    assert!(created.ends_with('Z') && created.len() == 24, "{created}");
    // ⌘S again: same file, no dialog, createdAt kept.
    edit(&mut r);
    press(&mut r, ev(Key::Command('s'), cmd()));
    assert!(r.file_dialog().is_none());
    let v2: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&a).unwrap()).unwrap();
    assert_eq!(v2["createdAt"].as_str(), Some(created.as_str()));
    // ⇧⌘S: the dialog with "<name>.mdaw", a new file and a new createdAt.
    press(
        &mut r,
        ev(
            Key::Command('s'),
            Mods {
                shift: true,
                ..cmd()
            },
        ),
    );
    let (_, o) = r.file_dialog().unwrap();
    assert_eq!(o.file_name.as_deref(), Some("a.mdaw"));
    let b = p(&dir, "b.mdaw");
    answer(&mut r, FileDialogKind::Save, Some(&b));
    assert_eq!(r.state.project.file_path.as_deref(), Some(b.as_str()));
    assert_eq!(r.state.project.title, "b - Micro DAW");
    // The palette's Save / Save As run the same.
    edit(&mut r);
    r.update(|s, tx| s.run_command("save", tx));
    frames(&mut r, 2);
    assert!(!r.state.dirty());
    r.update(|s, tx| s.run_command("save-as", tx));
    frames(&mut r, 2);
    answer(&mut r, FileDialogKind::Save, None);
    assert_eq!(r.state.project.file_path.as_deref(), Some(b.as_str()));
}

#[test]
fn new_project_asks_saves_resets_and_writes() {
    let dir = tmp_dir();
    let mut r = rt();
    r.state.svc.engine.set_master_volume_db(-20.0);
    edit(&mut r);
    // ⌘N while dirty: the prompt; Don't Save → the save dialog ("Untitled.mdaw").
    press(&mut r, ev(Key::Command('n'), cmd()));
    assert_eq!(r.state.project.unsaved, Some(ProjectOp::New));
    click(&mut r, "unsaved-dont-save");
    let (_, o) = r.file_dialog().unwrap();
    assert_eq!(o.file_name.as_deref(), Some("Untitled.mdaw"));
    let f = p(&dir, "fresh.mdaw");
    answer(&mut r, FileDialogKind::Save, Some(&f));
    let doc = r.state.svc.engine.doc();
    assert_eq!(doc.get_bpm(), 120.0);
    assert_eq!(doc.get_tracks().len(), 1, "the starter: one Drums track");
    assert!(!doc.undo_manager().can_undo());
    assert!(!r.state.dirty());
    assert_eq!(r.state.svc.engine.get_master_volume_db(), -12.0);
    assert_eq!(
        store::read_master_volume(&r.state.svc.settings.borrow()),
        -12.0,
        "persisted like setMasterVolumeDb"
    );
    let fx = r.state.svc.engine.live_effect_params();
    assert_eq!(
        (
            fx.reverb_wet,
            fx.chorus_depth,
            fx.filter_cutoff,
            fx.filter_resonance
        ),
        (0.0, 0.0, 18000.0, 1.0)
    );
    assert_eq!(saved_bpm(&f), 120.0);
    assert_eq!(r.state.project.title, "fresh - Micro DAW");
    // The sidebar's New: clean, so straight to the dialog; cancelled = nothing changes.
    r.state.sidebar_open = true;
    r.invalidate();
    frames(&mut r, 2);
    click(&mut r, "project-new");
    assert!(r.state.project.unsaved.is_none());
    answer(&mut r, FileDialogKind::Save, None);
    assert_eq!(r.state.project.file_path.as_deref(), Some(f.as_str()));
}

#[test]
fn new_from_template_asks_and_writes() {
    let dir = tmp_dir();
    let mut r = rt();
    edit(&mut r);
    r.update(|s, tx| s.new_from_template(1, tx));
    frames(&mut r, 2);
    assert_eq!(r.state.project.unsaved, Some(ProjectOp::Template(1)));
    click(&mut r, "unsaved-dont-save");
    let f = p(&dir, "rock.mdaw");
    answer(&mut r, FileDialogKind::Save, Some(&f));
    let n = r.state.svc.engine.doc().get_tracks().len();
    assert!(n > 1, "the rock-song template's tracks");
    assert!(!r.state.dirty());
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
    assert_eq!(v["arrangement"]["tracks"].as_array().unwrap().len(), n);
}

#[test]
fn open_loads_legacy_files_with_the_report_and_the_recent_list() {
    let mut r = rt();
    let legacy = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../mdaw-model/testdata/legacy-v1.0.mdaw"
    );
    press(&mut r, ev(Key::Command('o'), cmd()));
    let (k, o) = r.file_dialog().unwrap();
    assert_eq!(
        (k, o.title.as_str()),
        (FileDialogKind::Open, "Open Project")
    );
    answer(&mut r, FileDialogKind::Open, Some(legacy));
    let doc = r.state.svc.engine.doc();
    assert_eq!(doc.get_bpm(), 96.0);
    assert_eq!(doc.get_arrangement().name, "Old Song");
    let drums = doc.get_tracks().iter().find(|t| t.name == "Drums").unwrap();
    assert!(
        drums.instrument.kit.is_some(),
        "GM drums migrated onto the factory kit"
    );
    assert!(!r.state.dirty(), "opening is not an edit");
    assert_eq!(r.state.project.title, "legacy-v1.0 - Micro DAW");
    // The migration's warning is shown (Electron only logs it).
    let rep = r.state.project.report.clone().expect("report");
    assert!(rep.error.is_none());
    assert!(
        rep.warnings
            .iter()
            .any(|w| w.contains("legacy drum note(s) have no pad and were dropped")),
        "{:?}",
        rep.warnings
    );
    assert!(has(&r, "report-warnings"));
    press(&mut r, ev(Key::Escape, Mods::default()));
    assert!(!has(&r, "report"));
    assert_eq!(r.state.project.recent[0].name, "legacy-v1.0");
    assert_eq!(r.state.project.recent[0].file_path, legacy);
    // v1.3 (track FX, automation, markers) opens too.
    let v13 = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../mdaw-model/testdata/legacy-v1.3.mdaw"
    );
    r.update(|s, tx| s.request_project_op(ProjectOp::OpenPath(v13.into()), tx));
    frames(&mut r, 2);
    assert_eq!(r.state.project.file_path.as_deref(), Some(v13));
    assert_eq!(r.state.project.recent.len(), 2);
    assert_eq!(r.state.project.recent[0].name, "legacy-v1.3");
}

#[test]
fn a_broken_file_reports_and_keeps_the_project() {
    let dir = tmp_dir();
    let mut r = rt();
    let bad = p(&dir, "bad.mdaw");
    std::fs::write(&bad, "{ not json").unwrap();
    let before = r.state.svc.engine.doc().get_arrangement().id.clone();
    r.update(|s, tx| s.request_project_op(ProjectOp::OpenPath(bad.clone()), tx));
    frames(&mut r, 2);
    let rep = r.state.project.report.clone().unwrap();
    assert!(rep.error.unwrap().starts_with("Failed to load project:"));
    assert!(has(&r, "report-error"));
    assert_eq!(r.state.svc.engine.doc().get_arrangement().id, before);
    assert!(r.state.project.file_path.is_none());
    assert!(r.state.project.recent.is_empty());
    click(&mut r, "report-ok");
    assert!(r.state.project.report.is_none());
    // A missing file reports too.
    r.update(|s, tx| s.request_project_op(ProjectOp::OpenPath(p(&dir, "gone.mdaw")), tx));
    frames(&mut r, 2);
    assert!(r.state.project.report.as_ref().unwrap().error.is_some());
}

#[test]
fn a_cancelled_open_does_not_swallow_the_next_edit() {
    // Electron bug fixed in both apps (user decision 2026-11-18): `useProject` set
    // `skipDirtyRef` before an open / new that was then cancelled, so the next edit did not
    // mark the project dirty and the close guard let it go unsaved.
    let mut r = rt();
    press(&mut r, ev(Key::Command('o'), cmd()));
    answer(&mut r, FileDialogKind::Open, None);
    assert!(!r.state.dirty());
    edit(&mut r);
    assert!(r.state.dirty());
}

#[test]
fn recent_projects_order_limit_sidebar_and_palette() {
    let dir = tmp_dir();
    let mut r = rt();
    let files: Vec<String> = (0..12).map(|i| p(&dir, &format!("p{i}.mdaw"))).collect();
    for f in &files {
        r.update(|s, _| {
            s.write_project(f);
        });
    }
    let names: Vec<&str> = r
        .state
        .project
        .recent
        .iter()
        .map(|m| m.name.as_str())
        .collect();
    assert_eq!(
        names,
        ["p11", "p10", "p9", "p8", "p7", "p6", "p5", "p4", "p3", "p2"]
    );
    // Re-opening moves an entry to the front without duplicating it.
    r.update(|s, tx| s.request_project_op(ProjectOp::OpenPath(files[5].clone()), tx));
    frames(&mut r, 2);
    let names: Vec<&str> = r
        .state
        .project
        .recent
        .iter()
        .map(|m| m.name.as_str())
        .collect();
    assert_eq!(
        names,
        ["p5", "p11", "p10", "p9", "p8", "p7", "p6", "p4", "p3", "p2"]
    );
    // Persisted under `micro-daw-recent-projects` in the TS shape.
    let stored = store::read_recent_projects(&r.state.svc.settings.borrow());
    assert_eq!(stored, r.state.project.recent);
    // The expanded sidebar lists them; a click opens one.
    r.state.sidebar_open = true;
    r.invalidate();
    run(&mut r, 0.6);
    assert!(has(&r, "recent-9"));
    click(&mut r, "recent-1");
    assert_eq!(
        r.state.project.file_path.as_deref(),
        Some(files[11].as_str())
    );
    // The collapsed sidebar hides them (`group-data-[collapsible=icon]:hidden`).
    r.state.sidebar_open = false;
    r.invalidate();
    frames(&mut r, 2);
    assert!(!has(&r, "recent-0"));
    // The palette's "Recent Projects" group (path subtitle, searchable by path).
    press(&mut r, ev(Key::Command('k'), cmd()));
    assert!(has(&r, "cmd-recent-0"));
    let items = crate::ui::p9::dialogs::palette_items_with(
        AppMode::Daw,
        "p4.mdaw",
        &r.state.project.recent,
    );
    assert_eq!(items[0].id, "recent-7");
    assert_eq!(items[0].subtitle.as_deref(), Some(files[4].as_str()));
    r.update(|s, tx| s.run_command("recent-2", tx));
    frames(&mut r, 2);
    assert_eq!(
        r.state.project.file_path.as_deref(),
        Some(files[10].as_str())
    );
}

#[test]
fn opening_while_dirty_asks_first() {
    let dir = tmp_dir();
    let mut r = rt();
    let f = p(&dir, "other.mdaw");
    r.update(|s, _| {
        s.write_project(&f);
    });
    edit(&mut r);
    r.update(|s, tx| s.request_project_op(ProjectOp::OpenPath(f.clone()), tx));
    frames(&mut r, 2);
    assert_eq!(
        r.state.project.unsaved,
        Some(ProjectOp::OpenPath(f.clone()))
    );
    // The prompt names the current project (a successful write makes the path the
    // project's, review finding 11: only after the file exists).
    assert_eq!(r.state.project_name(), "other");
    click(&mut r, "unsaved-cancel");
    assert!(r.state.dirty());
    r.update(|s, tx| s.request_project_op(ProjectOp::OpenPath(f.clone()), tx));
    frames(&mut r, 2);
    click(&mut r, "unsaved-dont-save");
    assert!(!r.state.dirty());
    assert_eq!(
        r.state.svc.engine.doc().get_bpm(),
        120.0,
        "the file's tempo, not the edit"
    );
}

#[test]
fn a_dropped_mdaw_opens_and_audio_files_do_not() {
    let dir = tmp_dir();
    let mut r = rt();
    let f = p(&dir, "dropped.mdaw");
    r.update(|s, _| {
        s.svc.engine.edit(|d| d.set_bpm(133.0));
        s.write_project(&f);
        s.svc.engine.edit(|d| d.set_bpm(90.0));
    });
    r.update(|s, _| s.clean_version = s.svc.engine.doc().state_version());
    r.drop_files(vec![PathBuf::from(&f)], Some((700.0, 60.0)));
    frames(&mut r, 2);
    assert_eq!(r.state.svc.engine.doc().get_bpm(), 133.0);
    assert_eq!(r.state.project.file_path.as_deref(), Some(f.as_str()));
    r.drop_files(vec![dir.join("x.txt")], Some((700.0, 60.0)));
    frames(&mut r, 2);
    assert_eq!(r.state.project.file_path.as_deref(), Some(f.as_str()));
}

#[test]
fn quit_keeps_the_unsaved_session_for_the_next_launch() {
    // Rune cannot cancel a quit (⌘Q): the autosave and the session carry the unsaved project
    // into the next launch, dirty.
    let dir = tmp_dir();
    let f = p(&dir, "song.mdaw");
    let data = tmp_dir();
    {
        let mut a = App::with_settings(
            crate::services::RunMode::Headless,
            store::SettingsStore::open(&data),
        );
        a.svc.save_headless = true;
        a.project.file_path = Some(f.clone());
        a.write_project(&f);
        a.svc.engine.edit(|d| d.set_bpm(140.0));
        assert!(a.dirty());
        a.shutdown();
    }
    let mut b = App::with_settings(
        crate::services::RunMode::Headless,
        store::SettingsStore::open(&data),
    );
    assert!(b.svc.restored_autosave);
    b.restore_session();
    assert_eq!(b.project.file_path.as_deref(), Some(f.as_str()));
    assert_eq!(b.svc.engine.doc().get_bpm(), 140.0);
    assert!(b.dirty());
    assert_eq!(
        mdaw_model::migrate::window_title(b.project.file_path.as_deref(), b.dirty()),
        "song * - Micro DAW"
    );
    // A clean session comes back clean.
    b.svc.save_headless = true;
    b.write_project(&f);
    b.shutdown();
    let mut c = App::with_settings(
        crate::services::RunMode::Headless,
        store::SettingsStore::open(&data),
    );
    c.restore_session();
    assert_eq!(c.project.file_path.as_deref(), Some(f.as_str()));
    assert!(!c.dirty());
}

#[test]
fn a_rust_saved_project_reopens_identically() {
    let dir = tmp_dir();
    let mut r = rt();
    r.update(|s, _| {
        let arr = crate::views::templates()[2].arrangement.clone();
        let _ = s.svc.engine.edit(|d| d.load_from_template(&arr));
    });
    let f = p(&dir, "rt.mdaw");
    r.update(|s, _| {
        s.write_project(&f);
    });
    let before = r.state.svc.engine.doc().get_arrangement().clone();
    r.update(|s, _| s.svc.engine.edit(|d| d.reset_to_starter()));
    r.update(|s, _| {
        assert!(s.load_project_file(&f));
    });
    assert_eq!(r.state.svc.engine.doc().get_arrangement(), &before);
    assert!(
        r.state.project.report.is_none(),
        "{:?}",
        r.state.project.report
    );
}

#[test]
fn the_fullscreen_button_and_min_size() {
    let spec = crate::spec(true);
    assert_eq!(spec.min_size, Some((1360.0, 768.0)));
    assert_eq!(spec.title, "Untitled - Micro DAW");
    let r = rt();
    assert!(has(&r, "fullscreen"));
}

// --- cross-app goldens -------------------------------------------------------------------------
//
// `rune/parity/fixtures/project-saved-by-rust.mdaw` is written by the Rust writer below and
// opened by Electron's real `ProjectManager` in `src/services/ProjectManager.golden.test.ts`;
// `project-saved-by-electron.mdaw` is written by Electron's `ProjectManager.save` there and
// opened here. `UPDATE_GOLDENS=1` rewrites the one each side owns.

const RUST_GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../parity/fixtures/project-saved-by-rust.mdaw"
);
const ELECTRON_GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../parity/fixtures/project-saved-by-electron.mdaw"
);

/// The Rust writer's file for a fixed project: the hip-hop template with deterministic ids, a
/// marker, a group, loop markers, an audio track with a region (sample manifest), master volume
/// and live FX set.
fn rust_golden_json() -> String {
    use mdaw_model::doc::ArrangementDoc;
    use mdaw_model::types::{InstrumentType, SampleRef};
    let mut n = 0u32;
    let mut doc = ArrangementDoc::with_id_generator(Box::new(move || {
        n += 1;
        format!("id-{n:03}")
    }));
    let arr = crate::views::templates()[2].arrangement.clone();
    doc.load_from_template(&arr).unwrap();
    doc.add_marker("Hook", 16.0, None);
    let ids: Vec<String> = doc
        .get_tracks()
        .iter()
        .take(2)
        .map(|t| t.id.clone())
        .collect();
    doc.create_group("Rhythm", &ids);
    let audio = doc.add_track(InstrumentType::Audio, 0);
    let sample = SampleRef {
        id: "sample-1".into(),
        name: "Vox Chop".into(),
        path: "/Users/someone/Samples/vox chop.wav".into(),
        duration_seconds: 1.5,
        sample_rate: 44100.0,
        channels: 2,
    };
    doc.add_audio_region(&audio.id, Some(&sample), 4.0).unwrap();
    let fx = mdaw_model::types::EffectParams {
        reverb_wet: 0.25,
        chorus_depth: 0.4,
        filter_cutoff: 9000.0,
        filter_resonance: 2.0,
        ..Default::default()
    };
    let file = mdaw_model::migrate::build_project_file(
        doc.get_arrangement().clone(),
        -9.0,
        fx,
        "2026-11-19T09:00:00.000Z",
        "2026-11-19T10:30:00.000Z",
        &|id| (id == "sample-1").then(|| "Vox Chop".to_owned()),
    );
    mdaw_model::migrate::project_file_to_json(&file)
}

#[test]
fn golden_the_rust_writer_output_is_the_committed_fixture() {
    let json = rust_golden_json();
    if std::env::var_os("UPDATE_GOLDENS").is_some() {
        std::fs::write(RUST_GOLDEN, &json).unwrap();
    }
    let committed = std::fs::read_to_string(RUST_GOLDEN).expect("golden (UPDATE_GOLDENS=1)");
    assert_eq!(
        json, committed,
        "the Rust writer changed: rerun with UPDATE_GOLDENS=1 and the vitest golden"
    );
}

/// JSON with every number as f64 (`90` == `90.0`).
fn norm(v: &serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match v {
        Value::Number(n) => serde_json::json!(n.as_f64().unwrap()),
        Value::Array(a) => Value::Array(a.iter().map(norm).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, x)| (k.clone(), norm(x))).collect()),
        x => x.clone(),
    }
}

#[test]
fn golden_a_project_saved_by_electron_opens_unchanged() {
    let raw = std::fs::read_to_string(ELECTRON_GOLDEN).expect("written by the vitest golden");
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let mut r = rt();
    r.update(|s, _| {
        assert!(s.load_project_file(ELECTRON_GOLDEN));
    });
    frames(&mut r, 2);
    // Only the manifest's sample (a path on another machine) is reported, like the TS warning.
    let rep = r.state.project.report.clone().unwrap();
    assert_eq!(
        rep.warnings,
        vec![
            "could not load sample \"vox chop\" from /Users/someone/Samples/vox chop.wav"
                .to_owned()
        ]
    );
    let opened = serde_json::to_value(r.state.svc.engine.doc().get_arrangement()).unwrap();
    // The same arrangement, value for value (numbers compare as numbers: JS writes 90, serde
    // 90.0).
    assert_eq!(norm(&opened), norm(&v["arrangement"]));
    assert_eq!(
        r.state.svc.engine.get_master_volume_db(),
        v["audioConfig"]["masterVolume"].as_f64().unwrap()
    );
    let fx = r.state.svc.engine.live_effect_params();
    assert_eq!(
        fx.reverb_wet,
        v["audioConfig"]["effectParams"]["reverbWet"]
            .as_f64()
            .unwrap()
    );
    assert_eq!(
        fx.filter_cutoff,
        v["audioConfig"]["effectParams"]["filterCutoff"]
            .as_f64()
            .unwrap()
    );
    // Saving it again from Rust keeps the arrangement and the audio config.
    let dir = tmp_dir();
    let f = p(&dir, "again.mdaw");
    r.update(|s, _| {
        s.project.created_at = v["createdAt"].as_str().unwrap().to_owned();
        s.write_project(&f);
    });
    let again: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
    assert_eq!(norm(&again["arrangement"]), norm(&v["arrangement"]));
    assert_eq!(again["createdAt"], v["createdAt"]);
    assert_eq!(again["sampleManifest"], v["sampleManifest"]);
}
