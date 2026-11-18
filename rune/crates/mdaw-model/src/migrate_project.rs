//! Pure parts of `src/services/ProjectManager.ts`: building / serializing a `ProjectFile`
//! (`writeToFile`), parsing + migrating a loaded file (`loadFromFile`, `preloadProjectKits`),
//! the project name / window title, and the recent-projects list.

use serde_json::Value;

use super::{DroppedDrumNotes, MigrateError, js_truthy, migrate_arrangement_value, prop};
use crate::kit::normalize_kit;
use crate::types::{
    Arrangement, EffectParams, KitState, PROJECT_VERSION, ProjectAudioConfig, ProjectFile, ProjectMeta,
    SampleManifestEntry,
};

/// localStorage key of the recent projects list.
pub const RECENT_PROJECTS_KEY: &str = "micro-daw-recent-projects";
/// At most this many recent projects are kept.
pub const MAX_RECENT: usize = 10;

// ---------------------------------------------------------------------------
// Save
// ---------------------------------------------------------------------------

/// Sample manifest of `writeToFile`: one entry per unique `region.audio.sampleId` (first
/// occurrence, track then region order). Name: the loaded sample's name (`sample_name(id)`), else
/// the region name, else "Sample".
pub fn build_sample_manifest(
    arrangement: &Arrangement,
    sample_name: &dyn Fn(&str) -> Option<String>,
) -> Vec<SampleManifestEntry> {
    let mut out: Vec<SampleManifestEntry> = Vec::new();
    for track in &arrangement.tracks {
        for region in &track.regions {
            let Some(audio) = &region.audio else { continue };
            if out.iter().any(|e| e.sample_id == audio.sample_id) {
                continue;
            }
            out.push(SampleManifestEntry {
                sample_id: audio.sample_id.clone(),
                path: audio.sample_path.clone(),
                name: sample_name(&audio.sample_id).or_else(|| region.name.clone()).unwrap_or_else(|| "Sample".into()),
            });
        }
    }
    out
}

/// The `ProjectFile` `writeToFile` saves (`sampleManifest` only when non-empty).
pub fn build_project_file(
    arrangement: Arrangement,
    master_volume_db: f64,
    effect_params: EffectParams,
    created_at: &str,
    modified_at: &str,
    sample_name: &dyn Fn(&str) -> Option<String>,
) -> ProjectFile {
    let manifest = build_sample_manifest(&arrangement, sample_name);
    ProjectFile {
        version: PROJECT_VERSION.to_owned(),
        arrangement,
        audio_config: ProjectAudioConfig { master_volume: master_volume_db, effect_params },
        created_at: created_at.to_owned(),
        modified_at: modified_at.to_owned(),
        sample_manifest: (!manifest.is_empty()).then_some(manifest),
        extra: Default::default(),
    }
}

/// `JSON.stringify(projectFile, null, 2)`. (Numbers are written the serde way, e.g. `120.0`
/// where JS writes `120`; both apps read either.)
pub fn project_file_to_json(file: &ProjectFile) -> String {
    serde_json::to_string_pretty(file).expect("ProjectFile serializes")
}

// ---------------------------------------------------------------------------
// Load
// ---------------------------------------------------------------------------

/// Why a project failed to load (`console.error('Failed to load project:', err)`, returns false).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectLoadError {
    /// `JSON.parse` threw.
    Json(String),
    /// A TypeError on the data's shape (including inside the migration).
    Shape(MigrateError),
}

impl std::fmt::Display for ProjectLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json(e) => write!(f, "Failed to load project: {e}"),
            Self::Shape(e) => write!(f, "Failed to load project: {e}"),
        }
    }
}

impl std::error::Error for ProjectLoadError {}

impl From<MigrateError> for ProjectLoadError {
    fn from(e: MigrateError) -> Self {
        Self::Shape(e)
    }
}

/// One `sampleManifest` entry as `sampleManager.loadFromPathWithId(entry.sampleId, entry.path)`
/// receives it (None = not a string).
#[derive(Debug, Clone, PartialEq)]
pub struct ManifestLoad {
    pub sample_id: Option<String>,
    pub path: Option<String>,
    /// Used only in the failure warning.
    pub name: Option<String>,
}

/// The four effect params `loadFromFile` restores (the other 16 are saved but not restored).
/// None = the value was not a number (the TS passes it through as is).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RestoredAudio {
    /// Applied only when finite.
    pub master_volume_db: Option<f64>,
    pub reverb_wet: Option<f64>,
    pub chorus_depth: Option<f64>,
    pub filter_cutoff: Option<f64>,
    pub filter_resonance: Option<f64>,
}

/// Everything `loadFromFile` derives from the file, in the order it uses it.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedProject {
    /// `console.warn('Unknown project version:', v)` text when the version is not "1.0.0".
    pub version_warning: Option<String>,
    /// Samples to load before the arrangement.
    pub sample_manifest: Vec<ManifestLoad>,
    /// Normalized drum kits to preload (`preloadProjectKits`).
    pub kits_to_preload: Vec<KitState>,
    /// `data.arrangement` after `migrateArrangement` (what `loadProject` installs).
    pub arrangement: Value,
    pub dropped_drum_notes: Vec<DroppedDrumNotes>,
    pub audio: Option<RestoredAudio>,
    /// `data.createdAt` when truthy (else the caller uses now).
    pub created_at: Option<String>,
    /// `data.modifiedAt` (for the recent list).
    pub modified_at: Option<String>,
}

fn as_string(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_str).map(str::to_owned)
}

/// The pure half of `loadFromFile` + `preloadProjectKits` + `loadProject`'s migration.
/// `new_id` is consumed in the TS order (kit preload first, then the migration).
pub fn load_project_json(
    raw: &str,
    packs_dir: Option<&str>,
    new_id: &mut dyn FnMut() -> String,
) -> Result<LoadedProject, ProjectLoadError> {
    let data: Value = serde_json::from_str(raw).map_err(|e| ProjectLoadError::Json(e.to_string()))?;
    let version = prop(Some(&data), "version", "the project")?;
    let version_warning = (version.and_then(Value::as_str) != Some(PROJECT_VERSION))
        .then(|| format!("Unknown project version: {}", crate::js::string_of(version)));

    let mut sample_manifest = Vec::new();
    let manifest = data.get("sampleManifest");
    if js_truthy(manifest) {
        match manifest {
            Some(Value::Array(entries)) => {
                for entry in entries {
                    let field = |k: &str| prop(Some(entry), k, "a manifest entry").map(as_string);
                    sample_manifest.push(ManifestLoad {
                        sample_id: field("sampleId")?,
                        path: field("path")?,
                        name: field("name")?,
                    });
                }
            }
            Some(Value::String(s)) => {
                sample_manifest.extend(s.encode_utf16().map(|_| ManifestLoad {
                    sample_id: None,
                    path: None,
                    name: None,
                }));
            }
            _ => return Err(MigrateError("data.sampleManifest is not iterable".into()).into()),
        }
    }

    let arrangement = prop(Some(&data), "arrangement", "the project")?;
    let kits_to_preload = preload_project_kits(arrangement, packs_dir, new_id)?;

    let mut arrangement = arrangement.cloned().unwrap_or(Value::Null);
    let dropped_drum_notes = migrate_arrangement_value(&mut arrangement, packs_dir, new_id)?;

    let audio_config = data.get("audioConfig");
    let audio = if js_truthy(audio_config) {
        let num = |v: Option<&Value>| v.and_then(Value::as_f64);
        let fx = prop(audio_config, "effectParams", "audioConfig")?;
        let fx_field = |k: &str| prop(fx, k, "audioConfig.effectParams").map(num);
        Some(RestoredAudio {
            master_volume_db: num(prop(audio_config, "masterVolume", "audioConfig")?),
            reverb_wet: fx_field("reverbWet")?,
            chorus_depth: fx_field("chorusDepth")?,
            filter_cutoff: fx_field("filterCutoff")?,
            filter_resonance: fx_field("filterResonance")?,
        })
    } else {
        None
    };

    let created = data.get("createdAt");
    Ok(LoadedProject {
        version_warning,
        sample_manifest,
        kits_to_preload,
        arrangement,
        dropped_drum_notes,
        audio,
        created_at: if js_truthy(created) { as_string(created) } else { None },
        modified_at: as_string(data.get("modifiedAt")),
    })
}

/// `preloadProjectKits`: the normalized kit of every drum track that has one.
pub fn preload_project_kits(
    arrangement: Option<&Value>,
    packs_dir: Option<&str>,
    new_id: &mut dyn FnMut() -> String,
) -> Result<Vec<KitState>, MigrateError> {
    let tracks = match prop(arrangement, "tracks", "data.arrangement")? {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Array(t)) => t,
        Some(_) => return Err(MigrateError("arrangement.tracks.flatMap is not a function".into())),
    };
    let mut kits = Vec::new();
    for t in tracks {
        let inst = prop(Some(t), "instrument", "a track")?;
        let is_drums = match inst {
            None | Some(Value::Null) => false,
            Some(i) => i.get("type").and_then(Value::as_str) == Some("drums"),
        };
        if is_drums && js_truthy(inst.and_then(|i| i.get("kit"))) {
            kits.push(normalize_kit(&inst.expect("drums")["kit"], packs_dir, new_id));
        }
    }
    Ok(kits)
}

// ---------------------------------------------------------------------------
// Names, title, recent projects
// ---------------------------------------------------------------------------

/// `getProjectName`: "Untitled" without a file, else the file name (split on '/' only, as in the
/// TS) without the `.mdaw` suffix.
pub fn project_name(file_path: Option<&str>) -> String {
    match file_path {
        None => "Untitled".into(),
        Some(p) => strip_mdaw(p.rsplit('/').next().unwrap_or(p)).to_owned(),
    }
}

fn strip_mdaw(name: &str) -> &str {
    name.strip_suffix(".mdaw").unwrap_or(name)
}

/// `updateWindowTitle`: `<name>[ *] - Micro DAW`.
pub fn window_title(file_path: Option<&str>, dirty: bool) -> String {
    format!("{}{} - Micro DAW", project_name(file_path), if dirty { " *" } else { "" })
}

/// `addToRecentProjects`: most recent first, de-duplicated by path, at most MAX_RECENT. The name
/// falls back to "Untitled" when empty.
pub fn add_to_recent_projects(recent: &[ProjectMeta], file_path: &str, modified_at: &str) -> Vec<ProjectMeta> {
    let name = strip_mdaw(file_path.rsplit('/').next().unwrap_or(file_path));
    let mut out = vec![ProjectMeta {
        file_path: file_path.to_owned(),
        name: if name.is_empty() { "Untitled".into() } else { name.to_owned() },
        modified_at: modified_at.to_owned(),
    }];
    out.extend(recent.iter().filter(|p| p.file_path != file_path).cloned());
    out.truncate(MAX_RECENT);
    out
}

/// `loadRecentProjects`: the stored list (`[]` on a parse error; entries that do not fit
/// `ProjectMeta` are skipped, the TS keeps them unvalidated).
pub fn parse_recent_projects(raw: Option<&str>) -> Vec<ProjectMeta> {
    let Some(Ok(Value::Array(items))) = raw.map(serde_json::from_str::<Value>) else { return Vec::new() };
    items.into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect()
}
