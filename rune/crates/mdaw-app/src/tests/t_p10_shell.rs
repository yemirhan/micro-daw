//! Phase 10: the Electron settings import, the update check (Settings), the import note.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mdaw_model::settings::{self as store, SettingsStore};
use mdaw_model::types::UpdateStatus;

use super::harness::*;
use crate::app::{App, AppMode};
use crate::electron_import::{IMPORT_MARK_KEY, PACKS_DIR_KEY, import_once, read_local_storage};
use crate::services::RunMode;
use crate::updater::Updater;

/// A profile written by the real Electron (`scripts/electron-profile-fixture`): every
/// micro-daw key on the `file://` origin, a Latin-1 and UTF-16 mix, one overwritten value.
fn fixture_profile() -> PathBuf {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/electron-profile");
    let dst = std::env::temp_dir().join(format!("mdaw-eprofile-{}", uuid::Uuid::new_v4()));
    let ls = dst.join("Local Storage").join("leveldb");
    std::fs::create_dir_all(&ls).unwrap();
    for e in std::fs::read_dir(src.join("Local Storage").join("leveldb")).unwrap() {
        let e = e.unwrap();
        std::fs::copy(e.path(), ls.join(e.file_name())).unwrap();
    }
    dst
}

fn snapshot(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut v: Vec<(String, Vec<u8>)> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            (
                e.file_name().to_string_lossy().into_owned(),
                std::fs::read(e.path()).unwrap(),
            )
        })
        .collect();
    v.sort();
    v
}

#[test]
fn the_electron_profile_is_read_without_touching_it() {
    let profile = fixture_profile();
    let ls = profile.join("Local Storage").join("leveldb");
    let before = snapshot(&ls);
    let items = read_local_storage(&ls).unwrap();
    assert_eq!(snapshot(&ls), before, "the profile is read from a copy");
    let get = |k: &str| {
        items
            .iter()
            .find(|i| i.key == k && i.origin == "file://")
            .map(|i| i.value.as_str())
    };
    assert_eq!(
        get("micro-daw:beat-step-grid"),
        Some("1/8"),
        "the last write wins"
    );
    assert_eq!(get("micro-daw:masterVolume"), Some("-7"));
    assert!(
        get("micro-daw-recent-projects")
            .unwrap()
            .contains("/Users/şule/Müzik/Ritim.mdaw")
    );
    assert_eq!(items.iter().filter(|i| i.origin == "file://").count(), 19);
}

#[test]
fn the_import_copies_every_key_once_and_shares_the_packs_folder() {
    let profile = fixture_profile();
    std::fs::create_dir_all(profile.join("packs").join("lofi-kit")).unwrap();
    let mut st = SettingsStore::in_memory();
    // A value the Rust app already has is kept.
    st.set_item(store::BEAT_PAD_LAYOUT_KEY, "4x4").unwrap();
    let sum = import_once(&mut st, Some(&profile)).expect("imported");
    assert_eq!(sum.origin.as_deref(), Some("file://"));
    assert_eq!(sum.kept, vec![store::BEAT_PAD_LAYOUT_KEY.to_owned()]);
    // Every key but the never-written `micro-daw-recordings` and the kept one.
    assert_eq!(sum.imported.len(), 17, "{:?}", sum.imported);
    assert!(!sum.imported.contains(&"unrelated-key".to_owned()));
    assert_eq!(store::read_beat_pad_layout(&st), "4x4");
    assert_eq!(store::read_beat_step_grid(&st), "1/8");
    assert!(store::read_beat_key_hints(&st));
    assert_eq!(store::read_master_volume(&st), -7.0);
    assert!(!store::read_browser_open(&st));
    assert_eq!(store::read_browser_width(&st), 300.0);
    assert_eq!(
        store::read_browser_roots(&st),
        vec!["/Users/şule/Samples".to_owned()]
    );
    let rec = store::read_recent_projects(&st);
    assert_eq!(rec[0].file_path, "/Users/şule/Müzik/Ritim.mdaw");
    let app = store::read_app_settings(&st);
    assert!(!app.general.auto_check_updates);
    assert_eq!(app.audio.buffer_size_hint, 512);
    let packs = profile.join("packs").to_string_lossy().into_owned();
    assert_eq!(st.get_item(PACKS_DIR_KEY), Some(packs.as_str()));
    assert_eq!(crate::shell::packs_dir_for(&st), PathBuf::from(&packs));
    assert!(
        sum.message()
            .starts_with("Imported 17 settings from the Electron app (1 kept")
    );
    // Once only.
    st.remove_item(store::MASTER_VOLUME_KEY).unwrap();
    assert!(import_once(&mut st, Some(&profile)).is_none());
    assert!(st.get_item(store::MASTER_VOLUME_KEY).is_none());
    let mark: serde_json::Value =
        serde_json::from_str(st.get_item(IMPORT_MARK_KEY).unwrap()).unwrap();
    assert_eq!(mark["status"], "done");
    assert_eq!(mark["imported"].as_array().unwrap().len(), 17);
}

#[test]
fn no_profile_marks_the_import_done_silently() {
    let mut st = SettingsStore::in_memory();
    let missing = std::env::temp_dir().join(format!("mdaw-none-{}", uuid::Uuid::new_v4()));
    assert!(import_once(&mut st, Some(&missing)).is_none());
    assert!(import_once(&mut st, None).is_none());
    let mark: serde_json::Value =
        serde_json::from_str(st.get_item(IMPORT_MARK_KEY).unwrap()).unwrap();
    assert_eq!(mark["status"], "no-profile");
    // A broken LevelDB reports instead of failing the start.
    let mut st = SettingsStore::in_memory();
    let bad = std::env::temp_dir().join(format!("mdaw-bad-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(bad.join("Local Storage").join("leveldb")).unwrap();
    std::fs::write(
        bad.join("Local Storage").join("leveldb").join("CURRENT"),
        "garbage\n",
    )
    .unwrap();
    let sum = import_once(&mut st, Some(&bad)).expect("an error note");
    assert!(sum.error.is_some());
    assert!(sum.message().starts_with("Couldn't import"));
}

#[test]
fn the_imported_settings_reach_the_app_and_the_note_shows_once() {
    let profile = fixture_profile();
    let mut st = SettingsStore::in_memory();
    let note = import_once(&mut st, Some(&profile));
    let mut a = App::with_settings(RunMode::Headless, st);
    a.shell.import_note = note;
    crate::scenes::started(&mut a);
    let mut r = runtime(a);
    // The imported arrangement, recent list and preferences are the app's.
    assert_eq!(r.state.svc.engine.doc().get_bpm(), 97.0);
    assert_eq!(r.state.project.recent.len(), 2);
    assert_eq!(
        r.state.beat.layout,
        mdaw_model::beat_helpers::PadLayout::TwoByEight
    );
    assert!(!r.state.p9.settings.general.auto_check_updates);
    assert!(has(&r, "import-note"));
    click(&mut r, "import-note-dismiss");
    assert!(!has(&r, "import-note"));
}

fn release(tag: &'static str) -> crate::updater::Fetcher {
    Arc::new(move || {
        Ok((
            200,
            format!(
                r#"{{"tag_name":"{tag}","html_url":"https://github.com/yemirhan/micro-daw/releases/tag/{tag}"}}"#
            ),
        ))
    })
}

#[test]
fn check_now_offers_the_release_page() {
    let mut r = rt();
    r.state.shell.updater = Updater::with_fetcher(release("v9.0.0"), true);
    r.state.shell.updater.checked_on_start = true;
    r.state.switch_mode(AppMode::Settings);
    r.invalidate();
    frames(&mut r, 3);
    assert!(!has(&r, "open-release-page"));
    click_settle(&mut r, "check-updates");
    assert_eq!(
        r.state.shell.updater.status,
        UpdateStatus::Available {
            version: "9.0.0".into()
        }
    );
    assert!(has(&r, "open-release-page"));
    click(&mut r, "open-release-page");
    assert_eq!(
        r.take_url_requests(),
        vec!["https://github.com/yemirhan/micro-daw/releases/tag/v9.0.0".to_owned()]
    );
    // Up to date / failure.
    r.state.shell.updater =
        Updater::with_fetcher(release(concat!("v", env!("CARGO_PKG_VERSION"))), true);
    click(&mut r, "check-updates");
    assert_eq!(r.state.shell.updater.status, UpdateStatus::NotAvailable);
    assert!(!has(&r, "open-release-page"));
    r.state.shell.updater = Updater::with_fetcher(Arc::new(|| Err("offline".into())), true);
    click(&mut r, "check-updates");
    assert_eq!(
        r.state.shell.updater.status,
        UpdateStatus::Error {
            message: "Check failed: offline".into()
        }
    );
}

#[test]
fn the_start_up_check_follows_auto_check_updates() {
    // On: one check when the app runs its first frames.
    let mut a = App::headless();
    a.shell.updater = Updater::with_fetcher(release("v9.0.0"), true);
    let r = runtime(a);
    assert!(r.state.shell.updater.checked_on_start);
    assert!(matches!(
        r.state.shell.updater.status,
        UpdateStatus::Available { .. }
    ));
    // Off: no check.
    let mut st = SettingsStore::in_memory();
    store::update_app_settings(
        &mut st,
        &serde_json::json!({"general": {"autoCheckUpdates": false}}),
    )
    .unwrap();
    let mut a = App::with_settings(RunMode::Headless, st);
    a.shell.updater = Updater::with_fetcher(release("v9.0.0"), true);
    let r = runtime(a);
    assert_eq!(r.state.shell.updater.status, UpdateStatus::Idle);
}

/// A real Electron profile (`MDAW_REAL_PROFILE=<userData>`, read through a copy, nothing
/// written): its LevelDB has compacted `.ldb` tables (snappy) besides the log the fixture has.
/// `cargo test -p mdaw-app reads_a_real_profile -- --ignored --nocapture`.
#[test]
#[ignore]
fn reads_a_real_profile() {
    let Some(p) = std::env::var_os("MDAW_REAL_PROFILE") else {
        return;
    };
    let ls = PathBuf::from(p).join("Local Storage").join("leveldb");
    let before = snapshot(&ls);
    let items = read_local_storage(&ls).expect("readable");
    assert_eq!(snapshot(&ls), before);
    let origin = crate::electron_import::pick_origin(&items);
    let keys: Vec<&str> = items
        .iter()
        .filter(|i| Some(&i.origin) == origin.as_ref())
        .map(|i| i.key.as_str())
        .collect();
    eprintln!("origin {origin:?}: {} items: {keys:?}", keys.len());
    let mut st = SettingsStore::in_memory();
    let mut tmp = std::env::temp_dir().join(format!("mdaw-real-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(tmp.join("Local Storage")).unwrap();
    tmp.push("Local Storage");
    std::fs::create_dir_all(tmp.join("leveldb")).unwrap();
    for e in std::fs::read_dir(&ls).unwrap() {
        let e = e.unwrap();
        if e.file_type().unwrap().is_file() {
            std::fs::copy(e.path(), tmp.join("leveldb").join(e.file_name())).unwrap();
        }
    }
    tmp.pop();
    let sum = import_once(&mut st, Some(&tmp)).expect("something imported");
    eprintln!("{}", sum.message());
    assert!(sum.error.is_none());
}
