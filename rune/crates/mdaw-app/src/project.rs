//! Phase 10: the project service (`services/ProjectManager.ts` + `hooks/useProject.ts`) and the
//! window's close guard (`main.ts` close / `project:before-close` / `project:force-close`).
//! See `docs/rune-port/phase-10-app-shell.md`.
//!
//! Flows (same order and texts as the TS):
//! - **New** (⌘N, sidebar, palette): unsaved prompt → save dialog ("Untitled.mdaw") → starter
//!   project, undo cleared, master volume / live FX reset → written at once.
//! - **New from template**: the same with the template's arrangement (fresh ids).
//! - **Open** (⌘O, sidebar, palette) / **open recent** / a dropped `.mdaw`: unsaved prompt →
//!   (open dialog) → `loadFromFile` through [`mdaw_model::open::open_project`] (migrations +
//!   coercion); its warnings / a failure show in the load report dialog (Electron only logs them).
//! - **Save** (⌘S, the top bar's project button): the current path, else **Save As** (⇧⌘S):
//!   save dialog (`<name>.mdaw`), `createdAt` = now, written.
//! - **Close** (window close button / ⌘W) and **Quit**: see [`App::request_close`].
//!
//! The unsaved prompt (`project:show-unsaved-dialog`) is drawn by the app (Electron's is a
//! native sheet; Rune has no message boxes): Save / Don't Save / Cancel, Enter = Save,
//! Escape = Cancel. "Save" that ends without a file (save dialog cancelled, write failed)
//! cancels the operation, as `promptUnsaved` returns `save()`'s result.

use mdaw_model::migrate::{build_project_file, project_file_to_json, project_name, window_title};
use mdaw_model::settings as store;
use mdaw_model::types::ProjectMeta;
use rune::{FileDialogOptions, FileDialogResult, Tx};

use crate::app::App;

/// What runs once the unsaved prompt (if any) let it through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectOp {
    New,
    Open,
    OpenPath(String),
    /// New from the template at this index of [`crate::views::templates`].
    Template(usize),
    /// The window closes (`project:force-close`).
    Close,
}

/// The load report dialog: a failed open, or an open that had to migrate / coerce / skip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadReport {
    pub path: String,
    /// `Failed to load project: …` (the project stays as it was).
    pub error: Option<String>,
    pub warnings: Vec<String>,
}

/// The project state of `ProjectManager` + `useProject`.
#[derive(Default)]
pub struct ProjectUi {
    /// `filePath` (None = "Untitled").
    pub file_path: Option<String>,
    /// `createdAt` (ISO).
    pub created_at: String,
    /// `micro-daw-recent-projects`.
    pub recent: Vec<ProjectMeta>,
    /// The unsaved prompt is open; the operation it guards.
    pub unsaved: Option<ProjectOp>,
    /// The load report dialog.
    pub report: Option<LoadReport>,
    /// The last title set on the window.
    pub title: String,
    /// `forceCloseAllowed`: the next close goes through without the guard.
    pub force_close: bool,
    /// A save failed (shown in the report dialog's place as an error).
    pub write_error: Option<String>,
    /// The window app's handle (reopens the window the close guard kept).
    pub handle: Option<rune::AppHandle<App>>,
    /// Number of project files written (tests).
    pub writes: u64,
    /// Number of closes the guard let through (tests).
    pub window_closes: u64,
    /// `modifiedAt` of the file at [`ProjectUi::file_path`] as this app last read or wrote it
    /// (the recovery checkpoint checks the file still has it before reattaching the path).
    pub file_modified_at: Option<String>,
    /// A project read / write is running on the IO worker (new project operations wait).
    pub busy: bool,
    /// The outcome of the last write / load (the inline worker's synchronous answer).
    pub last_write_ok: Option<bool>,
    pub last_load_ok: Option<bool>,
    /// An operation a finished save lets through (run with the next `Tx`).
    pub queued_op: Option<ProjectOp>,
    /// Bumped by every load: sample decode failures of an older load are not reported.
    pub load_gen: u64,
    /// Manifest samples of the current load whose decode hasn't finished: id → (name, path).
    pub pending_samples: std::collections::HashMap<String, (String, String)>,
}

/// What a finished project write does next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AfterWrite {
    /// Save (⌘S, a recovery-free save).
    Plain,
    /// Save from the unsaved prompt: the guarded operation runs after a successful save.
    Then(ProjectOp),
    /// New / New from template: the path is the project's only once the file exists.
    New,
}

/// `new Date().toISOString()`.
pub fn iso_now() -> String {
    mdaw_packs::json::iso_now()
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn mdaw_options(title: &str, file_name: Option<String>) -> FileDialogOptions {
    FileDialogOptions {
        title: title.to_owned(),
        directory: None,
        file_name,
        filters: vec![("Micro DAW Project".into(), vec!["mdaw".into()])],
    }
}

impl ProjectUi {
    pub fn new(settings: &store::SettingsStore) -> Self {
        Self {
            created_at: iso_now(),
            recent: store::read_recent_projects(settings),
            title: window_title(None, false),
            ..Default::default()
        }
    }
}

impl App {
    /// `getProjectName()`.
    pub fn project_name(&self) -> String {
        project_name(self.project.file_path.as_deref())
    }

    /// `updateWindowTitle()`: `<name>[ *] - Micro DAW`, set when it changes.
    pub fn sync_title(&mut self, tx: &mut Tx) {
        let title = window_title(self.project.file_path.as_deref(), self.dirty());
        if title != self.project.title {
            self.project.title = title.clone();
            tx.set_title(title);
        }
    }

    /// The doc and audio config are clean from here (`isDirty = false`).
    pub(crate) fn mark_clean(&mut self) {
        self.clean_version = self.svc.engine.doc().state_version();
    }

    // --- entry points -----------------------------------------------------------------------

    /// Runs `op` behind the unsaved prompt (`promptUnsaved`).
    pub fn request_project_op(&mut self, op: ProjectOp, tx: &mut Tx) {
        if self.project.unsaved.is_some() || self.project.busy {
            return;
        }
        if self.dirty() {
            self.close_menu();
            self.close_dialog();
            self.project.unsaved = Some(op);
        } else {
            self.continue_project_op(op, tx);
        }
    }

    /// The unsaved prompt's answer: `save`, `dont-save` (`Some(false)`) or `cancel` (`None`).
    pub fn answer_unsaved(&mut self, save: Option<bool>, tx: &mut Tx) {
        let Some(op) = self.project.unsaved.take() else {
            return;
        };
        match save {
            None => {
                // Cancel: a close stays open, the guard re-arms.
                self.project.force_close = false;
            }
            Some(false) => self.continue_project_op(op, tx),
            Some(true) => self.save_then(Some(op), tx),
        }
    }

    fn continue_project_op(&mut self, op: ProjectOp, tx: &mut Tx) {
        match op {
            ProjectOp::New => self.pick_new_path(None, tx),
            ProjectOp::Template(i) => self.pick_new_path(Some(i), tx),
            ProjectOp::Open => {
                tx.open_file(
                    mdaw_options("Open Project", None),
                    |s: &mut App, r: FileDialogResult, _tx: &mut Tx| {
                        if let Ok(Some(p)) = r {
                            s.load_project_file(&p.to_string_lossy());
                        }
                    },
                );
            }
            ProjectOp::OpenPath(p) => {
                self.load_project_file(&p);
            }
            ProjectOp::Close => self.force_close(tx),
        }
    }

    /// `save()`: the current path, else Save As. `then` runs after a successful save.
    pub fn save_project(&mut self, tx: &mut Tx) {
        self.save_then(None, tx);
    }

    fn save_then(&mut self, then: Option<ProjectOp>, tx: &mut Tx) {
        if self.project.busy {
            return;
        }
        match self.project.file_path.clone() {
            Some(path) => {
                let after = then.map_or(AfterWrite::Plain, AfterWrite::Then);
                let created = self.project.created_at.clone();
                self.submit_write(&path, created, after);
                self.run_queued_op(tx);
            }
            None => self.save_as_then(then, tx),
        }
    }

    /// `saveAs()`.
    pub fn save_project_as(&mut self, tx: &mut Tx) {
        self.save_as_then(None, tx);
    }

    fn save_as_then(&mut self, then: Option<ProjectOp>, tx: &mut Tx) {
        if self.project.busy {
            return;
        }
        let name = format!("{}.mdaw", self.project_name());
        tx.save_file(
            mdaw_options("Save Project", Some(name)),
            move |s: &mut App, r: FileDialogResult, tx: &mut Tx| {
                let Ok(Some(p)) = r else {
                    // Cancelled: a guarded close stays open.
                    s.project.force_close = false;
                    return;
                };
                // The path and the new `createdAt` become the project's only once the file is
                // written (a failed write keeps the old path, so the title stays honest).
                let path = p.to_string_lossy().into_owned();
                let after = then.map_or(AfterWrite::Plain, AfterWrite::Then);
                s.submit_write(&path, iso_now(), after);
                s.run_queued_op(tx);
            },
        );
    }

    /// New project / template: the save dialog first ("Untitled.mdaw"), then the reset.
    fn pick_new_path(&mut self, template: Option<usize>, tx: &mut Tx) {
        tx.save_file(
            mdaw_options("Save Project", Some("Untitled.mdaw".into())),
            move |s: &mut App, r: FileDialogResult, _tx: &mut Tx| {
                if let Ok(Some(p)) = r {
                    s.new_project_at(&p.to_string_lossy(), template);
                }
            },
        );
    }

    /// `newProject()` / `newProjectFromTemplate()` after the path was chosen. The new project is
    /// untitled and unsaved until its file exists (review finding 11: Electron took the path and
    /// marked it clean before writing, so a failed write left a clean project pointing at a file
    /// that was never made). False when the write failed (headless; a window run answers later).
    pub fn new_project_at(&mut self, path: &str, template: Option<usize>) -> bool {
        if self.project.busy {
            return false;
        }
        self.project.created_at = iso_now();
        match template.and_then(|i| crate::views::templates().get(i)) {
            Some(t) => {
                let arr = t.arrangement.clone();
                let _ = self.svc.engine.edit(|d| d.load_from_template(&arr));
            }
            None => self.svc.engine.edit(|d| d.reset_to_starter()),
        }
        self.svc.engine.edit(|d| d.clear_history());
        self.reset_audio_config();
        self.after_project_change();
        self.project.file_path = None;
        self.project.file_modified_at = None;
        // Unsaved until the write lands.
        self.clean_version = u64::MAX;
        self.close_dialog();
        let created = self.project.created_at.clone();
        self.submit_write(path, created, AfterWrite::New)
    }

    /// The audio-config reset of New / New from template: master volume `MASTER_REFERENCE_DB`
    /// (persisted, like `setMasterVolumeDb`), reverb 0, chorus 0, cutoff 18000, resonance 1.
    fn reset_audio_config(&mut self) {
        use mdaw_engine::fx::live::LiveOp;
        self.set_master_volume_persisted(mdaw_engine::engine::MASTER_REFERENCE_DB);
        for op in [
            LiveOp::ReverbWet(0.0),
            LiveOp::ChorusDepth(0.0),
            LiveOp::FilterCutoff(18000.0),
            LiveOp::FilterResonance(1.0),
        ] {
            self.svc.engine.live_fx(op);
        }
    }

    pub(crate) fn set_master_volume_persisted(&mut self, db: f64) {
        self.svc.engine.set_master_volume_db(db);
        let v = self.svc.engine.get_master_volume_db();
        let _ = store::write_master_volume(&mut self.svc.settings.borrow_mut(), v);
    }

    /// UI state that follows a new arrangement (selection, editors, step focus).
    fn after_project_change(&mut self) {
        self.close_menu();
        self.song.editing = None;
        self.beat.step_focus.pad = self.svc.engine.doc().get_selected_pad();
        self.svc.with_router(|r, out| r.all_notes_off(out));
    }

    /// `writeToFile(path)` with the current `createdAt`. Headless: true when the file was
    /// written. A window run writes on the IO worker and answers true (started); the result
    /// arrives with a later frame.
    pub fn write_project(&mut self, path: &str) -> bool {
        let created = self.project.created_at.clone();
        self.submit_write(path, created, AfterWrite::Plain)
    }

    /// `writeToFile`: the project JSON is built now (an immutable snapshot of the doc version
    /// it was built from) and written on the IO worker; [`App::finish_write`] applies the
    /// result. Returns the inline result when the worker is inline, else true (submitted).
    pub fn submit_write(&mut self, path: &str, created_at: String, after: AfterWrite) -> bool {
        let now = iso_now();
        let version = self.svc.engine.doc().state_version();
        let arrangement = self.svc.engine.doc().get_arrangement().clone();
        let samples = &self.p9.samples;
        let file = build_project_file(
            arrangement,
            self.svc.engine.get_master_volume_db(),
            self.svc.engine.live_effect_params(),
            &created_at,
            &now,
            &|id| samples.get(id).map(|r| r.name.clone()),
        );
        let json = project_file_to_json(&file);
        self.project.busy = true;
        self.project.last_write_ok = None;
        let path = path.to_owned();
        self.io_submit(move || {
            let r =
                mdaw_model::fsio::write_atomic(&path, json.as_bytes()).map_err(|e| e.to_string());
            Box::new(move |s: &mut App| s.finish_write(path, created_at, now, version, after, r))
        });
        if self.io.is_sync() {
            self.project.last_write_ok.unwrap_or(false)
        } else {
            true
        }
    }

    /// A project write finished. Success: the path, `createdAt` and `modifiedAt` are the
    /// project's, it is clean **as of the version written** (an edit made while the write ran
    /// keeps it dirty), the recent list and the recovery checkpoint follow, and a guarded
    /// operation runs. Failure: nothing changes but the error dialog (New stays untitled and
    /// unsaved; a guarded close stays open).
    fn finish_write(
        &mut self,
        path: String,
        created_at: String,
        modified_at: String,
        version: u64,
        after: AfterWrite,
        r: Result<(), String>,
    ) {
        self.project.busy = false;
        if let Err(e) = r {
            self.project.write_error = Some(format!("Could not save \"{path}\": {e}"));
            self.project.last_write_ok = Some(false);
            self.project.force_close = false;
            self.checkpoint_now();
            return;
        }
        self.project.writes += 1;
        self.project.write_error = None;
        self.project.last_write_ok = Some(true);
        self.project.file_path = Some(path.clone());
        self.project.created_at = created_at;
        self.project.file_modified_at = Some(modified_at.clone());
        self.clean_version = version;
        self.add_recent(&path, &modified_at);
        self.checkpoint_now();
        if let AfterWrite::Then(op) = after {
            self.project.queued_op = Some(op);
        }
    }

    /// Runs the operation a finished save let through (needs a `Tx`: a dialog, the close).
    pub fn run_queued_op(&mut self, tx: &mut Tx) {
        if let Some(op) = self.project.queued_op.take() {
            self.continue_project_op(op, tx);
        }
    }

    fn add_recent(&mut self, path: &str, modified_at: &str) {
        self.project.recent = store::add_recent_project(&self.project.recent, path, modified_at);
        let _ =
            store::write_recent_projects(&mut self.svc.settings.borrow_mut(), &self.project.recent);
    }

    /// `loadFromFile(path)`: the file is read and parsed (migrations + coercion) on the IO
    /// worker, then applied here ([`App::finish_load`]). Headless: false when it failed (the
    /// report dialog says why; the project is unchanged). A window run answers true (started).
    pub fn load_project_file(&mut self, path: &str) -> bool {
        if self.project.busy {
            return false;
        }
        let packs_dir = self.svc.packs_dir.clone();
        self.project.busy = true;
        self.project.last_load_ok = None;
        let path = path.to_owned();
        self.io_submit(move || {
            let r = read_project(&path, &packs_dir);
            Box::new(move |s: &mut App| s.finish_load(path, r))
        });
        if self.io.is_sync() {
            self.project.last_load_ok.unwrap_or(false)
        } else {
            true
        }
    }

    fn finish_load(&mut self, path: String, r: Result<ReadProject, String>) {
        self.project.busy = false;
        let read = match r {
            Ok(o) => o,
            Err(e) => {
                self.project.report = Some(LoadReport {
                    path,
                    error: Some(e),
                    warnings: Vec::new(),
                });
                self.project.last_load_ok = Some(false);
                return;
            }
        };
        let opened = read.opened;
        let mut warnings: Vec<String> = Vec::new();
        warnings.extend(opened.version_warning.clone());
        // Samples of the manifest: loaded by id from their path (the regions load them too).
        // A file that exists but can't be decoded is reported when its decode fails
        // (`pending_samples`, review finding 16).
        self.project.load_gen += 1;
        self.project.pending_samples.clear();
        let mut manifest = Vec::new();
        for (m, exists) in opened.sample_manifest.iter().zip(&read.exists) {
            let (Some(id), Some(p)) = (m.sample_id.clone(), m.path.clone()) else {
                continue;
            };
            let name = m.name.clone().unwrap_or_default();
            if *exists {
                self.project
                    .pending_samples
                    .insert(id.clone(), (name, p.clone()));
            } else {
                warnings.push(format!("could not load sample \"{name}\" from {p}"));
            }
            manifest.push((id, p));
        }
        let now = self.now;
        self.p9.samples.ensure_region_samples(&manifest, now);
        // Kit preload: the pad buffers load lazily per pad (`PadBuffers`); missing files keep
        // the pad (name + path), silent, as in the TS.
        self.svc
            .engine
            .edit(|d| d.load_project(&opened.arrangement));
        self.svc.engine.edit(|d| d.clear_history());
        if let Some(a) = opened.audio {
            use mdaw_engine::fx::live::LiveOp;
            if let Some(v) = a.master_volume_db.filter(|v| v.is_finite()) {
                self.set_master_volume_persisted(v);
            }
            // `audioEngine.setX(fx.x)`: a non-number is passed through by the TS (a NaN param);
            // the Rust setters take numbers, so those are skipped (and reported).
            let fx = [
                (
                    "reverbWet",
                    a.reverb_wet,
                    LiveOp::ReverbWet as fn(f64) -> LiveOp,
                ),
                ("chorusDepth", a.chorus_depth, LiveOp::ChorusDepth),
                ("filterCutoff", a.filter_cutoff, LiveOp::FilterCutoff),
                (
                    "filterResonance",
                    a.filter_resonance,
                    LiveOp::FilterResonance,
                ),
            ];
            for (name, v, op) in fx {
                match v {
                    Some(v) => self.svc.engine.live_fx(op(v)),
                    None => warnings.push(format!(
                        "audioConfig.effectParams.{name}: not a number, not applied"
                    )),
                }
            }
        }
        warnings.extend(opened.dropped_drum_notes.iter().map(|d| d.message()));
        warnings.extend(opened.warnings.iter().map(|w| w.to_string()));
        self.after_project_change();
        self.project.file_path = Some(path.clone());
        self.project.created_at = opened.created_at.clone().unwrap_or_else(iso_now);
        self.project.file_modified_at = opened.modified_at.clone();
        self.mark_clean();
        // `addToRecentProjects(filePath, data.modifiedAt)` (`undefined` is dropped by
        // `JSON.stringify`; we store "").
        let modified = opened.modified_at.clone().unwrap_or_default();
        self.add_recent(&path, &modified);
        self.close_dialog();
        self.project.report = (!warnings.is_empty()).then(|| LoadReport {
            path: path.clone(),
            error: None,
            warnings,
        });
        self.project.last_load_ok = Some(true);
        self.checkpoint_now();
    }

    /// Sample decodes that finished since the last frame: a manifest sample of the current
    /// load that failed to decode joins the load report (review finding 16).
    pub fn report_sample_decodes(&mut self, loaded: &[String], failed: Vec<(String, String)>) {
        for id in loaded {
            self.project.pending_samples.remove(id);
        }
        for (id, e) in failed {
            let Some((name, path)) = self.project.pending_samples.remove(&id) else {
                continue;
            };
            let line = format!("could not load sample \"{name}\" from {path}: {e}");
            match &mut self.project.report {
                Some(r) if r.error.is_none() => r.warnings.push(line),
                _ => {
                    self.project.report = Some(LoadReport {
                        path: self.project.file_path.clone().unwrap_or_default(),
                        error: None,
                        warnings: vec![line],
                    })
                }
            }
        }
    }

    // --- close guard ------------------------------------------------------------------------

    /// The window's close button / ⌘W (`mainWindow.on('close')` + `handleBeforeClose`):
    /// a clean project (or a close already allowed) closes; a dirty one shows the unsaved
    /// prompt with [`ProjectOp::Close`] and the close is held. True when the window may go.
    pub fn close_requested(&mut self) -> bool {
        if self.project.force_close || !self.dirty() {
            return true;
        }
        if self.project.unsaved.is_none() {
            self.close_menu();
            self.close_dialog();
            self.project.unsaved = Some(ProjectOp::Close);
        }
        false
    }

    /// `project:force-close`: the next close is allowed, then the window closes.
    fn force_close(&mut self, tx: &mut Tx) {
        self.project.force_close = true;
        tx.close_window();
    }
}

/// What the IO worker reads for a load: the parsed project and, per manifest entry, whether
/// its file exists.
pub struct ReadProject {
    pub opened: mdaw_model::open::OpenedProject,
    pub exists: Vec<bool>,
}

/// Reads and parses a project file (the IO worker's half of `loadFromFile`).
pub fn read_project(path: &str, packs_dir: &str) -> Result<ReadProject, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| format!("Failed to load project: {e}"))?;
    let opened = mdaw_model::open::open_project(&raw, Some(packs_dir), &mut new_id)
        .map_err(|e| e.to_string())?;
    let exists = opened
        .sample_manifest
        .iter()
        .map(|m| {
            m.path
                .as_deref()
                .is_some_and(|p| std::path::Path::new(p).is_file())
        })
        .collect();
    Ok(ReadProject { opened, exists })
}

/// Writes `json` to `path` durably ([`mdaw_model::fsio::write_atomic`]).
pub fn write_atomic(path: &str, json: &str) -> std::io::Result<()> {
    mdaw_model::fsio::write_atomic(path, json.as_bytes())
}
