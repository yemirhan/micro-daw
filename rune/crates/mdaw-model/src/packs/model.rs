//! Port of `src/components/packs/packsModel.ts`: one card model for built-in classic kits and
//! downloadable packs, search / tag filtering, sizes, sound grouping and the install-button
//! state. No UI, no audio.

use std::cmp::Ordering;
use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

use super::{
    INSTALL_PAUSED_MESSAGE, InstalledPack, PackFile, PackInstallState, PackLicense, PackManifest,
};
use crate::fs_entries::{collator_compare, locale_compare};
use crate::js;
use crate::types::{ClassicMachineId, KitState, PadSampleSource};

/// One voice of a classic machine (`ClassicMachineInfo.voices[i]`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClassicVoiceInfo {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gm: Option<i64>,
}

/// Shape of one entry of `CLASSIC_MACHINES` (structural, so this module has no audio imports).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClassicMachineInfo {
    pub id: ClassicMachineId,
    pub name: String,
    pub tagline: String,
    pub color: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub voices: Vec<ClassicVoiceInfo>,
    /// Default 16-pad layout (voice ids; '' = empty pad).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pads: Option<Vec<String>>,
}

/// `PackItem`: a card of the Packs page.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum PackItem {
    #[serde(rename_all = "camelCase")]
    Builtin {
        id: String,
        machine: ClassicMachineInfo,
        name: String,
        tagline: String,
        description: String,
        tags: Vec<String>,
        color: String,
        sound_count: usize,
    },
    #[serde(rename_all = "camelCase")]
    Remote {
        id: String,
        manifest: PackManifest,
        name: String,
        tagline: String,
        description: String,
        tags: Vec<String>,
        color: String,
        sound_count: usize,
        /// Sum of known file sizes (bytes); None when no file has a size.
        size_bytes: Option<i64>,
    },
}

impl PackItem {
    pub fn id(&self) -> &str {
        match self {
            Self::Builtin { id, .. } | Self::Remote { id, .. } => id,
        }
    }
    pub fn name(&self) -> &str {
        match self {
            Self::Builtin { name, .. } | Self::Remote { name, .. } => name,
        }
    }
    pub fn tagline(&self) -> &str {
        match self {
            Self::Builtin { tagline, .. } | Self::Remote { tagline, .. } => tagline,
        }
    }
    pub fn description(&self) -> &str {
        match self {
            Self::Builtin { description, .. } | Self::Remote { description, .. } => description,
        }
    }
    pub fn tags(&self) -> &[String] {
        match self {
            Self::Builtin { tags, .. } | Self::Remote { tags, .. } => tags,
        }
    }
    pub fn sound_count(&self) -> usize {
        match self {
            Self::Builtin { sound_count, .. } | Self::Remote { sound_count, .. } => *sound_count,
        }
    }
    pub fn is_builtin(&self) -> bool {
        matches!(self, Self::Builtin { .. })
    }
}

fn machine_tag(id: ClassicMachineId) -> &'static str {
    match id {
        ClassicMachineId::Tr808 => "808",
        ClassicMachineId::Tr909 => "909",
        ClassicMachineId::Tr707 => "707",
        ClassicMachineId::Tr606 => "606",
        ClassicMachineId::Linn => "linn",
        ClassicMachineId::Cr78 => "cr-78",
    }
}

pub fn builtin_item_id(machine: ClassicMachineId) -> String {
    format!("builtin:{}", machine.id())
}

pub fn builtin_item(machine: &ClassicMachineInfo) -> PackItem {
    let description = match &machine.description {
        Some(d) if !d.is_empty() => d.clone(),
        _ => format!(
            "{} sounds synthesized right here in the app: an original take on the classic machine's character, not samples of it.",
            machine.name
        ),
    };
    PackItem::Builtin {
        id: builtin_item_id(machine.id),
        machine: machine.clone(),
        name: machine.name.clone(),
        tagline: machine.tagline.clone(),
        description,
        tags: vec![
            machine_tag(machine.id).into(),
            "built-in".into(),
            "drum machine".into(),
        ],
        color: machine.color.clone(),
        sound_count: machine.voices.len(),
    }
}

/// Sum of the files' sizes (every `size` is a finite number here, so `None` only for no files).
/// TS: sizes that are not finite non-negative numbers are skipped; negative sizes are skipped
/// here too.
pub fn pack_size_bytes(files: &[PackFile]) -> Option<i64> {
    let mut total = 0;
    let mut known = false;
    for f in files {
        if f.size >= 0 {
            total += f.size;
            known = true;
        }
    }
    known.then_some(total)
}

/// True for a playable pack file (packs also ship LICENSE / README text files).
pub fn is_sound_file(path: &str) -> bool {
    let ext = path
        .rfind('.')
        .map(|d| path[d + 1..].to_lowercase())
        .unwrap_or_default();
    ext != "txt" && ext != "md"
}

/// The pack's playable sounds (licence / readme files left out).
pub fn sound_files(files: &[PackFile]) -> Vec<&PackFile> {
    files.iter().filter(|f| is_sound_file(&f.path)).collect()
}

pub fn remote_item(manifest: &PackManifest) -> PackItem {
    PackItem::Remote {
        id: manifest.id.clone(),
        manifest: manifest.clone(),
        name: manifest.name.clone(),
        tagline: manifest.tagline.clone(),
        description: manifest.description.clone(),
        tags: manifest.tags.clone(),
        color: manifest.color.clone(),
        sound_count: sound_files(&manifest.files).len(),
        size_bytes: pack_size_bytes(&manifest.files),
    }
}

/// Downloadable pack items: the catalog's packs, plus installed packs the catalog no longer
/// lists (offline, or removed upstream) so they stay usable.
pub fn remote_items(
    catalog_packs: Option<&[PackManifest]>,
    installed: &[InstalledPack],
) -> Vec<PackItem> {
    let mut seen = HashSet::new();
    let mut items = Vec::new();
    for m in catalog_packs.unwrap_or(&[]) {
        if seen.insert(m.id.clone()) {
            items.push(remote_item(m));
        }
    }
    for p in installed {
        if seen.insert(p.id.clone()) {
            items.push(remote_item(&p.manifest));
        }
    }
    items
}

/// `s.toLowerCase().normalize('NFKD').replace(/[̀-ͯ]/g, '')`.
pub fn norm(s: &str) -> String {
    s.to_lowercase()
        .nfkd()
        .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
        .collect()
}

/// Every whitespace-separated query word must appear in the item's text (name, tagline, tags,
/// description, author + licence).
pub fn matches_query(item: &PackItem, query: &str) -> bool {
    let q = norm(query);
    let words: Vec<&str> = q.split(js::is_js_ws).filter(|w| !w.is_empty()).collect();
    if words.is_empty() {
        return true;
    }
    let extra = match item {
        PackItem::Remote { manifest, .. } => {
            format!("{} {}", manifest.author, manifest.license.id())
        }
        PackItem::Builtin { .. } => String::new(),
    };
    let hay = norm(
        &[
            item.name(),
            item.tagline(),
            &item.tags().join(" "),
            item.description(),
            &extra,
        ]
        .join(" "),
    );
    words.iter().all(|w| hay.contains(w))
}

pub fn matches_tag(item: &PackItem, tag: Option<&str>) -> bool {
    let Some(tag) = tag.filter(|t| !t.is_empty()) else {
        return true;
    };
    let t = norm(tag);
    item.tags().iter().any(|x| norm(x) == t)
}

pub fn filter_items(items: &[PackItem], query: &str, tag: Option<&str>) -> Vec<PackItem> {
    items
        .iter()
        .filter(|i| matches_tag(i, tag) && matches_query(i, query))
        .cloned()
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagCount {
    pub tag: String,
    pub count: usize,
}

/// Default `limit` of [`top_tags`].
pub const TOP_TAGS_LIMIT: usize = 10;

/// Tags by frequency (ties by `localeCompare`), case-insensitive, at most `limit`.
pub fn top_tags(items: &[PackItem], limit: usize) -> Vec<TagCount> {
    let mut counts: Vec<(String, TagCount)> = Vec::new();
    for item in items {
        let mut seen = HashSet::new();
        for tag in item.tags() {
            let key = norm(tag);
            if key.is_empty() || !seen.insert(key.clone()) {
                continue;
            }
            match counts.iter_mut().find(|(k, _)| *k == key) {
                Some((_, e)) => e.count += 1,
                None => counts.push((
                    key,
                    TagCount {
                        tag: tag.to_lowercase(),
                        count: 1,
                    },
                )),
            }
        }
    }
    let mut out: Vec<TagCount> = counts.into_iter().map(|(_, e)| e).collect();
    out.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| locale_compare(&a.tag, &b.tag))
    });
    out.truncate(limit);
    out
}

/// "1.5 MB", "2 KB", "12 B"; "" for None / non-finite / negative.
pub fn format_bytes(bytes: Option<f64>) -> String {
    let Some(bytes) = bytes.filter(|b| b.is_finite() && *b >= 0.0) else {
        return String::new();
    };
    if bytes < 1024.0 {
        return format!("{} B", js::number_to_string(bytes));
    }
    if bytes < 1024.0 * 1024.0 {
        return format!("{} KB", js::number_to_string(js::round(bytes / 1024.0)));
    }
    let mb = bytes / (1024.0 * 1024.0);
    let n = if mb < 10.0 {
        js::to_fixed(mb, 1)
    } else {
        js::number_to_string(js::round(mb))
    };
    format!("{n} MB")
}

/// `LICENSE_LABEL`.
pub fn license_label(l: PackLicense) -> &'static str {
    match l {
        PackLicense::Cc0 => "CC0",
        PackLicense::PublicDomain => "Public domain",
        PackLicense::CcBy4 => "CC BY 4.0",
    }
}

/// `LICENSE_URL`.
pub fn license_url(l: PackLicense) -> Option<&'static str> {
    match l {
        PackLicense::Cc0 => Some("https://creativecommons.org/publicdomain/zero/1.0/"),
        PackLicense::PublicDomain => None,
        PackLicense::CcBy4 => Some("https://creativecommons.org/licenses/by/4.0/"),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SoundGroup {
    /// Folder path ('' = pack root).
    pub folder: String,
    pub files: Vec<PackFile>,
}

/// Files grouped by their folder, root first, folders sorted, files in natural name order.
pub fn group_files_by_folder(files: &[PackFile]) -> Vec<SoundGroup> {
    let mut groups: Vec<(String, Vec<PackFile>)> = Vec::new();
    for f in files {
        let folder = f
            .path
            .rfind('/')
            .map(|i| f.path[..i].to_owned())
            .unwrap_or_default();
        match groups.iter_mut().find(|(k, _)| *k == folder) {
            Some((_, list)) => list.push(f.clone()),
            None => groups.push((folder, vec![f.clone()])),
        }
    }
    groups.sort_by(|(a, _), (b, _)| {
        if a.is_empty() {
            Ordering::Less
        } else if b.is_empty() {
            Ordering::Greater
        } else {
            collator_compare(a, b)
        }
    });
    groups
        .into_iter()
        .map(|(folder, mut list)| {
            list.sort_by(|a, b| collator_compare(&a.path, &b.path));
            SoundGroup {
                folder,
                files: list,
            }
        })
        .collect()
}

/// "kicks/BD 01.wav" -> "BD 01" (drops a final `.ext` of 2..5 ASCII letters/digits).
pub fn sound_label(path: &str) -> String {
    let base = path.rsplit('/').next().unwrap_or(path);
    if let Some(dot) = base.rfind('.') {
        let ext = &base[dot + 1..];
        if (2..=5).contains(&ext.len()) && ext.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return base[..dot].to_owned();
        }
    }
    base.to_owned()
}

/// What the card / detail install button shows.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum InstallButtonState {
    Builtin,
    Get,
    Downloading {
        progress: Option<f64>,
        label: String,
    },
    Installed,
    Error {
        message: String,
    },
    /// An interrupted install kept its downloaded files; Get continues it.
    Resume,
}

/// The install state of a pack whose download was interrupted (see `INSTALL_PAUSED_MESSAGE`).
pub fn is_paused_install(state: Option<&PackInstallState>) -> bool {
    matches!(state, Some(PackInstallState::Error { message }) if message == INSTALL_PAUSED_MESSAGE)
}

/// 0..1 overall progress of a download, or None when the total is unknown. Non-downloading
/// states give None.
pub fn download_progress(s: &PackInstallState) -> Option<f64> {
    let PackInstallState::Downloading {
        received_bytes,
        total_bytes,
        file_index,
        file_count,
    } = s
    else {
        return None;
    };
    if let Some(total) = total_bytes.filter(|t| *t > 0) {
        return Some((*received_bytes as f64 / total as f64).clamp(0.0, 1.0));
    }
    if *file_count > 0 {
        return Some((*file_index as f64 / *file_count as f64).clamp(0.0, 1.0));
    }
    None
}

pub fn install_button_state(
    item: &PackItem,
    state: Option<&PackInstallState>,
    installed: bool,
) -> InstallButtonState {
    if item.is_builtin() {
        return InstallButtonState::Builtin;
    }
    pack_button_state(state, installed)
}

/// Install-button state of a downloadable pack from its install state.
pub fn pack_button_state(state: Option<&PackInstallState>, installed: bool) -> InstallButtonState {
    if let Some(
        s @ PackInstallState::Downloading {
            file_index,
            file_count,
            ..
        },
    ) = state
    {
        let progress = download_progress(s);
        let label = match progress {
            Some(p) => format!("{}%", js::number_to_string(js::round(p * 100.0))),
            None if *file_count > 0 => {
                format!("{}/{}", (file_index + 1).min(*file_count), file_count)
            }
            None => "…".into(),
        };
        return InstallButtonState::Downloading { progress, label };
    }
    if is_paused_install(state) {
        return InstallButtonState::Resume;
    }
    if let Some(PackInstallState::Error { message }) = state {
        return InstallButtonState::Error {
            message: message.clone(),
        };
    }
    if installed || matches!(state, Some(PackInstallState::Installed)) {
        return InstallButtonState::Installed;
    }
    InstallButtonState::Get
}

/// Which of 16 pads a pack's first kit (or a classic machine) fills, for the faceplate.
pub fn lit_pads(item: &PackItem) -> [bool; 16] {
    let mut lit = [false; 16];
    match item {
        PackItem::Builtin { machine, .. } => match &machine.pads {
            Some(pads) => {
                for (i, v) in pads.iter().take(16).enumerate() {
                    lit[i] = !v.is_empty();
                }
            }
            None => {
                for l in lit.iter_mut().take(machine.voices.len().min(16)) {
                    *l = true;
                }
            }
        },
        PackItem::Remote {
            manifest,
            sound_count,
            ..
        } => match manifest.kits.first() {
            None => {
                for l in lit.iter_mut().take((*sound_count).min(16)) {
                    *l = true;
                }
            }
            Some(kit) => {
                for p in &kit.pads {
                    if (0..16).contains(&p.pad) {
                        lit[p.pad as usize] = true;
                    }
                }
            }
        },
    }
    lit
}

/// A playable sound in the detail panel: a classic voice or a pack file.
#[derive(Debug, Clone, PartialEq)]
pub enum SoundRef<'a> {
    Voice(&'a str),
    File(&'a PackFile),
}

/// Key of a sound preview for one sound of a pack / machine.
pub fn sound_key(item_id: &str, sound: &SoundRef) -> String {
    match sound {
        SoundRef::Voice(v) => format!("{item_id}::{v}"),
        SoundRef::File(f) => format!("{item_id}::{}", f.path),
    }
}

/// Display name of a pack file: its trimmed `label` when non-blank, else [`sound_label`].
pub fn pack_file_label(file: &PackFile) -> String {
    match file.label.as_deref().map(js::trim) {
        Some(l) if !l.is_empty() => l.to_owned(),
        _ => sound_label(&file.path),
    }
}

/// Pack id of a pad source that refers to a pack (`kind: 'pack'`), else None.
pub fn pack_id_of_source(source: Option<&PadSampleSource>) -> Option<&str> {
    match source {
        Some(PadSampleSource::Pack { pack_id, .. }) => Some(pack_id),
        _ => None,
    }
}

/// `/[\\/]+$/` removed.
fn strip_trailing_seps(dir: &str) -> &str {
    dir.trim_end_matches(['/', '\\'])
}

/// Does any pad of `kits` use pack `pack_id` (a pack source, or an absolute file path inside
/// the pack's folder `dir`)?
pub fn kits_use_pack(kits: &[Option<&KitState>], pack_id: &str, dir: Option<&str>) -> bool {
    let prefixes: Vec<String> = match dir.filter(|d| !d.is_empty()) {
        Some(d) => vec![
            format!("{}/", strip_trailing_seps(d)),
            format!("{}\\", strip_trailing_seps(d)),
        ],
        None => vec![],
    };
    kits.iter().flatten().any(|kit| {
        kit.pads.iter().any(|p| {
            let Some(src) = p.sample.as_ref().map(|s| &s.source) else { return false };
            if pack_id_of_source(Some(src)) == Some(pack_id) {
                return true;
            }
            matches!(src, PadSampleSource::File { path, .. } if prefixes.iter().any(|pre| path.starts_with(pre.as_str())))
        })
    })
}
