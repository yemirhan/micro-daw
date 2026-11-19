//! Phase 10 review fixes (`docs/rune-port/review-phase-10.md`): crash recovery, autosave
//! durability, New into a missing folder, dialogs over the start screen, project IO off the UI
//! thread, sample decode failures in the load report, the window-close MIDI suspension, exports
//! and the Electron import's robustness.
//!
//! A "crash" is an app dropped without `shutdown()` (no close or quit hook ran): only what the
//! autosave checkpoints already wrote survives, as after a real crash.

use std::path::{Path, PathBuf};

use mdaw_model::settings::{self as store, SettingsStore};
use rune::{FileDialogKind, Key, Mods};

use super::harness::*;
use crate::app::App;
use crate::project::ProjectOp;
use crate::services::RunMode;

fn tmp_dir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("mdaw-p10r-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn p(dir: &Path, name: &str) -> String {
    dir.join(name).to_string_lossy().into_owned()
}

/// A launch over the data folder `data` (file-backed store, checkpoints on), started, in a
/// runtime; the session restored as `App::window_with` does.
fn launch(data: &Path) -> rune::Runtime<App> {
    let mut a = App::with_settings(RunMode::Headless, SettingsStore::open(data));
    a.svc.save_headless = true;
    a.recovery_dir = Some(crate::recovery::recovery_dir_in(data));
    a.restore_session();
    crate::scenes::started(&mut a);
    runtime(a)
}

fn set_bpm(r: &mut rune::Runtime<App>, bpm: f64) {
    r.update(|s, _| s.set_bpm(bpm));
    frames(r, 2);
}

/// Lets the 300 ms autosave debounce run out.
fn settle(r: &mut rune::Runtime<App>) {
    run(r, 0.5);
    assert!(!r.state.svc.save_pending(), "the checkpoint is durable");
}

fn file_bpm(path: &str) -> f64 {
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    v["arrangement"]["bpm"].as_f64().unwrap()
}

fn title(r: &rune::Runtime<App>) -> String {
    mdaw_model::migrate::window_title(r.state.project.file_path.as_deref(), r.state.dirty())
}

// --- findings 1–3: the checkpoint -----------------------------------------------------------

#[test]
fn a_crash_after_opening_another_project_reopens_that_project_not_the_first() {
    // Review finding 1 (both reviewers' repro): A open, B opened and edited, crash. Before: the
    // session record (written only on close / quit) still named A, so B's arrangement came back
    // as "A", clean, and ⌘S wrote it over A.
    let data = tmp_dir();
    let dir = tmp_dir();
    let (fa, fb) = (p(&dir, "A.mdaw"), p(&dir, "B.mdaw"));
    {
        let mut r = launch(&data);
        set_bpm(&mut r, 101.0);
        r.update(|s, _| assert!(s.write_project(&fa)));
        set_bpm(&mut r, 102.0);
        r.update(|s, _| assert!(s.write_project(&fb)));
        r.update(|s, _| assert!(s.load_project_file(&fa)));
        settle(&mut r);
        r.update(|s, _| assert!(s.load_project_file(&fb)));
        set_bpm(&mut r, 133.0);
        settle(&mut r);
        // Crash: dropped, no hook.
    }
    let mut r = launch(&data);
    assert_eq!(r.state.project.file_path.as_deref(), Some(fb.as_str()));
    assert_eq!(r.state.svc.engine.doc().get_bpm(), 133.0);
    assert!(
        r.state.dirty(),
        "the unsaved edit comes back unsaved (finding 2)"
    );
    assert_eq!(title(&r), "B * - Micro DAW");
    // ⌘S saves B where it belongs; A is untouched.
    press(&mut r, ev(Key::Command('s'), cmd()));
    frames(&mut r, 2);
    assert_eq!(file_bpm(&fb), 133.0);
    assert_eq!(file_bpm(&fa), 101.0);
    assert!(!r.state.dirty());
}

#[test]
fn a_checkpoint_that_doesnt_match_its_arrangement_restores_untitled_and_unsaved() {
    // An arrangement written apart from its record (the pre-fix app wrote them separately; a
    // foreign writer): the path is not trusted.
    let data = tmp_dir();
    let dir = tmp_dir();
    let fa = p(&dir, "A.mdaw");
    {
        let mut r = launch(&data);
        r.update(|s, _| assert!(s.write_project(&fa)));
        settle(&mut r);
    }
    {
        let mut st = SettingsStore::open(&data);
        let mut doc = mdaw_model::doc::ArrangementDoc::new();
        doc.reset_to_starter();
        doc.set_bpm(150.0);
        st.set_item(store::ARRANGEMENT_STORAGE_KEY, doc.save_to_local_storage())
            .unwrap();
    }
    let mut r = launch(&data);
    assert_eq!(r.state.svc.engine.doc().get_bpm(), 150.0);
    assert_eq!(r.state.project.file_path, None, "not attached to A");
    assert!(r.state.dirty());
    assert!(has(&r, "recovery-note"));
    // ⌘S asks for a new path instead of writing over A.
    press(&mut r, ev(Key::Command('s'), cmd()));
    frames(&mut r, 2);
    assert_eq!(r.file_dialog().map(|d| d.0), Some(FileDialogKind::Save));
    click(&mut r, "recovery-note-dismiss");
    assert!(!has(&r, "recovery-note"));
}

#[test]
fn a_file_changed_on_disk_since_the_checkpoint_is_not_overwritten() {
    let data = tmp_dir();
    let dir = tmp_dir();
    let fa = p(&dir, "A.mdaw");
    {
        let mut r = launch(&data);
        r.update(|s, _| assert!(s.write_project(&fa)));
        set_bpm(&mut r, 140.0);
        settle(&mut r);
    }
    // Another app saved A meanwhile.
    let mut v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&fa).unwrap()).unwrap();
    v["modifiedAt"] = "2030-01-01T00:00:00.000Z".into();
    std::fs::write(&fa, v.to_string()).unwrap();
    let r = launch(&data);
    assert_eq!(
        r.state.svc.engine.doc().get_bpm(),
        140.0,
        "the work is kept"
    );
    assert_eq!(r.state.project.file_path, None);
    assert!(r.state.dirty());
    assert!(
        r.state
            .shell
            .recovery_note
            .as_deref()
            .unwrap()
            .contains("\u{201c}A\u{201d} changed on disk")
    );
    // A missing file: the same.
    std::fs::remove_file(&fa).unwrap();
    let r = launch(&data);
    assert_eq!(r.state.project.file_path, None);
    assert!(r.state.dirty());
}

#[test]
fn unsaved_edits_survive_a_crash_as_unsaved() {
    // Review finding 2: before, a crash brought the edits back clean (the record said clean).
    let data = tmp_dir();
    let dir = tmp_dir();
    let fa = p(&dir, "A.mdaw");
    {
        let mut r = launch(&data);
        r.update(|s, _| assert!(s.write_project(&fa)));
        settle(&mut r);
        set_bpm(&mut r, 99.0);
        settle(&mut r);
    }
    let r = launch(&data);
    assert_eq!(r.state.project.file_path.as_deref(), Some(fa.as_str()));
    assert_eq!(r.state.svc.engine.doc().get_bpm(), 99.0);
    assert!(r.state.dirty());
    // A clean project comes back clean.
    let data = tmp_dir();
    {
        let mut r = launch(&data);
        set_bpm(&mut r, 98.0);
        r.update(|s, _| assert!(s.write_project(&fa)));
        settle(&mut r);
    }
    let r = launch(&data);
    assert_eq!(r.state.project.file_path.as_deref(), Some(fa.as_str()));
    assert!(!r.state.dirty());
}

#[test]
fn an_untitled_unsaved_project_comes_back_unsaved_after_a_quit_and_a_crash() {
    // Review finding 3: the restore returned early without a path, so the work came back clean
    // and the next close let it go without asking.
    for crash in [false, true] {
        let data = tmp_dir();
        {
            let mut r = launch(&data);
            set_bpm(&mut r, 111.0);
            if crash {
                settle(&mut r);
            } else {
                r.state.shutdown();
            }
        }
        let mut r = launch(&data);
        assert_eq!(r.state.project.file_path, None);
        assert_eq!(r.state.svc.engine.doc().get_bpm(), 111.0);
        assert!(r.state.dirty(), "crash = {crash}");
        assert_eq!(title(&r), "Untitled * - Micro DAW");
        // The close guard asks.
        r.update(|s, tx| s.on_window_close(tx));
        frames(&mut r, 2);
        assert_eq!(r.state.project.unsaved, Some(ProjectOp::Close));
    }
}

// --- finding 4: autosave failures -------------------------------------------------------------

#[test]
fn a_failed_autosave_is_shown_kept_pending_and_retried() {
    let data = tmp_dir();
    let mut r = launch(&data);
    // The data folder becomes unwritable (a file where the folder was).
    std::fs::remove_dir_all(&data).unwrap();
    std::fs::write(&data, b"not a folder").unwrap();
    set_bpm(&mut r, 123.0);
    run(&mut r, 0.5);
    assert!(r.state.svc.autosave_error.is_some(), "the failure is kept");
    assert!(r.state.svc.save_pending(), "not acknowledged");
    assert!(has(&r, "autosave-error"));
    // Retried while it fails.
    run(&mut r, crate::recovery::AUTOSAVE_RETRY + 0.2);
    assert!(r.state.svc.autosave_error.is_some());
    // The folder is back: the next retry writes, the note goes.
    std::fs::remove_file(&data).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    run(&mut r, crate::recovery::AUTOSAVE_RETRY + 0.2);
    assert!(r.state.svc.autosave_error.is_none());
    assert!(!r.state.svc.save_pending());
    assert!(!has(&r, "autosave-error"));
    let st = SettingsStore::open(&data);
    let mut doc = mdaw_model::doc::ArrangementDoc::new();
    assert!(matches!(
        doc.load_from_local_storage(st.get_item(store::ARRANGEMENT_STORAGE_KEY)),
        Ok(Some(_))
    ));
    assert_eq!(doc.get_bpm(), 123.0);
}

// --- finding 6: generated samples and live FX -------------------------------------------------

#[test]
fn timeline_generated_samples_and_live_fx_survive_a_restart() {
    use mdaw_engine::fx::live::LiveOp;
    let data = tmp_dir();
    let frames_in = 4800;
    let id = {
        let mut r = launch(&data);
        let id = {
            let s = &mut r.state;
            let b = mdaw_dsp::buffer::AudioBuffer {
                sample_rate: 48_000,
                channels: vec![
                    (0..frames_in)
                        .map(|i| (i as f32 * 0.01).sin() * 0.5)
                        .collect(),
                ],
            };
            let sample = s.p9.samples.register_buffer("sine 440Hz 1s", b);
            s.svc.engine.edit(|d| d.import_audio_sample(&sample));
            s.svc.engine.live_fx(LiveOp::ReverbWet(0.7));
            sample.id
        };
        set_bpm(&mut r, 121.0);
        settle(&mut r);
        // The WAV is written, then the checkpoint lists it.
        settle_more(&mut r);
        let rec = crate::recovery::session_record_of(&r.state.svc.settings.borrow()).unwrap();
        assert_eq!(rec["assets"][0]["sampleId"], id.as_str());
        id
    };
    let mut r = launch(&data);
    frames(&mut r, 3);
    let b = r.state.p9.samples.buffer(&id).expect("the sample is back");
    assert_eq!(b.channels[0].len(), frames_in);
    assert_eq!(
        r.state.p9.samples.get(&id).map(|s| s.name.as_str()),
        Some("sine 440Hz 1s")
    );
    assert!((r.state.svc.engine.live_effect_params().reverb_wet - 0.7).abs() < 1e-9);
}

fn settle_more(r: &mut rune::Runtime<App>) {
    run(r, 0.5);
}

// --- finding 7: exports -----------------------------------------------------------------------

#[test]
fn an_export_replaces_the_file_atomically_and_a_quit_cancels_one_that_runs_on() {
    let dir = tmp_dir();
    let f = dir.join("song.mid");
    std::fs::write(&f, b"previous export").unwrap();
    let mut r = rt();
    r.update(|s, _| s.export_to(crate::views::ExportKind::Midi, f.clone()));
    frames(&mut r, 2);
    let bytes = std::fs::read(&f).unwrap();
    assert!(bytes.starts_with(b"MThd"));
    let left: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
    assert_eq!(left.len(), 1, "no temp file left");
    // A quit while an export runs past the wait: cancelled, nothing written.
    let cancel = r.state.p9.export.fake_running_for_test();
    r.state
        .finish_export_for_quit(std::time::Duration::from_millis(20));
    assert!(r.state.p9.export.cancelled_at_quit);
    assert!(cancel.load(std::sync::atomic::Ordering::Acquire));
    assert!(r.state.p9.export.running.is_none());
}

// --- finding 11: New into a folder that doesn't exist ----------------------------------------

#[test]
fn new_into_a_missing_folder_stays_untitled_and_unsaved_with_the_error() {
    let mut r = rt();
    press(&mut r, ev(Key::Command('n'), cmd()));
    frames(&mut r, 2);
    let missing = std::env::temp_dir()
        .join(format!("mdaw-missing-{}", uuid::Uuid::new_v4()))
        .join("New.mdaw");
    r.resolve_file_dialog(Some(missing.clone())).unwrap();
    frames(&mut r, 2);
    assert!(!missing.exists());
    assert_eq!(
        r.state.project.file_path, None,
        "before: the path was taken and marked clean before the write"
    );
    assert!(r.state.dirty());
    assert_eq!(title(&r), "Untitled * - Micro DAW");
    assert!(r.state.project.write_error.is_some());
    assert!(has(&r, "report-ok"));
}

// --- finding 12: the shell's dialogs over the start screen ------------------------------------

#[test]
fn a_restored_unsaved_project_prompts_over_the_start_screen() {
    let data = tmp_dir();
    {
        let mut r = launch(&data);
        set_bpm(&mut r, 90.0);
        settle(&mut r);
    }
    let mut a = App::with_settings(RunMode::Headless, SettingsStore::open(&data));
    a.restore_session();
    let mut r = runtime(a);
    assert!(!r.state.started(), "on the start screen");
    r.update(|s, tx| s.on_window_close(tx));
    frames(&mut r, 2);
    assert_eq!(r.state.project.unsaved, Some(ProjectOp::Close));
    assert!(
        has(&r, "unsaved-cancel"),
        "the prompt is drawn over the start screen"
    );
    // Escape cancels it (and doesn't start the session).
    press(&mut r, ev(Key::Escape, Mods::default()));
    frames(&mut r, 2);
    assert_eq!(r.state.project.unsaved, None);
    assert!(!r.state.started());
    // The load report too.
    r.update(|s, _| {
        s.load_project_file("/nonexistent/x.mdaw");
    });
    frames(&mut r, 2);
    assert!(has(&r, "report-ok"));
    press(&mut r, ev(Key::Enter, Mods::default()));
    frames(&mut r, 2);
    assert!(r.state.project.report.is_none());
    assert!(
        !r.state.started(),
        "Enter closed the report, not the start screen"
    );
}

// --- finding 13: project IO on a worker thread -------------------------------------------------

#[test]
fn saves_and_loads_run_on_the_io_worker_and_an_edit_during_a_save_stays_unsaved() {
    let dir = tmp_dir();
    let f = p(&dir, "worker.mdaw");
    let mut r = rt();
    r.state.io = crate::io::IoWorker::new(false);
    set_bpm(&mut r, 125.0);
    r.update(|s, _| {
        assert!(s.write_project(&f), "submitted");
        assert!(s.project.busy);
        // An edit while the write runs.
        s.set_bpm(126.0);
    });
    let t = std::time::Instant::now();
    while r.state.project.busy {
        assert!(t.elapsed().as_secs() < 10);
        frames(&mut r, 1);
    }
    assert_eq!(file_bpm(&f), 125.0, "the snapshot taken at submit");
    assert_eq!(r.state.project.file_path.as_deref(), Some(f.as_str()));
    assert!(
        r.state.dirty(),
        "the edit made during the write is not marked saved"
    );
    // A load through the worker.
    r.update(|s, _| assert!(s.load_project_file(&f)));
    while r.state.project.busy {
        frames(&mut r, 1);
    }
    assert_eq!(r.state.svc.engine.doc().get_bpm(), 125.0);
    assert!(!r.state.dirty());
}

// --- finding 15: edits while a dialog is pending -----------------------------------------------

#[test]
fn an_edit_while_the_open_dialog_is_pending_marks_the_project_unsaved() {
    let mut r = rt();
    press(&mut r, ev(Key::Command('o'), cmd()));
    frames(&mut r, 2);
    assert!(r.file_dialog().is_some());
    set_bpm(&mut r, 141.0);
    assert!(r.state.dirty(), "an edit during the pending Open counts");
    r.resolve_file_dialog(None).unwrap();
    frames(&mut r, 2);
    assert!(r.state.dirty());
}

// --- finding 16: an existing sample that can't be decoded --------------------------------------

#[test]
fn a_manifest_sample_that_cannot_be_decoded_is_in_the_load_report() {
    let dir = tmp_dir();
    let bad = dir.join("broken.wav");
    std::fs::write(&bad, b"RIFF\x10\x00\x00\x00WAVEjunkjunkjunk").unwrap();
    let f = p(&dir, "with-sample.mdaw");
    let mut r = rt();
    r.update(|s, _| assert!(s.write_project(&f)));
    let mut v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
    v["sampleManifest"] = serde_json::json!([
        {"sampleId": "smp-broken", "path": bad.to_string_lossy(), "name": "Broken"}
    ]);
    std::fs::write(&f, v.to_string()).unwrap();
    r.update(|s, _| assert!(s.load_project_file(&f)));
    frames(&mut r, 3);
    let rep = r.state.project.report.clone().expect("a report");
    assert!(
        rep.warnings
            .iter()
            .any(|w| w.starts_with("could not load sample \"Broken\"")),
        "{rep:?}"
    );
    // A later load of another project doesn't get reports of this one's samples.
    assert!(r.state.project.pending_samples.is_empty());
}

// --- finding 17: MIDI while the window is closed; quit without a window ------------------------

#[test]
fn a_closed_window_suspends_midi_and_a_windowless_quit_still_runs_its_cleanups() {
    let mut r = rt();
    r.update(|s, tx| s.on_window_close(tx));
    frames(&mut r, 1);
    let suspended =
        |r: &rune::Runtime<App>| r.state.svc.midi.host.shared().lock().router.suspended();
    assert!(
        suspended(&r),
        "nothing sounds from the controller without a window"
    );
    assert!(r.state.shell.window_closed);
    r.update(|s, _| s.on_reopen());
    assert!(!suspended(&r));
    // Closed again, then quit from the Dock with no window: the quit's cleanups run.
    r.update(|s, tx| s.on_window_close(tx));
    r.state.shutdown();
    assert!(r.state.shell.quit_paused);
}

// --- finding 8: the Electron import -------------------------------------------------------------

fn fixture_copy() -> PathBuf {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/electron-profile");
    let dst = tmp_dir();
    let ls = dst.join("Local Storage").join("leveldb");
    std::fs::create_dir_all(&ls).unwrap();
    for e in std::fs::read_dir(src.join("Local Storage").join("leveldb")).unwrap() {
        let e = e.unwrap();
        std::fs::copy(e.path(), ls.join(e.file_name())).unwrap();
    }
    dst
}

/// The fixture compacted by `rusty-leveldb` into `.ldb` tables (the shape of a real, older
/// profile), with one more record in a fresh log.
fn compacted_profile() -> PathBuf {
    let prof = fixture_copy();
    let ls = prof.join("Local Storage").join("leveldb");
    {
        let opts = rusty_leveldb::Options {
            create_if_missing: false,
            ..Default::default()
        };
        let mut db = rusty_leveldb::DB::open(&ls, opts).unwrap();
        db.compact_range(b"\x00", b"\xff").unwrap();
        db.flush().unwrap();
        let mut key = b"_file://\x00\x01".to_vec();
        key.extend_from_slice(b"micro-daw:midi-split-point");
        db.put(&key, b"\x0152").unwrap();
        db.flush().unwrap();
        db.close().unwrap();
    }
    let _ = std::fs::remove_file(ls.join("LOCK"));
    prof
}

#[test]
fn the_import_detects_a_damaged_log_instead_of_importing_nothing() {
    // GPT-6.1 Sol's repro: one flipped byte in the write-ahead log read as an empty profile
    // and the import was marked done.
    let prof = fixture_copy();
    let ls = prof.join("Local Storage").join("leveldb");
    let log = std::fs::read_dir(&ls)
        .unwrap()
        .flatten()
        .find(|e| e.file_name().to_string_lossy().ends_with(".log"))
        .unwrap()
        .path();
    let mut b = std::fs::read(&log).unwrap();
    b[200] ^= 0x40;
    std::fs::write(&log, b).unwrap();
    let mut st = SettingsStore::in_memory();
    let sum = crate::electron_import::import_once(&mut st, Some(&prof)).expect("a note");
    assert!(
        sum.error.as_deref().unwrap().contains("damaged"),
        "{:?}",
        sum.error
    );
    let mark: serde_json::Value = serde_json::from_str(
        st.get_item(crate::electron_import::IMPORT_MARK_KEY)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        mark["status"], "error",
        "not done: tried again at the next launch"
    );
    assert_eq!(mark["attempts"], 1);
    assert!(sum.imported.is_empty());
}

#[test]
fn an_import_that_crashed_is_counted_and_given_up_after_three_launches() {
    let prof = fixture_copy();
    let mut st = SettingsStore::in_memory();
    // A launch that died mid-import left this mark.
    st.set_item(
        crate::electron_import::IMPORT_MARK_KEY,
        r#"{"status":"in-progress","attempts":3}"#,
    )
    .unwrap();
    let sum = crate::electron_import::import_once(&mut st, Some(&prof)).expect("a note");
    assert!(sum.error.as_deref().unwrap().contains("stopped 3 times"));
    assert!(crate::electron_import::import_once(&mut st, Some(&prof)).is_none());
    // "Retry import" runs it anyway.
    let sum = crate::electron_import::import_retry(&mut st, Some(&prof)).expect("imported");
    assert!(sum.error.is_none());
    assert_eq!(
        sum.imported.len(),
        19,
        "every key but the never-written recordings"
    );
    // An in-progress mark below the limit is retried at once.
    let mut st = SettingsStore::in_memory();
    st.set_item(
        crate::electron_import::IMPORT_MARK_KEY,
        r#"{"status":"in-progress","attempts":1}"#,
    )
    .unwrap();
    let sum = crate::electron_import::import_once(&mut st, Some(&prof)).unwrap();
    assert!(sum.error.is_none());
    let mark: serde_json::Value = serde_json::from_str(
        st.get_item(crate::electron_import::IMPORT_MARK_KEY)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        (mark["status"].as_str(), mark["attempts"].as_u64()),
        (Some("done"), Some(2))
    );
}

#[test]
fn a_compacted_profile_imports_the_midi_split_point() {
    // Review finding 9: `micro-daw:midi-split-point` (both apps persist it) was not imported.
    let prof = compacted_profile();
    let ls = prof.join("Local Storage").join("leveldb");
    assert!(
        std::fs::read_dir(&ls)
            .unwrap()
            .flatten()
            .any(|e| e.file_name().to_string_lossy().ends_with(".ldb")),
        "the fixture has tables"
    );
    let mut st = SettingsStore::in_memory();
    let sum = crate::electron_import::import_once(&mut st, Some(&prof)).unwrap();
    assert!(sum.error.is_none(), "{:?}", sum.error);
    assert!(
        sum.imported
            .contains(&store::MIDI_SPLIT_POINT_KEY.to_owned())
    );
    assert_eq!(mdaw_midi::settings::read_split_point(&st), 52);
}

/// Review finding 8 (Opus 5.5: 385 of 3,000 random mutations panicked `rusty-leveldb`): a few
/// hundred deterministic single-byte mutations of every file of a profile with tables; the
/// import must never panic, and whatever it reads must be exactly the undamaged content (no
/// silent partial reads).
#[test]
fn mutated_profiles_never_panic_or_read_partially() {
    let prof = compacted_profile();
    let ls = prof.join("Local Storage").join("leveldb");
    let good = {
        let mut v = crate::electron_import::read_local_storage(&ls).unwrap();
        v.sort_by(|a, b| (&a.origin, &a.key).cmp(&(&b.origin, &b.key)));
        v
    };
    let files: Vec<(PathBuf, Vec<u8>)> = std::fs::read_dir(&ls)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let n = p.file_name().unwrap().to_string_lossy().into_owned();
            n.ends_with(".ldb") || n.ends_with(".log") || n.starts_with("MANIFEST-")
        })
        .map(|p| {
            let b = std::fs::read(&p).unwrap();
            (p, b)
        })
        .collect();
    let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let (mut ok, mut err) = (0, 0);
    for _ in 0..300 {
        let (path, orig) = &files[(rnd() % files.len() as u64) as usize];
        if orig.is_empty() {
            continue;
        }
        let mut b = orig.clone();
        let at = (rnd() % b.len() as u64) as usize;
        b[at] ^= 1 << (rnd() % 8);
        std::fs::write(path, &b).unwrap();
        let r = std::panic::catch_unwind(|| crate::electron_import::read_local_storage(&ls));
        std::fs::write(path, orig).unwrap();
        match r.expect("never panics") {
            Ok(mut items) => {
                items.sort_by(|a, b| (&a.origin, &a.key).cmp(&(&b.origin, &b.key)));
                assert_eq!(
                    items,
                    good,
                    "{} byte {at}: read something else without an error",
                    path.display()
                );
                ok += 1;
            }
            Err(_) => err += 1,
        }
    }
    eprintln!("mutations: {ok} read intact, {err} reported as damaged");
    assert!(err > 200, "most mutations are detected");
}
