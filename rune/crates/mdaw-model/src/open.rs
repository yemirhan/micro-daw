//! Opening a project / stored arrangement: JSON → TS migration → **coercion** into the typed model.
//!
//! The Electron app never validates what it loads: `JSON.parse` output goes through
//! `migrateArrangement` and is then used as is, so a file with `"volume": null`, a note without
//! `velocity`, `"lengthBeats": "100"` or an unknown filter type still opens and plays (JS coerces
//! at the point of use, or a `??` fallback kicks in). A strict `serde` decode would reject those
//! files, and the Rust app would refuse projects the Electron app opens.
//!
//! [`open_project`] (files), [`open_arrangement_value`] (the autosave in
//! `ArrangementDoc::load_from_local_storage`, and anything else that starts from untyped JSON)
//! are the single entry points: they run the `Value`-level migration (exact TS semantics), then
//! [`coerce_arrangement`] maps the migrated value onto the typed model with the rules below, and
//! collect a [`CoerceWarning`] for every value that had to be changed. Well-formed data decodes
//! exactly as `serde` would (`coerce_matches_serde_on_well_formed_data`).
//!
//! # Coercion rules
//!
//! | input                                    | result                                         | TS behaviour it mirrors |
//! |------------------------------------------|------------------------------------------------|-------------------------|
//! | number field: JSON number                | as is                                          | — |
//! | number field: string / bool / array      | JS `ToNumber` ([`crate::js::to_number`]), warn | arithmetic / comparisons coerce (`'100' >= 8`, `region.startBeat + x * 1`) |
//! | number field: null, missing, object, or `ToNumber` non-finite | the field default, warn (not for missing) | non-finite numbers are written as `null` by `JSON.stringify` (`setBpm(Infinity)`, `setTrackVolume(NaN)` are unchecked), so a later open must still work; the defaults are the values the TS falls back to (below) |
//! | string field: number / bool              | JS `String(v)`, warn                           | ids are compared and rendered as strings |
//! | string field: null / missing / object / array | field default (`""` or a fresh id), warn unless missing | — |
//! | bool field                               | JS truthiness (warn when not a bool / null)    | every TS read is `if (x)` / `x ? :` |
//! | enum field, unknown value                | dropped to the default, warn                   | `?? 'lowpass'` / `?? 'sine'` fallbacks; unknown instrument types play as synth (`createTrackAudio`) |
//! | list element of the wrong type           | element dropped, warn                          | — |
//!
//! Field defaults (and where they come from):
//! - `arrangement.bpm` → 120 (`DEFAULT_BPM`); `lengthBeats` → 64 (`DEFAULT_ARRANGEMENT_LENGTH`);
//!   `id` / track / region / marker / group `id` → a fresh id (`new_id`); names → `""`;
//!   colors → `""` (an `undefined` CSS colour paints nothing; the Rust UI treats `""` the same).
//! - `track.volume` / `pan` → 0 (`migrateTrack` `pan ??= 0`; Tone `Volume(undefined)` = 0 dB);
//!   `muted` / `solo` → false; `instrument` not an object → `{ type: 'synth', presetIndex: 0 }`;
//!   `presetIndex` not an integer (e.g. 2.5) → 0 (`SYNTH_PRESETS[2.5] ?? SYNTH_PRESETS[0]`).
//! - `effects.*` per field → `DEFAULT_TRACK_EFFECTS` (the v1.2/v1.3 `filter` shape lacks
//!   `type` / `lfo*`; Electron reads them with `?? 'lowpass'` / `?? 1` / `?? 'sine'` / `?? 0`).
//! - automation lane with an unknown `parameter` → lane dropped (the automation switch ignores
//!   it); a point with a non-numeric `beat` / `value` → point dropped; `visible` → truthiness
//!   (`lanes.filter(l => l.visible)`, so a missing flag hides the lane).
//! - region `startBeat` → 0, `lengthBeats` → 4 (one bar); `audio` falsy → none, not an object →
//!   dropped; audio `sampleId` / `samplePath` → `""` (the region then shows as unavailable),
//!   `offsetSeconds` / `gainDb` / `originalDuration` → 0.
//! - note: non-numeric `note` or `startBeat` → note dropped (Tone cannot schedule / pitch it);
//!   `velocity` → 0.8 (`normVelocity(undefined)` in src/audio/voices/common.ts);
//!   `durationBeats` → 0.25; `isDrum` → truthiness. Unknown note keys are dropped (see
//!   `RegionNote`).
//! - marker `beat` → 0; group `trackIds` not an array → `[]`, non-string ids are `String()`ed.
//! - `beat`: [`crate::loop_settings`] normalization (the TS already coerces it), active id
//!   `String()`ed.
//! - a non-drum track's garbage `kit` (drum kits are normalized by the migration) → dropped.
//! - `markers` / `groups` / `automation` / `synthOverrides` that are not arrays / objects → dropped.
//!
//! After coercion the beat state and loop markers are normalized again
//! ([`crate::migrate::migrate_beat_and_loop_markers`]) because coercion may have changed track
//! ids; this makes every opened arrangement a fixpoint of open → save → open.
//!
//! Unknown keys are kept everywhere except on notes (`extra` maps).

use serde_json::{Map, Value};

use crate::js;
use crate::migrate::{
    DroppedDrumNotes, LoadedProject, ManifestLoad, MigrateError, ProjectLoadError, RestoredAudio,
    load_project_json, migrate_arrangement_value, migrate_beat_and_loop_markers,
};
use crate::sound_design::DesignedSound;
use crate::types::{
    Arrangement, AudioRegionData, AutomationLane, AutomationParameter, AutomationPoint, BeatState,
    ChorusFx, CompressorFx, DelayFx, DistortionFx, EqFx, FilterFx, FilterType, InstrumentType,
    KitState, LfoWaveform, LoopMarkers, Marker, PAD_COUNT, Region, RegionNote, ReverbFx, Track,
    TrackEffectState, TrackGroup, TrackInstrument,
};

/// `DEFAULT_BPM`.
const DEFAULT_BPM: f64 = 120.0;
/// `DEFAULT_ARRANGEMENT_LENGTH`.
const DEFAULT_LENGTH: f64 = 64.0;
/// `normVelocity(undefined)`.
const DEFAULT_VELOCITY: f64 = 0.8;
const DEFAULT_NOTE_LENGTH: f64 = 0.25;
const DEFAULT_REGION_LENGTH: f64 = 4.0;

/// A value that had to be coerced, defaulted or dropped while opening.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoerceWarning {
    /// JSON path, e.g. `$.tracks[0].volume`.
    pub path: String,
    pub message: String,
}

impl std::fmt::Display for CoerceWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

/// A migrated and coerced arrangement.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenedArrangement {
    pub arrangement: Arrangement,
    /// The migration's `console.warn`s.
    pub dropped_drum_notes: Vec<DroppedDrumNotes>,
    pub warnings: Vec<CoerceWarning>,
}

/// Everything `loadFromFile` derives from a project file, with the arrangement typed.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenedProject {
    pub version_warning: Option<String>,
    pub sample_manifest: Vec<ManifestLoad>,
    pub kits_to_preload: Vec<KitState>,
    /// Migrated + coerced; hand it to `ArrangementDoc::load_project` (whose re-migration is then
    /// a no-op).
    pub arrangement: Arrangement,
    pub dropped_drum_notes: Vec<DroppedDrumNotes>,
    pub audio: Option<RestoredAudio>,
    pub created_at: Option<String>,
    pub modified_at: Option<String>,
    pub warnings: Vec<CoerceWarning>,
}

/// Open a `.mdaw` file: parse, `loadFromFile` (+ kit preload + migration, TS order of `new_id`
/// draws), then [`coerce_arrangement`]. Errors are the cases where the Electron app fails too
/// (invalid JSON, or a shape its migration throws a TypeError on).
pub fn open_project(
    raw: &str,
    packs_dir: Option<&str>,
    new_id: &mut dyn FnMut() -> String,
) -> Result<OpenedProject, ProjectLoadError> {
    let LoadedProject {
        version_warning,
        sample_manifest,
        kits_to_preload,
        arrangement,
        dropped_drum_notes,
        audio,
        created_at,
        modified_at,
    } = load_project_json(raw, packs_dir, new_id)?;
    let mut warnings = Vec::new();
    let arrangement = coerce_arrangement(&arrangement, new_id, &mut warnings);
    Ok(OpenedProject {
        version_warning,
        sample_manifest,
        kits_to_preload,
        arrangement,
        dropped_drum_notes,
        audio,
        created_at,
        modified_at,
        warnings,
    })
}

/// Migrate an untyped arrangement (`migrateArrangement`, in place) and coerce it.
pub fn open_arrangement_value(
    mut value: Value,
    packs_dir: Option<&str>,
    new_id: &mut dyn FnMut() -> String,
) -> Result<OpenedArrangement, MigrateError> {
    let dropped_drum_notes = migrate_arrangement_value(&mut value, packs_dir, new_id)?;
    let mut warnings = Vec::new();
    let arrangement = coerce_arrangement(&value, new_id, &mut warnings);
    Ok(OpenedArrangement {
        arrangement,
        dropped_drum_notes,
        warnings,
    })
}

/// Map an (already migrated) arrangement value onto the typed model; never fails (see the
/// module docs for the rules).
pub fn coerce_arrangement(
    v: &Value,
    new_id: &mut dyn FnMut() -> String,
    warnings: &mut Vec<CoerceWarning>,
) -> Arrangement {
    let mut cx = Cx { warnings, new_id };
    let mut a = cx.arrangement(v, "$");
    migrate_beat_and_loop_markers(&mut a);
    a
}

struct Cx<'a> {
    warnings: &'a mut Vec<CoerceWarning>,
    new_id: &'a mut dyn FnMut() -> String,
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

fn extra(m: &Map<String, Value>, known: &[&str]) -> Map<String, Value> {
    m.iter()
        .filter(|(k, _)| !known.contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

fn field(path: &str, key: &str) -> String {
    format!("{path}.{key}")
}

impl Cx<'_> {
    fn warn(&mut self, path: &str, message: impl Into<String>) {
        self.warnings.push(CoerceWarning {
            path: path.to_owned(),
            message: message.into(),
        });
    }

    /// A number, or None for missing / null / garbage (warned, except missing).
    fn num_opt(&mut self, v: Option<&Value>, path: &str) -> Option<f64> {
        match v {
            None => None,
            Some(Value::Number(n)) => n.as_f64().filter(|f| f.is_finite()),
            Some(Value::Null) => {
                self.warn(path, "null");
                None
            }
            Some(Value::Object(_)) => {
                self.warn(path, "an object is not a number");
                None
            }
            Some(x) => {
                let n = js::to_number(Some(x));
                if n.is_finite() {
                    self.warn(path, format!("{} coerced to the number {n}", type_name(x)));
                    Some(n)
                } else {
                    self.warn(
                        path,
                        format!("{} is not a finite number ({x})", type_name(x)),
                    );
                    None
                }
            }
        }
    }

    fn num(&mut self, v: Option<&Value>, path: &str, default: f64) -> f64 {
        let n = self.num_opt(v, path);
        if n.is_none() && v.is_some() {
            self.warn(path, format!("using the default {default}"));
        }
        n.unwrap_or(default)
    }

    /// A string, `String(v)` for numbers / booleans, None for missing / null / garbage.
    fn text_opt(&mut self, v: Option<&Value>, path: &str) -> Option<String> {
        match v {
            None => None,
            Some(Value::String(s)) => Some(s.clone()),
            Some(x @ (Value::Number(_) | Value::Bool(_))) => {
                let s = js::string_of(Some(x));
                self.warn(path, format!("{} used as the string {s:?}", type_name(x)));
                Some(s)
            }
            Some(x) => {
                self.warn(path, format!("{} is not a string", type_name(x)));
                None
            }
        }
    }

    fn text(&mut self, v: Option<&Value>, path: &str, default: &str) -> String {
        self.text_opt(v, path).unwrap_or_else(|| default.to_owned())
    }

    fn id(&mut self, v: Option<&Value>, path: &str) -> String {
        match self.text_opt(v, path) {
            Some(id) => id,
            None => {
                self.warn(path, "missing id: generated a new one");
                (self.new_id)()
            }
        }
    }

    /// JS truthiness; a present value that is not a boolean (or null) is warned.
    fn flag(&mut self, v: Option<&Value>, path: &str) -> bool {
        if let Some(x) = v
            && !matches!(x, Value::Bool(_) | Value::Null)
        {
            self.warn(path, format!("{} used as a boolean", type_name(x)));
        }
        js::truthy(v)
    }

    /// An optional list: None for missing / null, dropped (warned) when not an array.
    fn list<'v>(&mut self, v: Option<&'v Value>, path: &str) -> Option<&'v [Value]> {
        match v {
            None | Some(Value::Null) => None,
            Some(Value::Array(a)) => Some(a),
            Some(x) => {
                self.warn(path, format!("{} is not a list: dropped", type_name(x)));
                None
            }
        }
    }

    fn object<'v>(&mut self, v: &'v Value, path: &str) -> Option<&'v Map<String, Value>> {
        let o = v.as_object();
        if o.is_none() {
            self.warn(path, format!("{} is not an object: dropped", type_name(v)));
        }
        o
    }

    /// An enum from its JSON string (unknown values → None, warned).
    fn enum_opt<T: serde::de::DeserializeOwned>(
        &mut self,
        v: Option<&Value>,
        path: &str,
    ) -> Option<T> {
        let v = v?;
        let parsed = match v {
            Value::String(_) => serde_json::from_value(v.clone()).ok(),
            _ => None,
        };
        if parsed.is_none() {
            self.warn(path, format!("unknown value {v}"));
        }
        parsed
    }

    // --- Arrangement ----------------------------------------------------------------------------

    /// The Sound Designer's sounds: each one that doesn't parse (or has no id) is dropped with a
    /// warning; recipes are clamped to their ranges.
    fn sounds(&mut self, v: Option<&Value>, path: &str) -> Vec<DesignedSound> {
        let Some(list) = self.list(v, path) else {
            return Vec::new();
        };
        let mut out: Vec<DesignedSound> = Vec::new();
        for (i, x) in list.iter().enumerate() {
            match serde_json::from_value::<DesignedSound>(x.clone()) {
                Ok(mut s) if !s.id.is_empty() && !out.iter().any(|o| o.id == s.id) => {
                    s.recipe = s.recipe.sanitized();
                    out.push(s);
                }
                _ => self.warn(&format!("{path}[{i}]"), "not a valid sound"),
            }
        }
        out
    }

    fn arrangement(&mut self, v: &Value, path: &str) -> Arrangement {
        let empty = Map::new();
        let m = v.as_object().unwrap_or_else(|| {
            self.warn(path, "the arrangement is not an object");
            &empty
        });
        let p = |k: &str| field(path, k);
        let tracks = match m.get("tracks") {
            Some(Value::Array(ts)) => ts
                .iter()
                .enumerate()
                .filter_map(|(i, t)| {
                    let tp = format!("{path}.tracks[{i}]");
                    self.object(t, &tp).map(|o| self.track(o, &tp))
                })
                .collect(),
            other => {
                if other.is_some() {
                    self.warn(&p("tracks"), "not a list");
                }
                Vec::new()
            }
        };
        let markers = self.list(m.get("markers"), &p("markers")).map(|ms| {
            ms.iter()
                .enumerate()
                .filter_map(|(i, x)| {
                    let mp = format!("{path}.markers[{i}]");
                    self.object(x, &mp).map(|o| self.marker(o, &mp))
                })
                .collect()
        });
        let groups = self.list(m.get("groups"), &p("groups")).map(|gs| {
            gs.iter()
                .enumerate()
                .filter_map(|(i, x)| {
                    let gp = format!("{path}.groups[{i}]");
                    self.object(x, &gp).map(|o| self.group(o, &gp))
                })
                .collect()
        });
        let loop_markers = match m.get("loopMarkers") {
            Some(Value::Object(lm)) => {
                let s = self.num_opt(lm.get("startBeat"), &p("loopMarkers.startBeat"));
                let e = self.num_opt(lm.get("endBeat"), &p("loopMarkers.endBeat"));
                // Invalid markers are replaced by migrate_beat_and_loop_markers.
                Some(LoopMarkers {
                    start_beat: s.unwrap_or(f64::NAN),
                    end_beat: e.unwrap_or(f64::NAN),
                })
            }
            _ => None,
        };
        let beat = m.get("beat").map(|b| self.beat(b, &p("beat")));
        Arrangement {
            id: self.id(m.get("id"), &p("id")),
            name: self.text(m.get("name"), &p("name"), ""),
            bpm: self.num(m.get("bpm"), &p("bpm"), DEFAULT_BPM),
            length_beats: self.num(m.get("lengthBeats"), &p("lengthBeats"), DEFAULT_LENGTH),
            tracks,
            loop_markers,
            markers,
            groups,
            beat,
            sounds: self.sounds(m.get("sounds"), &p("sounds")),
            extra: extra(
                m,
                &[
                    "id",
                    "name",
                    "bpm",
                    "lengthBeats",
                    "tracks",
                    "loopMarkers",
                    "markers",
                    "groups",
                    "beat",
                    "sounds",
                ],
            ),
        }
    }

    fn beat(&mut self, v: &Value, path: &str) -> BeatState {
        let mut b = crate::loop_settings::beat_state_from_value(v);
        // beat_state_from_value only takes string ids; a numeric track id is String()ed like
        // the track ids themselves.
        if let Some(x @ Value::Number(_)) = v.get("activeDrumTrackId") {
            b.active_drum_track_id = self.text_opt(Some(x), &field(path, "activeDrumTrackId"));
        }
        debug_assert!(b.selected_pad < PAD_COUNT);
        b
    }

    fn marker(&mut self, m: &Map<String, Value>, path: &str) -> Marker {
        let p = |k: &str| field(path, k);
        Marker {
            id: self.id(m.get("id"), &p("id")),
            name: self.text(m.get("name"), &p("name"), ""),
            beat: self.num(m.get("beat"), &p("beat"), 0.0),
            color: self.text(m.get("color"), &p("color"), ""),
            extra: extra(m, &["id", "name", "beat", "color"]),
        }
    }

    fn group(&mut self, m: &Map<String, Value>, path: &str) -> TrackGroup {
        let p = |k: &str| field(path, k);
        let track_ids = match m.get("trackIds") {
            Some(Value::Array(ids)) => ids
                .iter()
                .enumerate()
                .filter_map(|(i, id)| self.text_opt(Some(id), &format!("{path}.trackIds[{i}]")))
                .collect(),
            other => {
                if other.is_some() {
                    self.warn(&p("trackIds"), "not a list: no tracks");
                }
                Vec::new()
            }
        };
        TrackGroup {
            id: self.id(m.get("id"), &p("id")),
            name: self.text(m.get("name"), &p("name"), ""),
            color: self.text(m.get("color"), &p("color"), ""),
            track_ids,
            collapsed: self.flag(m.get("collapsed"), &field(path, "collapsed")),
            extra: extra(m, &["id", "name", "color", "trackIds", "collapsed"]),
        }
    }

    // --- Track ----------------------------------------------------------------------------------

    fn track(&mut self, m: &Map<String, Value>, path: &str) -> Track {
        let p = |k: &str| field(path, k);
        let instrument = match m.get("instrument") {
            Some(Value::Object(i)) => self.instrument(i, &p("instrument")),
            other => {
                if let Some(x) = other {
                    self.warn(
                        &p("instrument"),
                        format!("{} is not an instrument: synth", type_name(x)),
                    );
                }
                TrackInstrument {
                    kind: InstrumentType::Synth,
                    preset_index: 0,
                    synth_overrides: None,
                    kit: None,
                    extra: Map::new(),
                }
            }
        };
        let regions = match m.get("regions") {
            Some(Value::Array(rs)) => rs
                .iter()
                .enumerate()
                .filter_map(|(i, r)| {
                    let rp = format!("{path}.regions[{i}]");
                    self.object(r, &rp).map(|o| self.region(o, &rp))
                })
                .collect(),
            other => {
                if other.is_some() {
                    self.warn(&p("regions"), "not a list");
                }
                Vec::new()
            }
        };
        let effects = match m.get("effects") {
            None | Some(Value::Null) => None,
            Some(Value::Object(fx)) => Some(self.effects(fx, &p("effects"))),
            Some(x) => {
                self.warn(
                    &p("effects"),
                    format!("{} is not an effects object: defaults", type_name(x)),
                );
                Some(TrackEffectState::default())
            }
        };
        let automation = self.list(m.get("automation"), &p("automation")).map(|ls| {
            ls.iter()
                .enumerate()
                .filter_map(|(i, l)| {
                    let lp = format!("{path}.automation[{i}]");
                    let o = self.object(l, &lp)?;
                    self.lane(o, &lp)
                })
                .collect()
        });
        Track {
            id: self.id(m.get("id"), &p("id")),
            name: self.text(m.get("name"), &p("name"), ""),
            instrument,
            regions,
            volume: self.num(m.get("volume"), &p("volume"), 0.0),
            pan: self.num(m.get("pan"), &p("pan"), 0.0),
            muted: self.flag(m.get("muted"), &field(path, "muted")),
            solo: self.flag(m.get("solo"), &field(path, "solo")),
            color: self.text(m.get("color"), &p("color"), ""),
            effects,
            automation,
            extra: extra(
                m,
                &[
                    "id",
                    "name",
                    "instrument",
                    "regions",
                    "volume",
                    "pan",
                    "muted",
                    "solo",
                    "color",
                    "effects",
                    "automation",
                ],
            ),
        }
    }

    fn instrument(&mut self, m: &Map<String, Value>, path: &str) -> TrackInstrument {
        let p = |k: &str| field(path, k);
        let kind = self
            .enum_opt::<InstrumentType>(m.get("type"), &p("type"))
            .unwrap_or(InstrumentType::Synth);
        let preset = self.num(m.get("presetIndex"), &p("presetIndex"), 0.0);
        let preset_index = if preset.fract() == 0.0 && preset.abs() < 9.0e15 {
            preset as i64
        } else {
            self.warn(&p("presetIndex"), format!("{preset} is not an index: 0"));
            0
        };
        let synth_overrides = match m.get("synthOverrides") {
            None | Some(Value::Null) => None,
            Some(Value::Object(o)) => Some(o.clone()),
            Some(x) => {
                self.warn(
                    &p("synthOverrides"),
                    format!("{} is not an object: dropped", type_name(x)),
                );
                None
            }
        };
        let kit = match m.get("kit") {
            k if !js::truthy(k) => None,
            Some(k) => match serde_json::from_value::<KitState>(k.clone()) {
                Ok(kit) => Some(kit),
                Err(e) => {
                    self.warn(&p("kit"), format!("invalid kit dropped ({e})"));
                    None
                }
            },
            None => None,
        };
        TrackInstrument {
            kind,
            preset_index,
            synth_overrides,
            kit,
            extra: extra(m, &["type", "presetIndex", "synthOverrides", "kit"]),
        }
    }

    fn effects(&mut self, m: &Map<String, Value>, path: &str) -> TrackEffectState {
        let empty = Map::new();
        let sub = |cx: &mut Self, key: &str| -> (Map<String, Value>, String) {
            let sp = field(path, key);
            match m.get(key) {
                None | Some(Value::Null) => (empty.clone(), sp),
                Some(Value::Object(o)) => (o.clone(), sp),
                Some(x) => {
                    cx.warn(&sp, format!("{} is not an object: defaults", type_name(x)));
                    (empty.clone(), sp)
                }
            }
        };
        let (o, sp) = sub(self, "reverb");
        let d = ReverbFx::default();
        let reverb = ReverbFx {
            enabled: self.flag(o.get("enabled"), &field(&sp, "enabled")),
            wet: self.num(o.get("wet"), &field(&sp, "wet"), d.wet),
            extra: extra(&o, &["enabled", "wet"]),
        };
        let (o, sp) = sub(self, "delay");
        let d = DelayFx::default();
        let delay = DelayFx {
            enabled: self.flag(o.get("enabled"), &field(&sp, "enabled")),
            time: self.num(o.get("time"), &field(&sp, "time"), d.time),
            feedback: self.num(o.get("feedback"), &field(&sp, "feedback"), d.feedback),
            wet: self.num(o.get("wet"), &field(&sp, "wet"), d.wet),
            extra: extra(&o, &["enabled", "time", "feedback", "wet"]),
        };
        let (o, sp) = sub(self, "chorus");
        let d = ChorusFx::default();
        let chorus = ChorusFx {
            enabled: self.flag(o.get("enabled"), &field(&sp, "enabled")),
            depth: self.num(o.get("depth"), &field(&sp, "depth"), d.depth),
            extra: extra(&o, &["enabled", "depth"]),
        };
        let (o, sp) = sub(self, "distortion");
        let d = DistortionFx::default();
        let distortion = DistortionFx {
            enabled: self.flag(o.get("enabled"), &field(&sp, "enabled")),
            amount: self.num(o.get("amount"), &field(&sp, "amount"), d.amount),
            wet: self.num(o.get("wet"), &field(&sp, "wet"), d.wet),
            extra: extra(&o, &["enabled", "amount", "wet"]),
        };
        let (o, sp) = sub(self, "eq");
        let d = EqFx::default();
        let eq = EqFx {
            enabled: self.flag(o.get("enabled"), &field(&sp, "enabled")),
            low: self.num(o.get("low"), &field(&sp, "low"), d.low),
            mid: self.num(o.get("mid"), &field(&sp, "mid"), d.mid),
            high: self.num(o.get("high"), &field(&sp, "high"), d.high),
            extra: extra(&o, &["enabled", "low", "mid", "high"]),
        };
        let (o, sp) = sub(self, "compressor");
        let d = CompressorFx::default();
        let compressor = CompressorFx {
            enabled: self.flag(o.get("enabled"), &field(&sp, "enabled")),
            threshold: self.num(o.get("threshold"), &field(&sp, "threshold"), d.threshold),
            ratio: self.num(o.get("ratio"), &field(&sp, "ratio"), d.ratio),
            attack: self.num(o.get("attack"), &field(&sp, "attack"), d.attack),
            release: self.num(o.get("release"), &field(&sp, "release"), d.release),
            extra: extra(&o, &["enabled", "threshold", "ratio", "attack", "release"]),
        };
        let (o, sp) = sub(self, "filter");
        let d = FilterFx::default();
        let filter = FilterFx {
            enabled: self.flag(o.get("enabled"), &field(&sp, "enabled")),
            kind: self
                .enum_opt::<FilterType>(o.get("type"), &field(&sp, "type"))
                .unwrap_or(d.kind),
            cutoff: self.num(o.get("cutoff"), &field(&sp, "cutoff"), d.cutoff),
            resonance: self.num(o.get("resonance"), &field(&sp, "resonance"), d.resonance),
            lfo_rate: self.num(o.get("lfoRate"), &field(&sp, "lfoRate"), d.lfo_rate),
            lfo_depth: self.num(o.get("lfoDepth"), &field(&sp, "lfoDepth"), d.lfo_depth),
            lfo_wave: self
                .enum_opt::<LfoWaveform>(o.get("lfoWave"), &field(&sp, "lfoWave"))
                .unwrap_or(d.lfo_wave),
            extra: extra(
                &o,
                &[
                    "enabled",
                    "type",
                    "cutoff",
                    "resonance",
                    "lfoRate",
                    "lfoDepth",
                    "lfoWave",
                ],
            ),
        };
        TrackEffectState {
            reverb,
            delay,
            chorus,
            distortion,
            eq,
            compressor,
            filter,
            extra: extra(
                m,
                &[
                    "reverb",
                    "delay",
                    "chorus",
                    "distortion",
                    "eq",
                    "compressor",
                    "filter",
                ],
            ),
        }
    }

    fn lane(&mut self, m: &Map<String, Value>, path: &str) -> Option<AutomationLane> {
        let pp = field(path, "parameter");
        let Some(parameter) = self.enum_opt::<AutomationParameter>(m.get("parameter"), &pp) else {
            self.warn(path, "automation lane without a known parameter: dropped");
            return None;
        };
        let points = match self.list(m.get("points"), &field(path, "points")) {
            None => Vec::new(),
            Some(ps) => ps
                .iter()
                .enumerate()
                .filter_map(|(i, x)| {
                    let pp = format!("{path}.points[{i}]");
                    let o = self.object(x, &pp)?;
                    let beat = self.num_opt(o.get("beat"), &field(&pp, "beat"));
                    let value = self.num_opt(o.get("value"), &field(&pp, "value"));
                    match (beat, value) {
                        (Some(beat), Some(value)) => Some(AutomationPoint { beat, value }),
                        _ => {
                            self.warn(&pp, "automation point without a beat / value: dropped");
                            None
                        }
                    }
                })
                .collect(),
        };
        Some(AutomationLane {
            parameter,
            points,
            visible: self.flag(m.get("visible"), &field(path, "visible")),
        })
    }

    // --- Region / note --------------------------------------------------------------------------

    fn region(&mut self, m: &Map<String, Value>, path: &str) -> Region {
        let p = |k: &str| field(path, k);
        let notes = match m.get("notes") {
            Some(Value::Array(ns)) => ns
                .iter()
                .enumerate()
                .filter_map(|(i, n)| {
                    let np = format!("{path}.notes[{i}]");
                    let o = self.object(n, &np)?;
                    self.note(o, &np)
                })
                .collect(),
            other => {
                if other.is_some() {
                    self.warn(&p("notes"), "not a list");
                }
                Vec::new()
            }
        };
        let audio = match m.get("audio") {
            a if !js::truthy(a) => None,
            Some(Value::Object(o)) => Some(self.audio(o, &p("audio"))),
            Some(x) => {
                self.warn(
                    &p("audio"),
                    format!("{} is not audio data: dropped", type_name(x)),
                );
                None
            }
            None => None,
        };
        Region {
            id: self.id(m.get("id"), &p("id")),
            start_beat: self.num(m.get("startBeat"), &p("startBeat"), 0.0),
            length_beats: self.num(
                m.get("lengthBeats"),
                &p("lengthBeats"),
                DEFAULT_REGION_LENGTH,
            ),
            notes,
            color: self.text_opt(m.get("color"), &p("color")),
            name: self.text_opt(m.get("name"), &p("name")),
            audio,
            extra: extra(
                m,
                &[
                    "id",
                    "startBeat",
                    "lengthBeats",
                    "notes",
                    "color",
                    "name",
                    "audio",
                ],
            ),
        }
    }

    fn audio(&mut self, m: &Map<String, Value>, path: &str) -> AudioRegionData {
        let p = |k: &str| field(path, k);
        AudioRegionData {
            sample_id: self.text(m.get("sampleId"), &p("sampleId"), ""),
            sample_path: self.text(m.get("samplePath"), &p("samplePath"), ""),
            offset_seconds: self.num(m.get("offsetSeconds"), &p("offsetSeconds"), 0.0),
            gain_db: self.num(m.get("gainDb"), &p("gainDb"), 0.0),
            original_duration: self.num(m.get("originalDuration"), &p("originalDuration"), 0.0),
            extra: extra(
                m,
                &[
                    "sampleId",
                    "samplePath",
                    "offsetSeconds",
                    "gainDb",
                    "originalDuration",
                ],
            ),
        }
    }

    fn note(&mut self, m: &Map<String, Value>, path: &str) -> Option<RegionNote> {
        let p = |k: &str| field(path, k);
        let note = self.num_opt(m.get("note"), &p("note"));
        let start = self.num_opt(m.get("startBeat"), &p("startBeat"));
        let (Some(note), Some(start_beat)) = (note, start) else {
            self.warn(path, "note without a pitch / start: dropped");
            return None;
        };
        Some(RegionNote {
            note,
            velocity: self.num(m.get("velocity"), &p("velocity"), DEFAULT_VELOCITY),
            start_beat,
            duration_beats: self.num(
                m.get("durationBeats"),
                &p("durationBeats"),
                DEFAULT_NOTE_LENGTH,
            ),
            is_drum: self.flag(m.get("isDrum"), &field(path, "isDrum")),
        })
    }
}

#[cfg(test)]
#[path = "open_tests.rs"]
mod tests;
