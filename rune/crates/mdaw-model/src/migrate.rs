//! Arrangement migration run on every load / restore, the pure id remapping of
//! "new project from template", and the pure parts of the project file (build, parse, sample
//! manifest, recent projects).
//!
//! Ports:
//! - `migrateTrack` / `migrateArrangement` (src/services/ArrangementEngine.ts ~2816-2843);
//! - the id-remapping half of `ArrangementEngine.loadFromTemplate` (~2204-2233);
//! - the pure parts of `src/services/ProjectManager.ts` (see `project` below).
//!
//! The TS migrates untyped `JSON.parse` output in place, so the core works on `serde_json::Value`
//! with the same JS semantics (`??=`, truthiness, `>` coercion, where TS throws a TypeError we
//! return `MigrateError`). Typed wrappers round-trip through `Value`.
//!
//! Injected instead of globals: the packs folder (`packs_dir`, absolute pack path migration in
//! `normalizeKit`) and the id generator (`new_id`, = `crypto.randomUUID`).
//!
//! Deviations (unrealistic inputs only): a non-array `tracks` / `regions` / `notes` / `markers` /
//! `trackIds` is an error (TS would iterate a string's characters); JS string-vs-string `>=`
//! comparisons of velocities compare numerically here.

use std::fmt;

use serde_json::{Map, Value, json};

use crate::arrangement_helpers::STEP_EPSILON;
use crate::js::{ge as js_ge, round as js_round, string_of as js_string};
pub(crate) use crate::js::{same_value_zero, to_number as js_num, truthy as js_truthy};
use crate::kit::{create_factory_kit, factory_pad_for_gm_note, normalize_kit};
use crate::loop_settings::normalize_loop_settings;
use crate::types::{Arrangement, LoopRecordSettings, PAD_COUNT, TrackEffectState, pad_to_note};

#[path = "migrate_project.rs"]
mod project;
pub use project::*;

/// A shape the TS code would throw a TypeError on (the load then fails).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrateError(pub String);

impl fmt::Display for MigrateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "migration failed: {}", self.0)
    }
}

impl std::error::Error for MigrateError {}

fn err<T>(what: &str) -> Result<T, MigrateError> {
    Err(MigrateError(what.to_owned()))
}

/// `console.warn('[arrangement] "<name>": N legacy drum note(s) have no pad and were dropped')`.
#[derive(Debug, Clone, PartialEq)]
pub struct DroppedDrumNotes {
    /// `track.name` as JS would print it (`String(track.name)`).
    pub track_name: String,
    pub dropped: usize,
}

impl DroppedDrumNotes {
    pub fn message(&self) -> String {
        format!(
            "[arrangement] \"{}\": {} legacy drum note(s) have no pad and were dropped",
            self.track_name, self.dropped
        )
    }
}

// ---------------------------------------------------------------------------
// JS value semantics
// ---------------------------------------------------------------------------

/// `obj.key` read: TypeError on `undefined` / `null`, `undefined` for other non-objects.
fn prop<'a>(
    v: Option<&'a Value>,
    key: &str,
    what: &str,
) -> Result<Option<&'a Value>, MigrateError> {
    match v {
        None | Some(Value::Null) => err(&format!("cannot read '{key}' of {what}")),
        Some(Value::Object(m)) => Ok(m.get(key)),
        Some(_) => Ok(None),
    }
}

/// An object we must assign to (assigning to `null`/`undefined`/a primitive throws in strict mode).
fn obj_mut<'a>(v: &'a mut Value, what: &str) -> Result<&'a mut Map<String, Value>, MigrateError> {
    v.as_object_mut()
        .ok_or_else(|| MigrateError(format!("{what} is not an object")))
}

fn arr_mut<'a>(v: Option<&'a mut Value>, what: &str) -> Result<&'a mut Vec<Value>, MigrateError> {
    v.and_then(Value::as_array_mut)
        .ok_or_else(|| MigrateError(format!("{what} is not iterable")))
}

/// `x ??= value`.
fn default_if_nullish(m: &mut Map<String, Value>, key: &str, value: impl FnOnce() -> Value) {
    if matches!(m.get(key), None | Some(Value::Null)) {
        m.insert(key.to_owned(), value());
    }
}

// ---------------------------------------------------------------------------
// Drum notes (Value-level twins of arrangement_helpers' typed functions)
// ---------------------------------------------------------------------------

/// `mergeDrumHit` on untyped notes.
fn merge_drum_hit_value(notes: &mut Vec<Value>, hit: Value) {
    let found = notes.iter().position(|n| {
        same_value_zero(
            n.get("note").unwrap_or(&Value::Null),
            hit.get("note").unwrap_or(&Value::Null),
        ) && (js_num(n.get("startBeat")) - js_num(hit.get("startBeat"))).abs() < STEP_EPSILON
    });
    match found {
        None => notes.push(hit),
        Some(i) => {
            if !js_ge(
                js_num(notes[i].get("velocity")),
                js_num(hit.get("velocity")),
            ) {
                let v = hit.get("velocity").cloned();
                let m = notes[i].as_object_mut().expect("merged notes are objects");
                match v {
                    Some(v) => m.insert("velocity".into(), v),
                    None => m.remove("velocity"),
                };
            }
        }
    }
}

/// `migrateGmDrumNotes` on untyped notes: returns (notes, dropped).
fn migrate_gm_drum_notes_value(notes: &[Value]) -> Result<(Vec<Value>, usize), MigrateError> {
    let mut out = Vec::new();
    let mut dropped = 0;
    for n in notes {
        let pad = factory_pad_for_gm_note(js_num(prop(Some(n), "note", "a note")?));
        let (Some(pad), Some(m)) = (pad, n.as_object()) else {
            dropped += 1;
            continue;
        };
        let mut hit = m.clone();
        hit.insert("note".into(), json!(pad_to_note(pad)));
        hit.insert("isDrum".into(), Value::Bool(true));
        merge_drum_hit_value(&mut out, Value::Object(hit));
    }
    Ok((out, dropped))
}

/// `migrateDrumTrack` on an untyped track: returns the number of dropped legacy notes.
fn migrate_drum_track_value(
    track: &mut Map<String, Value>,
    packs_dir: Option<&str>,
    new_id: &mut dyn FnMut() -> String,
) -> Result<usize, MigrateError> {
    let instrument = prop(track.get("instrument"), "type", "track.instrument")?;
    if instrument.and_then(Value::as_str) != Some("drums") {
        return Ok(0);
    }
    let inst = obj_mut(
        track.get_mut("instrument").expect("checked above"),
        "track.instrument",
    )?;
    if js_truthy(inst.get("kit")) {
        let kit = normalize_kit(&inst["kit"], packs_dir, new_id);
        inst.insert(
            "kit".into(),
            serde_json::to_value(kit).expect("kit serializes"),
        );
        return Ok(0);
    }
    let mut dropped = 0;
    for region in arr_mut(track.get_mut("regions"), "track.regions")? {
        let region = obj_mut(region, "region")?;
        if js_truthy(region.get("audio")) {
            continue;
        }
        let notes = arr_mut(region.get_mut("notes"), "region.notes")?;
        let (migrated, d) = migrate_gm_drum_notes_value(notes)?;
        *notes = migrated;
        dropped += d;
    }
    let inst = obj_mut(
        track.get_mut("instrument").expect("checked above"),
        "track.instrument",
    )?;
    inst.insert(
        "kit".into(),
        serde_json::to_value(create_factory_kit()).expect("kit serializes"),
    );
    Ok(dropped)
}

// ---------------------------------------------------------------------------
// migrateTrack / migrateArrangement
// ---------------------------------------------------------------------------

/// `migrateTrack`: default pan/effects, velocities on the 0..1 scale (old data stored MIDI
/// 0..127), legacy drum tracks onto the kit model.
fn migrate_track_value(
    track: &mut Value,
    packs_dir: Option<&str>,
    new_id: &mut dyn FnMut() -> String,
    warnings: &mut Vec<DroppedDrumNotes>,
) -> Result<(), MigrateError> {
    let t = obj_mut(track, "track")?;
    default_if_nullish(t, "pan", || json!(0));
    default_if_nullish(t, "effects", || {
        serde_json::to_value(TrackEffectState::default()).expect("effects serialize")
    });
    for region in arr_mut(t.get_mut("regions"), "track.regions")? {
        let region = obj_mut(region, "region")?;
        for note in arr_mut(region.get_mut("notes"), "region.notes")? {
            if note.is_null() {
                return err("cannot read 'velocity' of null");
            }
            let v = js_num(note.get("velocity"));
            if v > 1.0
                && let Some(m) = note.as_object_mut()
            {
                m.insert("velocity".into(), json!(1f64.min(v / 127.0)));
            }
        }
    }
    let dropped = migrate_drum_track_value(t, packs_dir, new_id)?;
    if dropped > 0 {
        warnings.push(DroppedDrumNotes {
            track_name: js_string(t.get("name")),
            dropped,
        });
    }
    Ok(())
}

/// `normalizeBeatState(raw, tracks)` on untyped data (the active id is whatever the first drum
/// track's id is, exactly as the TS returns it).
fn normalize_beat_state_value(
    raw: Option<&Value>,
    tracks: &[Value],
) -> Result<Value, MigrateError> {
    let empty = Map::new();
    let r = raw.and_then(Value::as_object).unwrap_or(&empty);
    let mut drum_ids = Vec::new();
    for t in tracks {
        let inst = prop(Some(t), "instrument", "a track")?;
        if prop(inst, "type", "track.instrument")?.and_then(Value::as_str) == Some("drums") {
            drum_ids.push(t.get("id").cloned().unwrap_or(Value::Null));
        }
    }
    let active = match r.get("activeDrumTrackId") {
        Some(id @ Value::String(_)) if drum_ids.iter().any(|d| same_value_zero(d, id)) => {
            id.clone()
        }
        _ => drum_ids.first().cloned().unwrap_or(Value::Null),
    };
    let pad = r
        .get("selectedPad")
        .and_then(Value::as_f64)
        .map_or(0.0, js_round);
    let lp: LoopRecordSettings = normalize_loop_settings(
        r.get("loop").unwrap_or(&Value::Null),
        &LoopRecordSettings::default(),
    );
    Ok(json!({
        "activeDrumTrackId": active,
        "loop": lp,
        "selectedPad": if pad >= 0.0 && pad < PAD_COUNT as f64 { pad } else { 0.0 },
    }))
}

/// `migrateArrangement` (mutates `arrangement` in place, like the TS). Returns the
/// dropped-legacy-note warnings the TS logs.
pub fn migrate_arrangement_value(
    arrangement: &mut Value,
    packs_dir: Option<&str>,
    new_id: &mut dyn FnMut() -> String,
) -> Result<Vec<DroppedDrumNotes>, MigrateError> {
    let a = obj_mut(arrangement, "arrangement")?;
    let mut warnings = Vec::new();
    for track in arr_mut(a.get_mut("tracks"), "arrangement.tracks")? {
        migrate_track_value(track, packs_dir, new_id, &mut warnings)?;
    }
    let beat = normalize_beat_state_value(
        a.get("beat"),
        a["tracks"].as_array().expect("checked above"),
    )?;
    let bars = beat["loop"]["bars"].as_f64().expect("bars is a number");
    a.insert("beat".into(), beat);
    // Without (valid) loop markers the pattern and the loop are the first `bars` bars.
    let m = a.get("loopMarkers");
    let finite = |k: &str| m.and_then(|m| m.get(k)).and_then(Value::as_f64);
    let valid = js_truthy(m)
        && matches!((finite("startBeat"), finite("endBeat")), (Some(s), Some(e)) if e > s);
    if !valid {
        let end_beat = bars * 4.0;
        a.insert(
            "loopMarkers".into(),
            json!({ "startBeat": 0, "endBeat": end_beat }),
        );
        if !js_ge(js_num(a.get("lengthBeats")), end_beat) {
            a.insert("lengthBeats".into(), json!((end_beat / 4.0).ceil() * 4.0));
        }
    }
    Ok(warnings)
}

/// Typed `migrateArrangement`: the arrangement after migration plus the warnings.
pub fn migrate_arrangement(
    arrangement: &Arrangement,
    packs_dir: Option<&str>,
    new_id: &mut dyn FnMut() -> String,
) -> Result<(Arrangement, Vec<DroppedDrumNotes>), MigrateError> {
    let mut v = serde_json::to_value(arrangement).map_err(|e| MigrateError(e.to_string()))?;
    let warnings = migrate_arrangement_value(&mut v, packs_dir, new_id)?;
    let out = serde_json::from_value(v).map_err(|e| MigrateError(e.to_string()))?;
    Ok((out, warnings))
}

// ---------------------------------------------------------------------------
// loadFromTemplate (id remapping)
// ---------------------------------------------------------------------------

/// The id remapping of `loadFromTemplate`: fresh ids for the arrangement, tracks, regions,
/// markers and groups (in that call order), with `beat.activeDrumTrackId` and group track lists
/// remapped. Mutates the clone in place.
pub fn remap_template_ids_value(
    clone: &mut Value,
    new_id: &mut dyn FnMut() -> String,
) -> Result<(), MigrateError> {
    let a = obj_mut(clone, "arrangement")?;
    a.insert("id".into(), Value::String(new_id()));
    let mut track_ids: Vec<(Value, String)> = Vec::new();
    for track in arr_mut(a.get_mut("tracks"), "arrangement.tracks")? {
        let t = obj_mut(track, "track")?;
        let id = new_id();
        let old = t.get("id").cloned().unwrap_or(Value::Null);
        track_ids.retain(|(k, _)| !same_value_zero(k, &old));
        track_ids.push((old, id.clone()));
        t.insert("id".into(), Value::String(id));
        for region in arr_mut(t.get_mut("regions"), "track.regions")? {
            obj_mut(region, "region")?.insert("id".into(), Value::String(new_id()));
        }
    }
    let lookup = |k: &Value| {
        track_ids
            .iter()
            .find(|(old, _)| same_value_zero(old, k))
            .map(|(_, new)| new.clone())
    };
    if let Some(beat) = a.get_mut("beat").and_then(Value::as_object_mut)
        && js_truthy(beat.get("activeDrumTrackId"))
    {
        let mapped = lookup(&beat["activeDrumTrackId"]).map_or(Value::Null, Value::String);
        beat.insert("activeDrumTrackId".into(), mapped);
    }
    if js_truthy(a.get("markers")) {
        for marker in arr_mut(a.get_mut("markers"), "arrangement.markers")? {
            obj_mut(marker, "marker")?.insert("id".into(), Value::String(new_id()));
        }
    }
    if js_truthy(a.get("groups")) {
        for group in arr_mut(a.get_mut("groups"), "arrangement.groups")? {
            let g = obj_mut(group, "group")?;
            g.insert("id".into(), Value::String(new_id()));
            let ids = arr_mut(g.get_mut("trackIds"), "group.trackIds")?;
            *ids = ids
                .iter()
                .filter_map(|k| lookup(k).map(Value::String))
                .collect();
        }
    }
    Ok(())
}

/// `loadFromTemplate` minus the engine: deep clone, fresh ids, then `migrateArrangement`.
pub fn load_from_template_value(
    template_arrangement: &Value,
    packs_dir: Option<&str>,
    new_id: &mut dyn FnMut() -> String,
) -> Result<(Value, Vec<DroppedDrumNotes>), MigrateError> {
    let mut clone = template_arrangement.clone();
    remap_template_ids_value(&mut clone, new_id)?;
    let warnings = migrate_arrangement_value(&mut clone, packs_dir, new_id)?;
    Ok((clone, warnings))
}

/// Typed `loadFromTemplate` (see `load_from_template_value`).
pub fn load_from_template(
    template_arrangement: &Arrangement,
    packs_dir: Option<&str>,
    new_id: &mut dyn FnMut() -> String,
) -> Result<(Arrangement, Vec<DroppedDrumNotes>), MigrateError> {
    let v = serde_json::to_value(template_arrangement).map_err(|e| MigrateError(e.to_string()))?;
    let (out, warnings) = load_from_template_value(&v, packs_dir, new_id)?;
    Ok((
        serde_json::from_value(out).map_err(|e| MigrateError(e.to_string()))?,
        warnings,
    ))
}

/// A fresh v4 UUID (`crypto.randomUUID()`), the production `new_id`.
pub fn random_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
pub(crate) mod tests_support {
    use serde_json::Value;

    /// Structural JSON diff (numbers within 1e-9, object key order ignored). None when equal.
    pub fn json_diff(got: &Value, want: &Value, path: &str) -> Option<String> {
        match (got, want) {
            (Value::Number(a), Value::Number(b)) => {
                let (a, b) = (a.as_f64().unwrap(), b.as_f64().unwrap());
                ((a - b).abs() > 1e-9).then(|| format!("{path}: {a} != {b}"))
            }
            (Value::Array(a), Value::Array(b)) => {
                if a.len() != b.len() {
                    return Some(format!("{path}: length {} != {}", a.len(), b.len()));
                }
                a.iter()
                    .zip(b)
                    .enumerate()
                    .find_map(|(i, (x, y))| json_diff(x, y, &format!("{path}[{i}]")))
            }
            (Value::Object(a), Value::Object(b)) => {
                let mut keys: Vec<&String> = a.keys().chain(b.keys()).collect();
                keys.sort();
                keys.dedup();
                keys.into_iter().find_map(|k| match (a.get(k), b.get(k)) {
                    (Some(x), Some(y)) => json_diff(x, y, &format!("{path}.{k}")),
                    (x, y) => Some(format!("{path}.{k}: {x:?} != {y:?}")),
                })
            }
            _ => (got != want).then(|| format!("{path}: {got} != {want}")),
        }
    }

    #[track_caller]
    pub fn assert_json_eq(got: &Value, want: &Value, ctx: &str) {
        if let Some(d) = json_diff(got, want, "$") {
            panic!("{ctx}: {d}\n got: {got}\nwant: {want}");
        }
    }

    /// The deterministic id generator the golden scripts install (`uuid-1`, `uuid-2`, ...).
    pub fn counter_ids() -> impl FnMut() -> String {
        let mut n = 0;
        move || {
            n += 1;
            format!("uuid-{n}")
        }
    }

    /// `ALL_TEMPLATES` (src/data/templates/index.ts order) as raw JSON, with their ids.
    pub fn templates() -> Vec<(&'static str, Value)> {
        [
            (
                "blank-4track",
                include_str!("../../../../src/data/templates/blank-4track.json"),
            ),
            (
                "rock-song",
                include_str!("../../../../src/data/templates/rock-song.json"),
            ),
            (
                "hip-hop-beat",
                include_str!("../../../../src/data/templates/hip-hop-beat.json"),
            ),
            (
                "jazz-trio",
                include_str!("../../../../src/data/templates/jazz-trio.json"),
            ),
            (
                "edm-drop",
                include_str!("../../../../src/data/templates/edm-drop.json"),
            ),
            (
                "ambient-pad",
                include_str!("../../../../src/data/templates/ambient-pad.json"),
            ),
        ]
        .into_iter()
        .map(|(id, s)| (id, serde_json::from_str(s).expect("template JSON")))
        .collect()
    }
}

#[cfg(test)]
#[path = "migrate_tests.rs"]
mod tests;
