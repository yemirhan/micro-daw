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
    fn mark_clean(&mut self) {
        self.clean_version = self.svc.engine.doc().state_version();
    }

    // --- entry points -----------------------------------------------------------------------

    /// Runs `op` behind the unsaved prompt (`promptUnsaved`).
    pub fn request_project_op(&mut self, op: ProjectOp, tx: &mut Tx) {
        if self.project.unsaved.is_some() {
            return;
        }
        if self.dirty() {
            self.close_menu();
            self.p9.dialog = None;
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
        match self.project.file_path.clone() {
            Some(path) => {
                if self.write_project(&path)
                    && let Some(op) = then
                {
                    self.continue_project_op(op, tx);
                }
            }
            None => self.save_as_then(then, tx),
        }
    }

    /// `saveAs()`.
    pub fn save_project_as(&mut self, tx: &mut Tx) {
        self.save_as_then(None, tx);
    }

    fn save_as_then(&mut self, then: Option<ProjectOp>, tx: &mut Tx) {
        let name = format!("{}.mdaw", self.project_name());
        tx.save_file(
            mdaw_options("Save Project", Some(name)),
            move |s: &mut App, r: FileDialogResult, tx: &mut Tx| {
                let Ok(Some(p)) = r else {
                    // Cancelled: a guarded close stays open.
                    s.project.force_close = false;
                    return;
                };
                let path = p.to_string_lossy().into_owned();
                let previous = (s.project.file_path.clone(), s.project.created_at.clone());
                s.project.file_path = Some(path.clone());
                s.project.created_at = iso_now();
                if s.write_project(&path) {
                    if let Some(op) = then {
                        s.continue_project_op(op, tx);
                    }
                } else {
                    // A failed write keeps the old file (Electron's write rejects before the
                    // path sticks only in effect: we restore it so the title stays honest).
                    (s.project.file_path, s.project.created_at) = previous;
                }
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

    /// `newProject()` / `newProjectFromTemplate()` after the path was chosen.
    pub fn new_project_at(&mut self, path: &str, template: Option<usize>) {
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
        self.project.file_path = Some(path.to_owned());
        self.mark_clean();
        self.write_project(path);
        self.p9.dialog = None;
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

    fn set_master_volume_persisted(&mut self, db: f64) {
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

    /// `writeToFile(path)`: the project JSON, then clean, recent list, title. False when the
    /// write failed (the error shows in the report dialog; Electron's IPC rejects unhandled).
    pub fn write_project(&mut self, path: &str) -> bool {
        let now = iso_now();
        let arrangement = self.svc.engine.doc().get_arrangement().clone();
        let samples = &self.p9.samples;
        let file = build_project_file(
            arrangement,
            self.svc.engine.get_master_volume_db(),
            self.svc.engine.live_effect_params(),
            &self.project.created_at,
            &now,
            &|id| samples.get(id).map(|r| r.name.clone()),
        );
        let json = project_file_to_json(&file);
        if let Err(e) = write_atomic(path, &json) {
            self.project.write_error = Some(format!("Could not save \"{path}\": {e}"));
            return false;
        }
        self.project.writes += 1;
        self.project.write_error = None;
        self.mark_clean();
        self.add_recent(path, &now);
        true
    }

    fn add_recent(&mut self, path: &str, modified_at: &str) {
        self.project.recent = store::add_recent_project(&self.project.recent, path, modified_at);
        let _ =
            store::write_recent_projects(&mut self.svc.settings.borrow_mut(), &self.project.recent);
    }

    /// `loadFromFile(path)`. False when it failed (the report dialog says why; the project is
    /// unchanged).
    pub fn load_project_file(&mut self, path: &str) -> bool {
        let raw = match std::fs::read_to_string(path) {
            Ok(r) => r,
            Err(e) => {
                self.project.report = Some(LoadReport {
                    path: path.to_owned(),
                    error: Some(format!("Failed to load project: {e}")),
                    warnings: Vec::new(),
                });
                return false;
            }
        };
        let packs_dir = self.svc.packs_dir.clone();
        let opened = match mdaw_model::open::open_project(&raw, Some(&packs_dir), &mut new_id) {
            Ok(o) => o,
            Err(e) => {
                self.project.report = Some(LoadReport {
                    path: path.to_owned(),
                    error: Some(e.to_string()),
                    warnings: Vec::new(),
                });
                return false;
            }
        };
        let mut warnings: Vec<String> = Vec::new();
        warnings.extend(opened.version_warning.clone());
        // Samples of the manifest: loaded by id from their path (the regions load them too).
        let mut manifest = Vec::new();
        for m in &opened.sample_manifest {
            let (Some(id), Some(p)) = (m.sample_id.clone(), m.path.clone()) else {
                continue;
            };
            if !std::path::Path::new(&p).is_file() {
                warnings.push(format!(
                    "could not load sample \"{}\" from {p}",
                    m.name.clone().unwrap_or_default()
                ));
            }
            manifest.push((id, p));
        }
        self.p9.samples.ensure_region_samples(&manifest);
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
        self.project.file_path = Some(path.to_owned());
        self.project.created_at = opened.created_at.clone().unwrap_or_else(iso_now);
        self.mark_clean();
        // `addToRecentProjects(filePath, data.modifiedAt)` (`undefined` is dropped by
        // `JSON.stringify`; we store "").
        let modified = opened.modified_at.clone().unwrap_or_default();
        self.add_recent(path, &modified);
        self.p9.dialog = None;
        if !warnings.is_empty() {
            self.project.report = Some(LoadReport {
                path: path.to_owned(),
                error: None,
                warnings,
            });
        }
        true
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
            self.p9.dialog = None;
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

/// Writes `json` to `path` through a temp file + rename (Electron's `fs.writeFile` writes in
/// place; the rename keeps the old file intact when the disk fills up mid-write).
pub fn write_atomic(path: &str, json: &str) -> std::io::Result<()> {
    let p = std::path::Path::new(path);
    let dir = p.parent().filter(|d| !d.as_os_str().is_empty());
    let tmp = match dir {
        Some(d) => d.join(format!(".{}.tmp-{}", file_name(p), std::process::id())),
        None => std::path::PathBuf::from(format!(".mdaw.tmp-{}", std::process::id())),
    };
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, p).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

fn file_name(p: &std::path::Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}
