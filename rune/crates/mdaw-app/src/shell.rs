//! Phase 10: the app shell around the project service (`crate::project`): the window's close
//! guard and quit, the session the next launch restores, the update check, the Electron settings
//! import note, and the keys of the phase-10 dialogs. See `docs/rune-port/phase-10-app-shell.md`.
//!
//! **Close and quit** (Electron: `mainWindow.on('close')` → `project:before-close` → unsaved
//! dialog → `project:force-close`; `window-all-closed` quits except on macOS; `will-quit` pauses
//! pack installs). Rune cannot veto a close, so the window app runs with `keep_running`:
//! - the close button / ⌘W / Alt-F4 run [`App::on_window_close`]: a clean project closes (the
//!   app quits, except on macOS, where it keeps running windowless like Electron's); a dirty one
//!   shows the window again with the unsaved prompt, whose Save / Don't Save close it and whose
//!   Cancel keeps it;
//! - a quit (macOS ⌘Q, the Dock, logout; a Windows session end) runs [`App::shutdown`] and
//!   cannot be cancelled in Rune. The working arrangement is already autosaved, and the session
//!   ([`SESSION_KEY`]: file path, `createdAt`, dirty) is written with it, so the next launch
//!   reopens the project with its unsaved changes and its `*` (see the phase doc).

use mdaw_model::settings::SettingsStore;
use rune::{Key, KeyEvent, Tx};
use serde_json::{Value, json};

use crate::app::App;
use crate::electron_import::{ImportSummary, PACKS_DIR_KEY};
use crate::services::RunMode;
use crate::updater::Updater;

/// The project of the last session: `{filePath, createdAt, dirty}` (Rust app only).
pub const SESSION_KEY: &str = "micro-daw-rune:session";

/// Phase 10's shell state.
#[derive(Default)]
pub struct Shell {
    pub updater: Updater,
    /// The Electron import's note (shown until dismissed).
    pub import_note: Option<ImportSummary>,
    /// The `app-icon` scenes: the view draws the app icon only, on this background.
    pub icon_scene: Option<rune::Color>,
}

/// The packs folder of a window run: the shared Electron one when imported, else
/// `<data dir>/packs`.
pub fn packs_dir_for(settings: &SettingsStore) -> std::path::PathBuf {
    settings
        .get_item(PACKS_DIR_KEY)
        .filter(|p| !p.is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| crate::services::data_dir().join("packs"))
}

/// The phase-10 dialogs own the keyboard: the unsaved prompt (Enter = Save, the default
/// button; Escape = Cancel) and the load report (Enter / Escape close it).
pub fn dialog_keys(s: &mut App, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if s.project.unsaved.is_some() {
        match ev.key {
            Key::Enter => s.answer_unsaved(Some(true), tx),
            Key::Escape => s.answer_unsaved(None, tx),
            _ => {}
        }
        return true;
    }
    if s.project.report.is_some() || s.project.write_error.is_some() {
        if matches!(ev.key, Key::Enter | Key::Escape) {
            s.project.report = None;
            s.project.write_error = None;
        }
        return true;
    }
    false
}

impl App {
    /// The window app with phase 10's start: the Electron import first (its values are what the
    /// services read), the update fetcher, then the last session's project.
    pub fn window_with(handle: Option<rune::AppHandle<App>>) -> Self {
        let dir = crate::services::data_dir();
        let _ = std::fs::create_dir_all(&dir);
        let mut store = SettingsStore::open(&dir);
        let profile = crate::electron_import::electron_profile_dir();
        let note = crate::electron_import::import_once(&mut store, profile.as_deref());
        let mut app = Self::with_settings(RunMode::Window, store);
        app.shell.import_note = note;
        app.shell.updater = Updater::with_fetcher(crate::updater::github_fetcher(), false);
        app.project.handle = handle;
        app.restore_session();
        app
    }

    /// `autoCheckUpdates`: the start-up check (`useAutoUpdater` on mount).
    pub fn auto_check_updates(&mut self) {
        if self.shell.updater.checked_on_start {
            return;
        }
        self.shell.updater.checked_on_start = true;
        if self.p9.settings.general.auto_check_updates && std::env::var_os("MDAW_SMOKE").is_none() {
            self.shell.updater.check();
        }
    }

    /// "Open release page" (the Rust stand-in for "Download & install").
    pub fn open_release_page(&mut self, tx: &mut Tx) {
        let url = self
            .shell
            .updater
            .page
            .clone()
            .unwrap_or_else(|| crate::updater::RELEASES_PAGE.to_owned());
        let _ = tx.open_url(&url);
    }

    /// Phase 10's frame work: title, update check. True when the view must re-run.
    pub fn shell_poll(&mut self, tx: &mut Tx) -> bool {
        self.sync_title(tx);
        if !self.shell.updater.checked_on_start {
            self.auto_check_updates();
        }
        self.shell.updater.poll()
    }

    // --- session ----------------------------------------------------------------------------

    /// Writes the session (path, `createdAt`, dirty) next to the autosaved arrangement.
    pub fn save_session(&mut self) {
        let v = json!({
            "filePath": self.project.file_path,
            "createdAt": self.project.created_at,
            "dirty": self.dirty(),
        });
        let _ = self
            .svc
            .settings
            .borrow_mut()
            .set_item(SESSION_KEY, v.to_string());
    }

    /// The last session's project, when its arrangement came back from the autosave: the path
    /// and `createdAt`, and dirty again if it was (the title shows `*`, ⌘S saves it there).
    pub fn restore_session(&mut self) {
        if !self.svc.restored_autosave {
            return;
        }
        let raw = self
            .svc
            .settings
            .borrow()
            .get_item(SESSION_KEY)
            .map(str::to_owned);
        let Some(v) = raw.and_then(|r| serde_json::from_str::<Value>(&r).ok()) else {
            return;
        };
        let Some(path) = v.get("filePath").and_then(Value::as_str) else {
            return;
        };
        self.project.file_path = Some(path.to_owned());
        if let Some(c) = v.get("createdAt").and_then(Value::as_str) {
            self.project.created_at = c.to_owned();
        }
        if v.get("dirty").and_then(Value::as_bool) == Some(true) {
            // No load or save since: dirty until the next one.
            self.clean_version = u64::MAX;
        }
    }

    // --- close / quit -----------------------------------------------------------------------

    /// The window's close (`App::on_close`): see the module docs.
    pub fn on_window_close(&mut self, tx: &mut Tx) {
        if rune::quitting() {
            // macOS closes the windows after the quit hooks ran.
            self.shutdown();
            return;
        }
        if self.close_requested() {
            self.project.force_close = false;
            self.project.window_closes += 1;
            self.stop_for_close();
            // `window-all-closed`: quit unless macOS.
            if !cfg!(target_os = "macos") {
                tx.quit();
            }
        } else if let Some(h) = &self.project.handle {
            // The close is held: the window comes back with the prompt.
            h.open();
        }
    }

    /// macOS: the Dock icon / Finder reopened the app (`activate`): the guard is armed again.
    pub fn on_reopen(&mut self) {
        self.project.force_close = false;
    }

    /// What the window's going does without quitting (`beforeunload` / `pagehide`): a typed
    /// BPM commits, held notes are released, the transport stops, the autosave and session are
    /// written. Repeatable (the window may come back).
    pub fn stop_for_close(&mut self) {
        if self.beat.bpm_edit.is_some() {
            self.commit_bpm();
        }
        self.release_pad_hold();
        self.release_held_keys();
        self.release_dock_note();
        self.svc.packs.stop_preview(&mut self.svc.engine);
        self.svc.with_router(|r, out| r.all_notes_off(out));
        if self.playing() {
            self.svc.engine.stop();
        }
        self.svc.flush_autosave();
        if self.svc.mode == RunMode::Window || self.svc.save_headless {
            self.save_session();
        }
    }
}
