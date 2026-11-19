//! Phase 10's screenshot scenes (`scripts/parity-phase-10/run.sh`): the unsaved prompt, the
//! load report, the recent projects (sidebar, palette), the update row, the import note, and
//! the app icon the bundle script renders.

use mdaw_model::types::{ProjectMeta, UpdateStatus};

use crate::app::{App, AppMode};
use crate::project::{LoadReport, ProjectOp};
use crate::scenes::started;

fn recent() -> Vec<ProjectMeta> {
    [
        "Night Drive",
        "Boom Bap Sketch",
        "Lo-Fi Study",
        "Untitled 3",
    ]
    .iter()
    .enumerate()
    .map(|(i, n)| ProjectMeta {
        file_path: format!("/Users/you/Music/Micro DAW/{n}.mdaw"),
        name: (*n).to_owned(),
        modified_at: format!("2026-11-1{i}T10:00:00.000Z"),
    })
    .collect()
}

/// A dirty "Night Drive" with the unsaved prompt (the close guard's).
pub fn unsaved(s: &mut App) {
    started(s);
    s.project.file_path = Some("/Users/you/Music/Micro DAW/Night Drive.mdaw".into());
    s.set_bpm(98.0);
    s.project.unsaved = Some(ProjectOp::Close);
}

/// The same prompt on an untitled project (New / Open while dirty).
pub fn unsaved_untitled(s: &mut App) {
    started(s);
    s.set_bpm(98.0);
    s.project.unsaved = Some(ProjectOp::Open);
}

/// `legacy-v1.0.mdaw` opened: the migration's warning.
pub fn open_report(s: &mut App) {
    started(s);
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../mdaw-model/testdata/legacy-v1.0.mdaw"
    );
    s.load_project_file(path);
    if let Some(r) = &mut s.project.report {
        r.path = "/Users/you/Music/Micro DAW/Old Song.mdaw".into();
    }
}

/// A file that is not a project.
pub fn open_error(s: &mut App) {
    started(s);
    s.project.report = Some(LoadReport {
        path: "/Users/you/Downloads/notes.mdaw".into(),
        error: Some("Failed to load project: expected value at line 1 column 1".into()),
        warnings: Vec::new(),
    });
}

/// The expanded sidebar with the recent projects.
pub fn sidebar_recent(s: &mut App) {
    started(s);
    s.project.recent = recent();
    s.project.file_path = Some(recent()[0].file_path.clone());
    s.sidebar_open = true;
}

/// The palette searched for a recent project.
pub fn palette_recent(s: &mut App) {
    started(s);
    s.project.recent = recent();
    s.open_dialog(crate::views::Dialog::Palette);
    s.p9.palette.query = rune::TextEdit::new("lo-fi");
}

/// Settings: an update is available.
pub fn settings_update(s: &mut App) {
    started(s);
    s.switch_mode(AppMode::Settings);
    s.shell.updater.status = UpdateStatus::Available {
        version: "1.5.0".into(),
    };
    s.shell.updater.checked_on_start = true;
}

/// The Electron import note over the Beat view.
pub fn import_note(s: &mut App) {
    started(s);
    s.shell.import_note = Some(crate::electron_import::ImportSummary {
        profile: "/Users/you/Library/Application Support/micro-daw".into(),
        origin: Some("file://".into()),
        imported: vec!["micro-daw-settings".into(); 14],
        kept: Vec::new(),
        packs_dir: Some("/Users/you/Library/Application Support/micro-daw/packs".into()),
        error: None,
    });
}

/// The app icon (1024 × 1024, macOS grid: the 824-point tile with its shadow) on black; with
/// [`app_icon_white`] the bundle script mattes the two into a transparent PNG.
pub fn app_icon(s: &mut App) {
    s.shell.icon_scene = Some(rune::Color::BLACK);
}

/// [`app_icon`] on white.
pub fn app_icon_white(s: &mut App) {
    s.shell.icon_scene = Some(rune::Color::WHITE);
}
