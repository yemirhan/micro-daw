//! Renderer `localStorage` emulation plus a typed reader / writer for every key the Electron
//! app stores there, each with the same parsing, validation and fallback as the TS reader.
//!
//! [`SettingsStore`] keeps string values per key (like `localStorage`) and persists them as one
//! JSON object file (`local-storage.json`) in an injected directory, written atomically (temp
//! file + rename). A missing or corrupt file starts empty.
//!
//! ## Keys (complete list; `grep -rn "localStorage\|micro-daw:" src`)
//! | Key | Format | Reader / writer | TS source |
//! |---|---|---|---|
//! | `micro-daw-arrangement` | Arrangement JSON (autosave) | [`read_arrangement`] / [`write_arrangement`] (migration is `migrate.rs`'s) | constants.ts:68, ArrangementEngine.ts:1527-1551 |
//! | `micro-daw-recent-projects` | `[{filePath,name,modifiedAt}]`, most recent first, max 10, unvalidated `JSON.parse` | [`read_recent_projects`], [`add_recent_project`] | ProjectManager.ts:11-12,313-344 |
//! | `micro-daw-settings` | `{general:{autoCheckUpdates}, audio:{defaultMasterVolume,bufferSizeHint}, midi:{autoConnectLastDevice}}` | [`read_app_settings_value`], [`read_app_settings`], [`update_app_settings`] | useSettings.ts, constants.ts:102-109 |
//! | `micro-daw-sample-library` | `[{sampleId,samplePath,sampleName,trim?,addedAt}]` | [`read_sample_library`] / [`write_sample_library`] | useSampleLibrary.ts:9-60 |
//! | `micro-daw-piano-roll` | `{snap≥0, drawLength>0, follow}` (defaults 0.25 / 1 / true) | [`read_piano_roll_prefs`] / [`write_piano_roll_prefs`] | PianoRollEditor.tsx:70-90,191 |
//! | `micro-daw-recordings` | defined (`MAX_RECORDINGS` 20) but never read or written | [`RECORDINGS_STORAGE_KEY`] | constants.ts:63-64 |
//! | `micro-daw:controller` | `{version:1, profile}` (parsed by `controller.rs`) | [`read_controller_raw`] / [`write_controller_raw`] | ControllerMapping.ts:22,252,282 |
//! | `micro-daw:kits` | `KitState[]` (normalized; file `sampleId` stripped) | [`read_kits`], [`save_kit`], [`remove_kit`] | KitStore.ts |
//! | `micro-daw:pack-names` | `{[packId]: name}`, ≤ 200, most recent last | [`read_pack_names`] / [`write_pack_names`] | PackService.ts:92-106,389-408 |
//! | `micro-daw:masterVolume` | number string, dB clamped -40..0, default -12 | [`read_master_volume`] / [`write_master_volume`] | masterBus.ts:53-96 |
//! | `micro-daw:browser-open` | `'false'` = closed, anything else open | [`read_browser_open`] / [`write_browser_open`] | App.tsx:50-55,103 |
//! | `micro-daw:browser-width` | number, clamped 200..520 and rounded, default 248 | [`read_browser_width`] / [`write_browser_width`] | BrowserPanel.tsx:5-26 |
//! | `micro-daw:browser-roots` | JSON array of folder paths (`normalizeRoots`) | [`read_browser_roots`] / [`write_browser_roots`] | useSampleBrowser.ts:17,38-46,206-209 |
//! | `micro-daw:browser-last-path` | path; used only when under a root | [`read_browser_last_path`] / [`write_browser_last_path`] | useSampleBrowser.ts:18,48-51,171 |
//! | `micro-daw:browser-auto-preview` | `'0'` = off, anything else on | [`read_browser_auto_preview`] / [`write_browser_auto_preview`] | useSampleBrowser.ts:19,131,312 |
//! | `micro-daw:dock-pad-bank` | `'B'` = bank B, anything else A | [`read_dock_pad_bank`] / [`write_dock_pad_bank`] | InstrumentDock.tsx:37-67 |
//! | `micro-daw:beat-pad-layout` | `'4x4'` (default) or `'2x8'` | [`read_beat_pad_layout`] / [`write_beat_pad_layout`] | BeatView.tsx:49-65,119-134 |
//! | `micro-daw:beat-step-grid` | `'1/8'`, `'1/16'` (default) or `'1/32'` | [`read_beat_step_grid`] / [`write_beat_step_grid`] | BeatView.tsx |
//! | `micro-daw:beat-key-hints` | `'on'` shows key hints, else off (default `'off'`) | [`read_beat_key_hints`] / [`write_beat_key_hints`] | BeatView.tsx |
//!
//! Numbers are written with JS `String(number)` formatting ([`crate::js::number_to_string`]).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::js;
use crate::packs::service::{RememberedNames, parse_remembered_names, serialize_remembered_names};
use crate::paths::{separator_of, trim_trailing_sep};
use crate::types::{
    AppSettings, AudioSettings, GeneralSettings, KitState, MidiSettings, PadSampleSource,
    ProjectMeta,
};

pub const ARRANGEMENT_STORAGE_KEY: &str = "micro-daw-arrangement";
pub const RECENT_PROJECTS_KEY: &str = "micro-daw-recent-projects";
pub const SETTINGS_STORAGE_KEY: &str = "micro-daw-settings";
pub const SAMPLE_LIBRARY_STORAGE_KEY: &str = "micro-daw-sample-library";
pub const PIANO_ROLL_KEY: &str = "micro-daw-piano-roll";
/// Defined by the Electron app but never read or written.
pub const RECORDINGS_STORAGE_KEY: &str = "micro-daw-recordings";
pub const MAX_RECORDINGS: usize = 20;
pub const CONTROLLER_STORAGE_KEY: &str = "micro-daw:controller";
pub const KITS_KEY: &str = "micro-daw:kits";
pub const PACK_NAMES_KEY: &str = crate::packs::service::PACK_NAMES_STORAGE_KEY;
pub const MASTER_VOLUME_KEY: &str = "micro-daw:masterVolume";
pub const BROWSER_OPEN_KEY: &str = "micro-daw:browser-open";
pub const BROWSER_WIDTH_KEY: &str = "micro-daw:browser-width";
pub const BROWSER_ROOTS_KEY: &str = "micro-daw:browser-roots";
pub const BROWSER_LAST_PATH_KEY: &str = "micro-daw:browser-last-path";
pub const BROWSER_AUTO_PREVIEW_KEY: &str = "micro-daw:browser-auto-preview";
pub const DOCK_PAD_BANK_KEY: &str = "micro-daw:dock-pad-bank";
pub const BEAT_PAD_LAYOUT_KEY: &str = "micro-daw:beat-pad-layout";
pub const BEAT_STEP_GRID_KEY: &str = "micro-daw:beat-step-grid";
pub const BEAT_KEY_HINTS_KEY: &str = "micro-daw:beat-key-hints";

/// Every key above.
pub const ALL_KEYS: [&str; 19] = [
    ARRANGEMENT_STORAGE_KEY,
    RECENT_PROJECTS_KEY,
    SETTINGS_STORAGE_KEY,
    SAMPLE_LIBRARY_STORAGE_KEY,
    PIANO_ROLL_KEY,
    RECORDINGS_STORAGE_KEY,
    CONTROLLER_STORAGE_KEY,
    KITS_KEY,
    PACK_NAMES_KEY,
    MASTER_VOLUME_KEY,
    BROWSER_OPEN_KEY,
    BROWSER_WIDTH_KEY,
    BROWSER_ROOTS_KEY,
    BROWSER_LAST_PATH_KEY,
    BROWSER_AUTO_PREVIEW_KEY,
    DOCK_PAD_BANK_KEY,
    BEAT_PAD_LAYOUT_KEY,
    BEAT_STEP_GRID_KEY,
    BEAT_KEY_HINTS_KEY,
];

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

/// `localStorage`: string values per key, persisted as one JSON object file.
#[derive(Debug, Clone, Default)]
pub struct SettingsStore {
    /// None = in-memory only.
    file: Option<PathBuf>,
    values: Map<String, Value>,
}

impl SettingsStore {
    /// File name inside the settings directory.
    pub const FILE_NAME: &'static str = "local-storage.json";

    /// A store that is never persisted (tests, previews).
    pub fn in_memory() -> Self {
        Self::default()
    }

    /// Opens (or starts) the store in `dir`. A missing file, unreadable JSON or a non-object
    /// document start empty; non-string values in the file are ignored.
    pub fn open(dir: impl AsRef<Path>) -> Self {
        let file = dir.as_ref().join(Self::FILE_NAME);
        let values = fs::read_to_string(&file)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| match v {
                Value::Object(o) => Some(o.into_iter().filter(|(_, v)| v.is_string()).collect()),
                _ => None,
            })
            .unwrap_or_default();
        Self {
            file: Some(file),
            values,
        }
    }

    /// Path of the backing file (None for an in-memory store).
    pub fn file_path(&self) -> Option<&Path> {
        self.file.as_deref()
    }

    /// `localStorage.getItem(key)`.
    pub fn get_item(&self, key: &str) -> Option<&str> {
        self.values.get(key).and_then(Value::as_str)
    }

    /// `localStorage.setItem(key, value)`; persisted immediately.
    pub fn set_item(&mut self, key: &str, value: impl Into<String>) -> io::Result<()> {
        self.values
            .insert(key.to_owned(), Value::String(value.into()));
        self.flush()
    }

    /// `localStorage.removeItem(key)`.
    pub fn remove_item(&mut self, key: &str) -> io::Result<()> {
        if self.values.shift_remove(key).is_some() {
            self.flush()?;
        }
        Ok(())
    }

    /// `localStorage.clear()`.
    pub fn clear(&mut self) -> io::Result<()> {
        self.values.clear();
        self.flush()
    }

    /// Stored keys in insertion order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.values.keys().map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Writes the file atomically (temp file in the same directory, then rename).
    fn flush(&self) -> io::Result<()> {
        let Some(file) = &self.file else {
            return Ok(());
        };
        if let Some(dir) = file.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = file.with_file_name(format!(".{}.tmp-{}", Self::FILE_NAME, std::process::id()));
        let body = serde_json::to_string_pretty(&Value::Object(self.values.clone()))?;
        fs::write(&tmp, body)?;
        fs::rename(&tmp, file).inspect_err(|_| {
            let _ = fs::remove_file(&tmp);
        })
    }
}

/// `JSON.parse(raw)` of a stored value (None when missing or invalid).
fn parse(raw: Option<&str>) -> Option<Value> {
    serde_json::from_str(raw?).ok()
}

// ---------------------------------------------------------------------------
// micro-daw:browser-open, dock bank, beat prefs, auto preview
// ---------------------------------------------------------------------------

/// `readBrowserOpen`: open unless the stored value is exactly `'false'`.
pub fn read_browser_open(s: &SettingsStore) -> bool {
    s.get_item(BROWSER_OPEN_KEY) != Some("false")
}

/// Stores `String(open)`.
pub fn write_browser_open(s: &mut SettingsStore, open: bool) -> io::Result<()> {
    s.set_item(BROWSER_OPEN_KEY, open.to_string())
}

/// Pad bank of the song dock's compact pads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PadBank {
    #[default]
    A,
    B,
}

/// `readBank`: `'B'` → B, anything else A.
pub fn read_dock_pad_bank(s: &SettingsStore) -> PadBank {
    if s.get_item(DOCK_PAD_BANK_KEY) == Some("B") {
        PadBank::B
    } else {
        PadBank::A
    }
}

pub fn write_dock_pad_bank(s: &mut SettingsStore, bank: PadBank) -> io::Result<()> {
    s.set_item(
        DOCK_PAD_BANK_KEY,
        if bank == PadBank::B { "B" } else { "A" },
    )
}

/// `readPref(key, allowed, fallback)`: the stored value when it is one of `allowed`.
pub fn read_pref<'a>(
    s: &SettingsStore,
    key: &str,
    allowed: &[&'a str],
    fallback: &'a str,
) -> &'a str {
    match s.get_item(key) {
        Some(v) if !v.is_empty() => allowed
            .iter()
            .copied()
            .find(|a| *a == v)
            .unwrap_or(fallback),
        _ => fallback,
    }
}

pub const BEAT_PAD_LAYOUTS: [&str; 2] = ["4x4", "2x8"];
pub const BEAT_STEP_GRIDS: [&str; 3] = ["1/8", "1/16", "1/32"];

/// Beat view pad layout: `'4x4'` (default) or `'2x8'`.
pub fn read_beat_pad_layout(s: &SettingsStore) -> &'static str {
    read_pref(s, BEAT_PAD_LAYOUT_KEY, &BEAT_PAD_LAYOUTS, "4x4")
}

pub fn write_beat_pad_layout(s: &mut SettingsStore, layout: &str) -> io::Result<()> {
    s.set_item(BEAT_PAD_LAYOUT_KEY, layout)
}

/// Beat view step grid: `'1/8'`, `'1/16'` (default) or `'1/32'`.
pub fn read_beat_step_grid(s: &SettingsStore) -> &'static str {
    read_pref(s, BEAT_STEP_GRID_KEY, &BEAT_STEP_GRIDS, "1/16")
}

pub fn write_beat_step_grid(s: &mut SettingsStore, grid: &str) -> io::Result<()> {
    s.set_item(BEAT_STEP_GRID_KEY, grid)
}

/// Beat view key hints shown (`readPref(KEYS_KEY, ['on','off'], 'off') === 'on'`).
pub fn read_beat_key_hints(s: &SettingsStore) -> bool {
    read_pref(s, BEAT_KEY_HINTS_KEY, &["on", "off"], "off") == "on"
}

pub fn write_beat_key_hints(s: &mut SettingsStore, show: bool) -> io::Result<()> {
    s.set_item(BEAT_KEY_HINTS_KEY, if show { "on" } else { "off" })
}

/// Sample browser auto-preview: on unless the stored value is exactly `'0'`.
pub fn read_browser_auto_preview(s: &SettingsStore) -> bool {
    s.get_item(BROWSER_AUTO_PREVIEW_KEY) != Some("0")
}

pub fn write_browser_auto_preview(s: &mut SettingsStore, on: bool) -> io::Result<()> {
    s.set_item(BROWSER_AUTO_PREVIEW_KEY, if on { "1" } else { "0" })
}

// ---------------------------------------------------------------------------
// micro-daw:masterVolume, micro-daw:browser-width
// ---------------------------------------------------------------------------

pub const DEFAULT_VOLUME: f64 = -12.0;
pub const MIN_VOLUME: f64 = -40.0;
pub const MAX_VOLUME: f64 = 0.0;

/// `clampVolume`: non-finite → default, else clamped to -40..0.
pub fn clamp_volume(db: f64) -> f64 {
    if db.is_finite() {
        db.clamp(MIN_VOLUME, MAX_VOLUME)
    } else {
        DEFAULT_VOLUME
    }
}

/// `loadStoredVolume`: missing → -12; `Number(raw)` finite → clamped, else -12.
/// JS quirk kept: an empty (or blank) string is `Number('') === 0`, i.e. 0 dB.
pub fn read_master_volume(s: &SettingsStore) -> f64 {
    let v = s
        .get_item(MASTER_VOLUME_KEY)
        .map(js::string_to_number)
        .unwrap_or(f64::NAN);
    if v.is_finite() {
        clamp_volume(v)
    } else {
        DEFAULT_VOLUME
    }
}

/// Stores `String(clampVolume(db))`; returns the stored value.
pub fn write_master_volume(s: &mut SettingsStore, db: f64) -> io::Result<f64> {
    let v = clamp_volume(db);
    s.set_item(MASTER_VOLUME_KEY, js::number_to_string(v))?;
    Ok(v)
}

pub const BROWSER_MIN_WIDTH: f64 = 200.0;
pub const BROWSER_MAX_WIDTH: f64 = 520.0;
pub const BROWSER_DEFAULT_WIDTH: f64 = 248.0;

/// `clampBrowserWidth`: finite → rounded and clamped to 200..520, else 248.
pub fn clamp_browser_width(w: f64) -> f64 {
    if w.is_finite() {
        js::round(w.clamp(BROWSER_MIN_WIDTH, BROWSER_MAX_WIDTH))
    } else {
        BROWSER_DEFAULT_WIDTH
    }
}

/// `readWidth`: `Number(getItem())` (null → 0); falsy (0, NaN) → 248, else clamped.
pub fn read_browser_width(s: &SettingsStore) -> f64 {
    let v = s
        .get_item(BROWSER_WIDTH_KEY)
        .map(js::string_to_number)
        .unwrap_or(0.0);
    if v == 0.0 || v.is_nan() {
        BROWSER_DEFAULT_WIDTH
    } else {
        clamp_browser_width(v)
    }
}

/// Stores `String(w)` (the caller passes a clamped width, as the TS does).
pub fn write_browser_width(s: &mut SettingsStore, w: f64) -> io::Result<()> {
    s.set_item(BROWSER_WIDTH_KEY, js::number_to_string(w))
}

// ---------------------------------------------------------------------------
// micro-daw:browser-roots, micro-daw:browser-last-path
// ---------------------------------------------------------------------------

/// `normalizeRoots`: an array's non-blank strings, trailing separators trimmed, de-duplicated.
pub fn normalize_roots(value: &Value) -> Vec<String> {
    let Value::Array(a) = value else {
        return vec![];
    };
    let mut out: Vec<String> = Vec::new();
    for v in a {
        let Value::String(v) = v else { continue };
        if js::trim(v).is_empty() {
            continue;
        }
        let p = trim_trailing_sep(v);
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// `loadRoots`: missing / empty / invalid JSON → []; else `normalizeRoots`.
pub fn read_browser_roots(s: &SettingsStore) -> Vec<String> {
    match s.get_item(BROWSER_ROOTS_KEY) {
        Some(raw) if !raw.is_empty() => parse(Some(raw))
            .map(|v| normalize_roots(&v))
            .unwrap_or_default(),
        _ => vec![],
    }
}

/// Stores `JSON.stringify(roots)`.
pub fn write_browser_roots(s: &mut SettingsStore, roots: &[String]) -> io::Result<()> {
    s.set_item(
        BROWSER_ROOTS_KEY,
        serde_json::to_string(roots).unwrap_or_else(|_| "[]".into()),
    )
}

/// `isWithin(root, p)` (browserHelpers.ts).
pub fn is_within(root: &str, p: &str) -> bool {
    let r = trim_trailing_sep(root);
    let q = trim_trailing_sep(p);
    if q == r {
        return true;
    }
    let prefix = if r.ends_with('/') || r.ends_with('\\') {
        r.to_owned()
    } else {
        format!("{r}{}", separator_of(&r))
    };
    q.starts_with(&prefix)
}

/// `findRootFor(roots, p)`: the longest root containing `p`.
pub fn find_root_for<'a>(roots: &'a [String], p: &str) -> Option<&'a str> {
    let mut best: Option<&str> = None;
    for root in roots {
        if is_within(root, p)
            && best
                .is_none_or(|b| js::len(&trim_trailing_sep(root)) > js::len(&trim_trailing_sep(b)))
        {
            best = Some(root);
        }
    }
    best
}

/// `loadLastPath(roots)`: the stored path when non-empty and under one of `roots`.
pub fn read_browser_last_path(s: &SettingsStore, roots: &[String]) -> Option<String> {
    let last = s
        .get_item(BROWSER_LAST_PATH_KEY)
        .filter(|l| !l.is_empty())?;
    find_root_for(roots, last).map(|_| last.to_owned())
}

/// Stores the path, or removes the key for None.
pub fn write_browser_last_path(s: &mut SettingsStore, path: Option<&str>) -> io::Result<()> {
    match path {
        Some(p) => s.set_item(BROWSER_LAST_PATH_KEY, p),
        None => s.remove_item(BROWSER_LAST_PATH_KEY),
    }
}

// ---------------------------------------------------------------------------
// micro-daw-piano-roll
// ---------------------------------------------------------------------------

/// `DEFAULT_NOTE_DURATION` (pianoRollHelpers.ts): 1 beat.
pub const DEFAULT_NOTE_DURATION: f64 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PianoRollPrefs {
    pub snap: f64,
    pub draw_length: f64,
    pub follow: bool,
}

impl Default for PianoRollPrefs {
    fn default() -> Self {
        Self {
            snap: 0.25,
            draw_length: DEFAULT_NOTE_DURATION,
            follow: true,
        }
    }
}

/// `loadSettings` of the piano roll: each field kept when valid (`snap` number ≥ 0,
/// `drawLength` number > 0, `follow` boolean), else its default. Missing, empty, invalid JSON
/// or `null` → all defaults.
pub fn read_piano_roll_prefs(s: &SettingsStore) -> PianoRollPrefs {
    let d = PianoRollPrefs::default();
    let Some(raw) = s.get_item(PIANO_ROLL_KEY).filter(|r| !r.is_empty()) else {
        return d;
    };
    let Some(v) = parse(Some(raw)) else { return d };
    if v.is_null() {
        return d; // `s.snap` on null throws → defaults
    }
    let field = |k: &str| v.as_object().and_then(|o| o.get(k));
    PianoRollPrefs {
        snap: field("snap")
            .and_then(Value::as_f64)
            .filter(|x| *x >= 0.0)
            .unwrap_or(d.snap),
        draw_length: field("drawLength")
            .and_then(Value::as_f64)
            .filter(|x| *x > 0.0)
            .unwrap_or(d.draw_length),
        follow: field("follow").and_then(Value::as_bool).unwrap_or(d.follow),
    }
}

/// Stores `JSON.stringify({ snap, drawLength, follow })`.
pub fn write_piano_roll_prefs(s: &mut SettingsStore, p: &PianoRollPrefs) -> io::Result<()> {
    let mut o = Map::new();
    o.insert("snap".into(), js::number_value(p.snap));
    o.insert("drawLength".into(), js::number_value(p.draw_length));
    o.insert("follow".into(), Value::Bool(p.follow));
    s.set_item(PIANO_ROLL_KEY, Value::Object(o).to_string())
}

// ---------------------------------------------------------------------------
// micro-daw-settings
// ---------------------------------------------------------------------------

/// `DEFAULT_SETTINGS`.
pub fn default_app_settings() -> AppSettings {
    AppSettings {
        general: GeneralSettings {
            auto_check_updates: true,
        },
        audio: AudioSettings {
            default_master_volume: -12.0,
            buffer_size_hint: 256,
        },
        midi: MidiSettings {
            auto_connect_last_device: true,
        },
    }
}

fn default_settings_value() -> Value {
    serde_json::json!({
        "general": { "autoCheckUpdates": true },
        "audio": { "defaultMasterVolume": -12, "bufferSizeHint": 256 },
        "midi": { "autoConnectLastDevice": true },
    })
}

/// `loadSettings` of useSettings, as the exact JS object it builds:
/// `general.autoCheckUpdates` = stored `?? true` (any non-null type is kept), `audio` / `midi`
/// = defaults spread with the stored object (unknown keys and wrong types survive; strings and
/// arrays spread their indices — JS quirks kept). Missing / empty value, invalid JSON or a
/// stored `null` → the defaults.
pub fn read_app_settings_value(s: &SettingsStore) -> Value {
    let Some(raw) = s.get_item(SETTINGS_STORAGE_KEY).filter(|r| !r.is_empty()) else {
        return default_settings_value();
    };
    let Some(parsed) = parse(Some(raw)) else {
        return default_settings_value();
    };
    if parsed.is_null() {
        return default_settings_value(); // `parsed.general` on null throws → defaults
    }
    let d = default_settings_value();
    let get = |k: &str| parsed.as_object().and_then(|o| o.get(k));
    let auto = get("general")
        .and_then(|g| g.as_object())
        .and_then(|g| g.get("autoCheckUpdates"))
        .filter(|v| !v.is_null());
    let mut general = Map::new();
    general.insert(
        "autoCheckUpdates".into(),
        auto.cloned().unwrap_or(Value::Bool(true)),
    );
    let spread = |key: &str| -> Value {
        let mut m = d[key].as_object().cloned().unwrap_or_default();
        if let Some(v) = get(key) {
            js::spread_into(&mut m, v);
        }
        Value::Object(m)
    };
    let mut out = Map::new();
    out.insert("general".into(), Value::Object(general));
    out.insert("audio".into(), spread("audio"));
    out.insert("midi".into(), spread("midi"));
    Value::Object(out)
}

/// Typed view of [`read_app_settings_value`]: a field whose stored value has the wrong type
/// (which the TS would carry along untyped) reads as its default.
pub fn read_app_settings(s: &SettingsStore) -> AppSettings {
    app_settings_from_value(&read_app_settings_value(s))
}

fn app_settings_from_value(v: &Value) -> AppSettings {
    let d = default_app_settings();
    AppSettings {
        general: GeneralSettings {
            auto_check_updates: v["general"]["autoCheckUpdates"]
                .as_bool()
                .unwrap_or(d.general.auto_check_updates),
        },
        audio: AudioSettings {
            default_master_volume: v["audio"]["defaultMasterVolume"]
                .as_f64()
                .unwrap_or(d.audio.default_master_volume),
            buffer_size_hint: v["audio"]["bufferSizeHint"]
                .as_f64()
                .filter(|x| x.fract() == 0.0 && *x >= 0.0 && *x <= u32::MAX as f64)
                .map(|x| x as u32)
                .unwrap_or(d.audio.buffer_size_hint),
        },
        midi: MidiSettings {
            auto_connect_last_device: v["midi"]["autoConnectLastDevice"]
                .as_bool()
                .unwrap_or(d.midi.auto_connect_last_device),
        },
    }
}

/// `updateSettings(partial)`: `{ general: {...prev.general, ...partial.general}, audio: …,
/// midi: … }` from the stored settings, saved as JSON. Returns the new settings object.
pub fn update_app_settings(s: &mut SettingsStore, partial: &Value) -> io::Result<Value> {
    let prev = read_app_settings_value(s);
    let mut out = Map::new();
    for key in ["general", "audio", "midi"] {
        let mut m = prev[key].as_object().cloned().unwrap_or_default();
        if let Some(v) = partial.get(key) {
            js::spread_into(&mut m, v);
        }
        out.insert(key.into(), Value::Object(m));
    }
    let next = Value::Object(out);
    s.set_item(SETTINGS_STORAGE_KEY, next.to_string())?;
    Ok(next)
}

/// Stores typed settings (`saveSettings`).
pub fn write_app_settings(s: &mut SettingsStore, settings: &AppSettings) -> io::Result<()> {
    let mut v = serde_json::to_value(settings).unwrap_or(Value::Null);
    v["audio"]["defaultMasterVolume"] = js::number_value(settings.audio.default_master_volume);
    s.set_item(SETTINGS_STORAGE_KEY, v.to_string())
}

// ---------------------------------------------------------------------------
// micro-daw-sample-library
// ---------------------------------------------------------------------------

/// `SerializedEntry` of the sample library.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedSampleEntry {
    pub sample_id: String,
    pub sample_path: String,
    pub sample_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trim: Option<crate::types::SampleTrim>,
    pub added_at: f64,
}

/// The stored entries the library loader walks (`for (const entry of serialized)`), as raw
/// JSON (each is handed to the sample loader, which skips entries that fail to load).
/// Missing / empty value, invalid JSON or a non-array → none. JS quirk kept: a `null` /
/// missing element makes the TS loop throw, so nothing loads at all.
pub fn read_sample_library(s: &SettingsStore) -> Vec<Value> {
    let Some(Value::Array(a)) = parse(
        s.get_item(SAMPLE_LIBRARY_STORAGE_KEY)
            .filter(|r| !r.is_empty()),
    ) else {
        return vec![];
    };
    if a.iter().any(Value::is_null) {
        return vec![];
    }
    a
}

/// `persist(list)`: entries with an empty path are left out.
pub fn write_sample_library(
    s: &mut SettingsStore,
    entries: &[SerializedSampleEntry],
) -> io::Result<()> {
    let kept: Vec<Value> = entries
        .iter()
        .filter(|e| !e.sample_path.is_empty())
        .map(|e| {
            let mut v = serde_json::to_value(e).unwrap_or(Value::Null);
            v["addedAt"] = js::number_value(e.added_at);
            v
        })
        .collect();
    s.set_item(SAMPLE_LIBRARY_STORAGE_KEY, Value::Array(kept).to_string())
}

// ---------------------------------------------------------------------------
// micro-daw-recent-projects
// ---------------------------------------------------------------------------

pub const MAX_RECENT: usize = 10;

/// `loadRecentProjects`: None when the key is missing or empty (the TS keeps its current list,
/// `[]` at start-up); `Some([])` when the JSON is invalid (the `catch` resets the list);
/// otherwise the parsed value as-is, unvalidated (it may be `null`, a number, …, as in TS).
pub fn read_recent_projects_raw(s: &SettingsStore) -> Option<Value> {
    let raw = s.get_item(RECENT_PROJECTS_KEY).filter(|r| !r.is_empty())?;
    Some(serde_json::from_str(raw).unwrap_or(Value::Array(vec![])))
}

/// Typed recent projects: the stored array's elements that are `{filePath, name, modifiedAt}`
/// string records (others skipped).
pub fn read_recent_projects(s: &SettingsStore) -> Vec<ProjectMeta> {
    match read_recent_projects_raw(s) {
        Some(Value::Array(a)) => a
            .into_iter()
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect(),
        _ => vec![],
    }
}

/// `addToRecentProjects`: drops `file_path`, prepends it (name = last `/` segment without
/// `.mdaw`, or 'Untitled'), keeps 10.
pub fn add_recent_project(
    list: &[ProjectMeta],
    file_path: &str,
    modified_at: &str,
) -> Vec<ProjectMeta> {
    let mut out: Vec<ProjectMeta> = list
        .iter()
        .filter(|p| p.file_path != file_path)
        .cloned()
        .collect();
    let last = file_path.rsplit('/').next().unwrap_or("");
    let name = last.strip_suffix(".mdaw").unwrap_or(last);
    let name = if name.is_empty() { "Untitled" } else { name };
    out.insert(
        0,
        ProjectMeta {
            file_path: file_path.into(),
            name: name.into(),
            modified_at: modified_at.into(),
        },
    );
    out.truncate(MAX_RECENT);
    out
}

pub fn write_recent_projects(s: &mut SettingsStore, list: &[ProjectMeta]) -> io::Result<()> {
    s.set_item(
        RECENT_PROJECTS_KEY,
        serde_json::to_string(list).unwrap_or_else(|_| "[]".into()),
    )
}

// ---------------------------------------------------------------------------
// micro-daw-arrangement, micro-daw:controller
// ---------------------------------------------------------------------------

/// `loadFromLocalStorage`'s `JSON.parse(raw)`: None when missing, empty or invalid. The caller
/// runs `migrateArrangement` on it (migrate.rs).
pub fn read_arrangement(s: &SettingsStore) -> Option<Value> {
    parse(
        s.get_item(ARRANGEMENT_STORAGE_KEY)
            .filter(|r| !r.is_empty()),
    )
}

/// `saveToLocalStorage`: `JSON.stringify(arrangement)`.
pub fn write_arrangement<T: Serialize>(s: &mut SettingsStore, arrangement: &T) -> io::Result<()> {
    let body = serde_json::to_string(arrangement).map_err(io::Error::other)?;
    s.set_item(ARRANGEMENT_STORAGE_KEY, body)
}

/// The raw `micro-daw:controller` string (parsed and validated by `controller.rs`).
pub fn read_controller_raw(s: &SettingsStore) -> Option<&str> {
    s.get_item(CONTROLLER_STORAGE_KEY)
}

pub fn write_controller_raw(s: &mut SettingsStore, raw: &str) -> io::Result<()> {
    s.set_item(CONTROLLER_STORAGE_KEY, raw)
}

// ---------------------------------------------------------------------------
// micro-daw:pack-names
// ---------------------------------------------------------------------------

/// `readRememberedNames` (see [`crate::packs::service::parse_remembered_names`]).
pub fn read_pack_names(s: &SettingsStore) -> RememberedNames {
    parse_remembered_names(s.get_item(PACK_NAMES_KEY))
}

pub fn write_pack_names(s: &mut SettingsStore, names: &RememberedNames) -> io::Result<()> {
    s.set_item(PACK_NAMES_KEY, serialize_remembered_names(names))
}

// ---------------------------------------------------------------------------
// micro-daw:kits (KitStore)
// ---------------------------------------------------------------------------

/// `KitStore.read()` before normalization: the stored array's truthy object elements (arrays
/// included: `typeof [] === 'object'`). Missing / empty / invalid / non-array → none.
pub fn read_kits_raw(s: &SettingsStore) -> Vec<Value> {
    match parse(s.get_item(KITS_KEY).filter(|r| !r.is_empty())) {
        Some(Value::Array(a)) => a
            .into_iter()
            .filter(|k| k.is_object() || k.is_array())
            .collect(),
        _ => vec![],
    }
}

/// `kitStore.list()`: [`read_kits_raw`] mapped through `normalize_kit` (`normalizeKit`, owned
/// by `kit.rs`; it assigns a fresh uuid to a kit without an id on every read, as in TS).
pub fn read_kits(s: &SettingsStore, normalize_kit: impl Fn(&Value) -> KitState) -> Vec<KitState> {
    read_kits_raw(s).iter().map(normalize_kit).collect()
}

/// `forStorage`: a normalized copy with every file source's runtime `sampleId` stripped.
pub fn kit_for_storage(kit: &KitState, normalize_kit: &impl Fn(&Value) -> KitState) -> KitState {
    let mut copy = normalize_kit(&serde_json::to_value(kit).unwrap_or(Value::Null));
    for pad in &mut copy.pads {
        if let Some(PadSampleSource::File { sample_id, .. }) =
            pad.sample.as_mut().map(|s| &mut s.source)
        {
            *sample_id = None;
        }
    }
    copy
}

fn write_kits(s: &mut SettingsStore, kits: &[KitState]) -> io::Result<()> {
    s.set_item(
        KITS_KEY,
        serde_json::to_string(kits).map_err(io::Error::other)?,
    )
}

/// `kitStore.save(kit)`: insert or replace by id (after normalizing every stored kit).
pub fn save_kit(
    s: &mut SettingsStore,
    kit: &KitState,
    normalize_kit: impl Fn(&Value) -> KitState,
) -> io::Result<()> {
    let clean = kit_for_storage(kit, &normalize_kit);
    let mut kits = read_kits(s, &normalize_kit);
    match kits.iter().position(|k| k.id == clean.id) {
        Some(i) => kits[i] = clean,
        None => kits.push(clean),
    }
    write_kits(s, &kits)
}

/// `kitStore.remove(id)`: writes only when a kit was removed; returns whether one was.
pub fn remove_kit(
    s: &mut SettingsStore,
    id: &str,
    normalize_kit: impl Fn(&Value) -> KitState,
) -> io::Result<bool> {
    let kits = read_kits(s, &normalize_kit);
    let next: Vec<KitState> = kits.iter().filter(|k| k.id != id).cloned().collect();
    if next.len() == kits.len() {
        return Ok(false);
    }
    write_kits(s, &next)?;
    Ok(true)
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
