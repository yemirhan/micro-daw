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
//!   cannot be cancelled in Rune. What the next launch gets back is the recovery checkpoint
//!   (`crate::recovery`): written with every autosave and project transition, validated on
//!   restore; see the phase doc for what is and isn't recovered.
//! - while the window is closed (macOS keeps the app running windowless) MIDI input is
//!   suspended (Electron's renderer, which routes MIDI, is destroyed with its window): nothing
//!   sounds, and the reopened window resumes it.

use mdaw_model::settings::SettingsStore;
use rune::{Key, KeyEvent, Tx};

use crate::app::App;
use crate::electron_import::{ImportSummary, PACKS_DIR_KEY};
use crate::services::RunMode;
use crate::updater::Updater;

pub use crate::recovery::SESSION_KEY;

/// Phase 10's shell state.
#[derive(Default)]
pub struct Shell {
    pub updater: Updater,
    /// The Electron import's note (shown until dismissed).
    pub import_note: Option<ImportSummary>,
    /// The `app-icon` scenes: the view draws the app icon only, on this background.
    pub icon_scene: Option<rune::Color>,
    /// The restored session couldn't be matched to its file: shown until dismissed.
    pub recovery_note: Option<String>,
    /// The window is closed (macOS keeps running windowless): MIDI input is suspended.
    pub window_closed: bool,
    /// The quit's cleanups ran: running pack installs were paused (tests).
    pub quit_paused: bool,
    /// The Electron profile the import reads (kept for "Retry import").
    pub import_profile: Option<std::path::PathBuf>,
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
    let _ = tx;
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
        // From here the settings file is written on its own thread (the autosave's
        // checkpoints never block a frame on the disk).
        store.use_background_writer();
        let mut app = Self::with_settings(RunMode::Window, store);
        app.shell.import_note = note;
        app.shell.import_profile = profile;
        app.shell.updater = Updater::with_fetcher(crate::updater::github_fetcher(), false);
        app.project.handle = handle;
        app.recovery_dir = Some(crate::recovery::recovery_dir_in(&dir));
        app.io = crate::io::IoWorker::new(false);
        app.restore_session();
        app
    }

    /// "Retry import" on the import note: runs the Electron import again (after a failure).
    pub fn retry_electron_import(&mut self) {
        let profile = self.shell.import_profile.clone();
        let note = crate::electron_import::import_retry(
            &mut self.svc.settings.borrow_mut(),
            profile.as_deref(),
        );
        self.shell.import_note = note;
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

    /// Phase 10's frame work: finished project IO, title, update check. True when the view
    /// must re-run.
    pub fn shell_poll(&mut self, tx: &mut Tx) -> bool {
        let mut changed = self.io_poll();
        self.run_queued_op(tx);
        self.sync_title(tx);
        if !self.shell.updater.checked_on_start {
            self.auto_check_updates();
        }
        changed |= self.shell.updater.poll(std::time::Instant::now());
        changed
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

    /// macOS: the Dock icon / Finder reopened the app (`activate`): the guard is armed again
    /// and MIDI input resumes.
    pub fn on_reopen(&mut self) {
        self.project.force_close = false;
        if std::mem::take(&mut self.shell.window_closed) {
            self.svc.with_router(|r, out| r.set_suspended(false, out));
        }
    }

    /// What the window's going does without quitting (`beforeunload` / `pagehide`): a typed
    /// BPM commits, held notes are released, MIDI input is suspended until the window comes
    /// back, the transport stops, the recovery checkpoint is written. Repeatable (the window
    /// may come back).
    pub fn stop_for_close(&mut self) {
        if self.beat.bpm_edit.is_some() {
            self.commit_bpm();
        }
        // Phase 9: typed sample drafts commit, a drag's held settings are written.
        self.flush_sample_drafts();
        self.svc.settings.borrow_mut().set_deferred(false);
        self.release_pad_hold();
        self.release_held_keys();
        self.release_dock_note();
        self.svc.packs.stop_preview(&mut self.svc.engine);
        self.svc.with_router(|r, out| {
            r.all_notes_off(out);
            r.set_suspended(true, out);
        });
        self.shell.window_closed = true;
        if self.playing() {
            self.svc.engine.stop();
        }
        self.flush_autosave();
    }
}
