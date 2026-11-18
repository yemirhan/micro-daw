//! The pure parts of `src/services/PackService.ts`: pack kits → `KitState`, pack file paths and
//! labels, which packs a project uses, IPC error cleanup, pack display names (catalog →
//! installed → `packs/withdrawn.json` → remembered `micro-daw:pack-names` → id), and the state
//! reductions the service applies (progress events, install errors, pack status, "changed
//! since last refresh"). The service object itself (IPC, timers, audio) lives in a later crate.
//!
//! Pad settings are normalized by [`crate::kit::normalize_pad`], paths by [`crate::paths`] and
//! legacy absolute paths are mapped by [`crate::pack_source::pack_ref_from_path`].

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::OnceLock;

use serde_json::{Map, Value};

use crate::js;
use super::validate::{is_safe_pack_path, is_valid_pack_id};
use super::{InstalledPack, PackCatalog, PackFile, PackInstallState, PackManifest};
use crate::kit::normalize_pad;
use crate::pack_source::pack_ref_from_path;
use crate::paths::{basename, display_name, join_path, separator_of};
use crate::types::{KitState, PAD_COUNT, PadSample, PadSampleSource, PadSettings};

// ---------------------------------------------------------------------------
// Pad normalization
// ---------------------------------------------------------------------------

/// `normalizePad({ ...defaultPadSettings(), ...settings, sample })` for pack-kit settings.
pub(crate) fn normalize_pad_settings(
    settings: Option<&Map<String, Value>>,
    sample: Option<PadSample>,
) -> PadSettings {
    // `normalizePad` falls back to `defaultPadSettings()` per field, so spreading the defaults
    // first changes nothing; the explicit `sample` replaces whatever the settings carried.
    let raw = Value::Object(settings.cloned().unwrap_or_default());
    PadSettings {
        sample,
        ..normalize_pad(&raw, None)
    }
}

/// `sourceFallbackName` of a pack / file source: `displayName(basename(path)) || 'Sample'`.
pub(crate) fn fallback_sample_name(path: &str) -> String {
    let n = display_name(&basename(path));
    if n.is_empty() { "Sample".to_owned() } else { n }
}

/// `normalizeSample` of a pack source: an invalid pack id or unsafe file drops the sample.
fn pack_sample(pack_id: &str, file: &str, name: &str) -> Option<PadSample> {
    if !is_valid_pack_id(pack_id) || !is_safe_pack_path(file) {
        return None;
    }
    let name = if name.is_empty() {
        fallback_sample_name(file)
    } else {
        name.to_owned()
    };
    Some(PadSample {
        source: PadSampleSource::Pack {
            pack_id: pack_id.into(),
            file: file.into(),
        },
        name,
    })
}

// ---------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------

/// Stable KitState id of a pack kit.
pub fn pack_kit_id(pack_id: &str, kit_id: &str) -> String {
    format!("pack:{pack_id}:{kit_id}")
}

/// Absolute path of a pack file (`rel` is POSIX; converted for Windows-style pack dirs).
pub fn pack_file_path(dir: &str, rel: &str) -> String {
    let rel = if separator_of(dir) == '\\' {
        rel.replace('/', "\\")
    } else {
        rel.to_owned()
    };
    join_path(dir, &rel)
}

/// PackService's `packFileLabel`: the `label` when non-empty (untrimmed), else the file name
/// without extension. (The Packs page uses [`super::model::pack_file_label`], which trims.)
pub fn service_pack_file_label(path: &str, label: Option<&str>) -> String {
    match label {
        Some(l) if !l.is_empty() => l.to_owned(),
        _ => display_name(&basename(path)),
    }
}

/// A 16-pad KitState for kit `kit_id` of pack `pack_id` (`kitFromPack`): `{ kind: 'pack' }`
/// sources, pad settings from the manifest (clamped like normalizePad), unused pads empty.
/// None when the pack has no such kit.
pub fn kit_from_pack(pack_id: &str, manifest: &PackManifest, kit_id: &str) -> Option<KitState> {
    let kit = manifest.kits.iter().find(|k| k.id == kit_id)?;
    // `new Map(files.map(f => [f.path, f]))`: the last file with a path wins.
    let files: HashMap<&str, &PackFile> = manifest
        .files
        .iter()
        .map(|f| (f.path.as_str(), f))
        .collect();
    let mut pads: Vec<PadSettings> = (0..PAD_COUNT).map(|_| PadSettings::default()).collect();
    for p in &kit.pads {
        if p.pad < 0 || p.pad >= PAD_COUNT as i64 {
            continue;
        }
        let Some(file) = files.get(p.file.as_str()) else {
            continue;
        };
        if !is_safe_pack_path(&p.file) {
            continue;
        }
        let name = match p.name.as_deref() {
            Some(n) if !n.is_empty() => n.to_owned(),
            _ => service_pack_file_label(&file.path, file.label.as_deref()),
        };
        pads[p.pad as usize] =
            normalize_pad_settings(p.settings.as_ref(), pack_sample(pack_id, &p.file, &name));
    }
    Some(KitState {
        id: pack_kit_id(pack_id, &kit.id),
        name: kit.name.clone(),
        pads,
    })
}

/// `kitFromPack(installed, kitId)`.
pub fn kit_from_installed(installed: &InstalledPack, kit_id: &str) -> Option<KitState> {
    kit_from_pack(&installed.id, &installed.manifest, kit_id)
}

/// Pack id a pad source belongs to (pack sources, and absolute paths into `packs_dir`).
pub fn pack_id_of_pad_source(
    source: Option<&PadSampleSource>,
    packs_dir: Option<&str>,
) -> Option<String> {
    match source? {
        PadSampleSource::Pack { pack_id, .. } => Some(pack_id.clone()),
        PadSampleSource::File { path, .. } => pack_ref_from_path(packs_dir, path).map(|r| r.pack_id),
        _ => None,
    }
}

/// Ids of every pack the kits use (pack sources and legacy absolute paths into the packs folder).
pub fn pack_ids_used_by_kits(
    kits: &[Option<&KitState>],
    packs_dir: Option<&str>,
) -> HashSet<String> {
    let mut out = HashSet::new();
    for kit in kits.iter().flatten() {
        for p in &kit.pads {
            if let Some(id) = pack_id_of_pad_source(p.sample.as_ref().map(|s| &s.source), packs_dir)
            {
                out.insert(id);
            }
        }
    }
    out
}

/// Strips Electron's "Error invoking remote method 'x': Error: " prefix (`ipcErrorMessage`,
/// given the error's message / `String(err)`); an empty result is 'Unknown error'.
pub fn ipc_error_message(msg: &str) -> String {
    let mut out = msg;
    if let Some(rest) = msg.strip_prefix("Error invoking remote method '")
        && let Some(q) = rest.find('\'')
        && q > 0
        && rest[q..].starts_with("': ")
    {
        out = &rest[q + 3..];
        // `(\w*Error: )?`
        let word_end = out
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(out.len());
        let word = &out[..word_end];
        if word.ends_with("Error") && out[word_end..].starts_with(": ") {
            out = &out[word_end + 2..];
        }
    }
    if out.is_empty() {
        "Unknown error".into()
    } else {
        out.into()
    }
}

// ---------------------------------------------------------------------------
// Pack names (N6)
// ---------------------------------------------------------------------------

/// localStorage key of the pack names this machine has seen (catalog + installed).
pub const PACK_NAMES_STORAGE_KEY: &str = "micro-daw:pack-names";
/// The remembered-names map keeps at most this many entries (most recent last).
pub const MAX_REMEMBERED_NAMES: usize = 200;

/// `packs/withdrawn.json` (bundled): display names of packs the catalog no longer lists.
pub fn withdrawn_pack_names() -> &'static BTreeMap<String, String> {
    static NAMES: OnceLock<BTreeMap<String, String>> = OnceLock::new();
    NAMES.get_or_init(|| {
        let raw: Value = serde_json::from_str(include_str!("../../../../../packs/withdrawn.json"))
            .unwrap_or(Value::Null);
        raw["packs"]
            .as_object()
            .map(|o| {
                o.iter()
                    .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned())))
                    .collect()
            })
            .unwrap_or_default()
    })
}

/// Remembered pack names in JS object order (array-index-like ids first, ascending; then
/// insertion order = most recent last).
pub type RememberedNames = Vec<(String, String)>;

fn reorder(names: RememberedNames) -> RememberedNames {
    let map: Map<String, Value> = names
        .into_iter()
        .map(|(k, v)| (k, Value::String(v)))
        .collect();
    js::order_keys(map)
        .into_iter()
        .map(|(k, v)| (k, v.as_str().unwrap_or_default().to_owned()))
        .collect()
}

/// `readRememberedNames`: the stored JSON object's non-empty string values (`__proto__` is
/// dropped, as the JS assignment does); anything else `{}`.
pub fn parse_remembered_names(raw: Option<&str>) -> RememberedNames {
    let Some(raw) = raw.filter(|r| !r.is_empty()) else {
        return vec![];
    };
    match serde_json::from_str::<Value>(raw) {
        Ok(Value::Object(o)) => reorder(
            o.into_iter()
                .filter_map(|(k, v)| match v {
                    // `out['__proto__'] = v` sets the prototype, not a key.
                    Value::String(_) if k == "__proto__" => None,
                    Value::String(s) if !s.is_empty() => Some((k, s)),
                    _ => None,
                })
                .collect(),
        ),
        _ => vec![],
    }
}

/// `JSON.stringify(names)`.
pub fn serialize_remembered_names(names: &RememberedNames) -> String {
    let map: Map<String, Value> = names
        .iter()
        .map(|(k, v)| (k.clone(), Value::String(v.clone())))
        .collect();
    Value::Object(map).to_string()
}

/// `rememberNames`: re-inserts changed names (most recent last), trims the oldest beyond
/// [`MAX_REMEMBERED_NAMES`]. None when nothing changed (nothing is written then).
pub fn remember_names<'a>(
    prev: &RememberedNames,
    manifests: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Option<RememberedNames> {
    let mut next = prev.clone();
    let mut changed = false;
    for (id, name) in manifests {
        if name.is_empty() {
            continue;
        }
        if next.iter().any(|(k, v)| k == id && v == name) {
            continue;
        }
        next.retain(|(k, _)| k != id);
        next.push((id.to_owned(), name.to_owned()));
        next = reorder(next);
        changed = true;
    }
    if !changed {
        return None;
    }
    let excess = next.len().saturating_sub(MAX_REMEMBERED_NAMES);
    next.drain(..excess);
    Some(next)
}

/// `packName(id)`: catalog → installed copy → withdrawn list → remembered name → the id.
pub fn pack_name(
    id: &str,
    catalog: Option<&PackCatalog>,
    installed: &[InstalledPack],
    remembered: &RememberedNames,
) -> String {
    let nonempty = |s: &str| (!s.is_empty()).then(|| s.to_owned());
    catalog
        .and_then(|c| c.packs.iter().find(|p| p.id == id))
        .and_then(|p| nonempty(&p.name))
        .or_else(|| {
            installed
                .iter()
                .find(|p| p.id == id)
                .and_then(|p| nonempty(&p.manifest.name))
        })
        .or_else(|| withdrawn_pack_names().get(id).and_then(|n| nonempty(n)))
        .or_else(|| {
            remembered
                .iter()
                .find(|(k, _)| k == id)
                .and_then(|(_, v)| nonempty(v))
        })
        .unwrap_or_else(|| id.to_owned())
}

// ---------------------------------------------------------------------------
// State reductions
// ---------------------------------------------------------------------------

/// Status of a pack referenced by a pad (`PackRefStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PackRefStatus {
    Installing,
    Installed,
    /// The installed list has not been loaded yet.
    Unknown,
    /// The loaded catalog does not list the pack (withdrawn / unknown) and it is not installed.
    Unavailable,
    Missing,
}

/// The renderer-side pack store state the pure queries read (`PackServiceSnapshot` subset).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PackStoreState {
    pub catalog: Option<PackCatalog>,
    pub installed: Vec<InstalledPack>,
    /// Install states of packs that are downloading or failed.
    pub install_states: BTreeMap<String, PackInstallState>,
    /// `installedStamps`: None until the first installed-list refresh.
    pub installed_stamps: Option<BTreeMap<String, String>>,
}

impl PackStoreState {
    pub fn is_installed(&self, id: &str) -> bool {
        self.installed.iter().any(|p| p.id == id)
    }

    pub fn get_installed(&self, id: &str) -> Option<&InstalledPack> {
        self.installed.iter().find(|p| p.id == id)
    }

    /// The loaded catalog does not list pack `id` and it is not installed (false while the
    /// catalog is not loaded).
    pub fn is_unavailable(&self, id: &str) -> bool {
        self.catalog
            .as_ref()
            .is_some_and(|c| !self.is_installed(id) && !c.packs.iter().any(|p| p.id == id))
    }

    pub fn pack_status(&self, id: &str) -> PackRefStatus {
        if matches!(
            self.install_states.get(id),
            Some(PackInstallState::Downloading { .. })
        ) {
            return PackRefStatus::Installing;
        }
        if self.is_installed(id) {
            return PackRefStatus::Installed;
        }
        if self.installed_stamps.is_none() {
            return PackRefStatus::Unknown;
        }
        if self.is_unavailable(id) {
            PackRefStatus::Unavailable
        } else {
            PackRefStatus::Missing
        }
    }

    /// Effective state: running/failed install, else installed/idle.
    pub fn install_state(&self, id: &str) -> PackInstallState {
        self.install_states
            .get(id)
            .cloned()
            .unwrap_or(if self.is_installed(id) {
                PackInstallState::Installed
            } else {
                PackInstallState::Idle
            })
    }

    /// Installed copy is older than the catalog's manifest.
    pub fn has_update(&self, manifest: &PackManifest) -> bool {
        self.get_installed(&manifest.id)
            .is_some_and(|i| i.version < manifest.version)
    }

    /// Packs the kits use that are not installed (sorted); empty before the first refresh.
    pub fn missing_packs(
        &self,
        kits: &[Option<&KitState>],
        packs_dir: Option<&str>,
    ) -> Vec<String> {
        if self.installed_stamps.is_none() {
            return vec![];
        }
        let mut ids: Vec<String> = pack_ids_used_by_kits(kits, packs_dir)
            .into_iter()
            .filter(|id| !self.is_installed(id))
            .collect();
        ids.sort_by(|a, b| js::utf16_cmp(a, b));
        ids
    }

    /// `missingPackOf`: the pack id of a pad source whose pack is `missing`.
    pub fn missing_pack_of(
        &self,
        source: Option<&PadSampleSource>,
        packs_dir: Option<&str>,
    ) -> Option<String> {
        let id = pack_id_of_pad_source(source, packs_dir)?;
        (self.pack_status(&id) == PackRefStatus::Missing).then_some(id)
    }

    /// `invalidateChanged`: records the new stamps (`${version}@${installedAt}`) and returns
    /// the ids whose buffers must be dropped (changed or removed; none on the first refresh).
    pub fn invalidate_changed(&mut self, installed: &[InstalledPack]) -> Vec<String> {
        let mut next = BTreeMap::new();
        let mut order = Vec::new();
        for p in installed {
            if !next.contains_key(&p.id) {
                order.push(p.id.clone());
            }
            next.insert(p.id.clone(), format!("{}@{}", p.version, p.installed_at));
        }
        let prev = self.installed_stamps.replace(next.clone());
        let Some(prev) = prev else { return vec![] };
        let mut changed: Vec<String> = Vec::new();
        for id in &order {
            if prev.get(id) != next.get(id) && !changed.contains(id) {
                changed.push(id.clone());
            }
        }
        for id in prev.keys() {
            if !next.contains_key(id) && !changed.contains(id) {
                changed.push(id.clone());
            }
        }
        changed
    }

    /// `onProgress`: `downloading` / `error` are stored; `installed` / `idle` clear the state
    /// and return true (the caller then refreshes the installed list).
    pub fn apply_progress(&mut self, id: &str, state: PackInstallState) -> bool {
        match state {
            PackInstallState::Downloading { .. } | PackInstallState::Error { .. } => {
                self.install_states.insert(id.to_owned(), state);
                false
            }
            PackInstallState::Installed | PackInstallState::Idle => {
                self.install_states.remove(id);
                true
            }
        }
    }

    /// `install(id)` start: optimistic `downloading { 0, null, 0, 0 }`.
    pub fn begin_install(&mut self, id: &str) {
        self.install_states.insert(
            id.to_owned(),
            PackInstallState::Downloading {
                received_bytes: 0,
                total_bytes: None,
                file_index: 0,
                file_count: 0,
            },
        );
    }

    /// `install(id)` failure with the cleaned message: a `/cancelled/i` message clears a still
    /// `downloading` state; any other message sets `error` unless main already set one.
    pub fn install_failed(&mut self, id: &str, message: &str) {
        if message.to_ascii_lowercase().contains("cancelled") {
            if matches!(
                self.install_states.get(id),
                Some(PackInstallState::Downloading { .. })
            ) {
                self.install_states.remove(id);
            }
        } else if !matches!(
            self.install_states.get(id),
            Some(PackInstallState::Error { .. })
        ) {
            self.install_states.insert(
                id.to_owned(),
                PackInstallState::Error {
                    message: message.to_owned(),
                },
            );
        }
    }
}

/// `packKitChoices`: kit choices of installed packs, in install order (id, kit name, pack
/// name, pack color); build the kit with [`kit_from_installed`].
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct KitChoice {
    pub id: String,
    pub name: String,
    pub detail: Option<String>,
    pub color: Option<String>,
    #[serde(skip)]
    pub pack_id: String,
    #[serde(skip)]
    pub kit_id: String,
}

pub fn pack_kit_choices(installed: &[InstalledPack]) -> Vec<KitChoice> {
    installed
        .iter()
        .flat_map(|inst| {
            inst.manifest.kits.iter().map(move |k| KitChoice {
                id: pack_kit_id(&inst.id, &k.id),
                name: k.name.clone(),
                detail: Some(inst.manifest.name.clone()),
                color: Some(inst.manifest.color.clone()),
                pack_id: inst.id.clone(),
                kit_id: k.id.clone(),
            })
        })
        .collect()
}
