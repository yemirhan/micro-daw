//! Merging of (possibly legacy / untrusted) parameter overrides onto presets
//! (`src/audio/presetParams.ts`), on the presets' JSON form:
//!
//! - only keys that exist on the base preset are applied, and only with a value of the same type
//!   (numbers must be finite); unknown keys are dropped;
//! - enumerated string fields (and `filter.rolloff`) are validated against their allowed values;
//! - arrays (`partials`) are taken only when every element is a finite number (first 32);
//! - overrides written by the previous engine (Tone.js synth options: `envelope`,
//!   `filterEnvelope`, `modulationIndex`, `harmonicity`, `modulationEnvelope`, `pitchDecay`)
//!   are translated first;
//! - identity fields (id, name, engine, category, vibe, description, sample) are never
//!   overridden.

use serde_json::{Map, Value};

use super::presets::SynthPreset;

const LOCKED_KEYS: [&str; 7] = [
    "id",
    "name",
    "engine",
    "category",
    "vibe",
    "description",
    "sample",
];

fn allowed(path: &str) -> Option<&'static [&'static str]> {
    const OSC: &[&str] = &["sine", "triangle", "sawtooth", "square", "pulse"];
    Some(match path {
        "oscA.wave" | "oscB.wave" => OSC,
        "sub.wave" => &["sine", "triangle", "square"],
        "noise.type" => &["white", "pink", "brown"],
        "filter.type" => &["lowpass", "highpass", "bandpass", "notch"],
        "carrier.wave" => &["sine", "triangle", "square", "sawtooth"],
        _ => return None,
    })
}

fn finite_number(v: &Value) -> bool {
    v.as_f64().is_some_and(f64::is_finite)
}

fn merge_into(
    base: &Map<String, Value>,
    over: &Map<String, Value>,
    path: &str,
) -> Map<String, Value> {
    let mut out = base.clone();
    for (key, value) in over {
        let Some(cur) = base.get(key) else { continue };
        let full = if path.is_empty() {
            key.clone()
        } else {
            format!("{path}.{key}")
        };
        if path.is_empty() && LOCKED_KEYS.contains(&key.as_str()) {
            continue;
        }
        if let Value::Object(cur) = cur {
            if let Value::Object(v) = value {
                out.insert(key.clone(), Value::Object(merge_into(cur, v, &full)));
            }
            continue;
        }
        if cur.is_array() {
            if let Value::Array(items) = value
                && items.iter().all(finite_number)
            {
                out.insert(
                    key.clone(),
                    Value::Array(items.iter().take(32).cloned().collect()),
                );
            }
            continue;
        }
        if full == "filter.rolloff" {
            if value.as_f64().is_some_and(|r| r == -12.0 || r == -24.0) {
                out.insert(key.clone(), value.clone());
            }
            continue;
        }
        if let Some(allowed) = allowed(&full) {
            if value.as_str().is_some_and(|s| allowed.contains(&s)) {
                out.insert(key.clone(), value.clone());
            }
            continue;
        }
        let same_type = match cur {
            Value::Number(_) => finite_number(value),
            Value::Bool(_) => value.is_boolean(),
            Value::String(_) => value.is_string(),
            _ => false,
        };
        if same_type {
            out.insert(key.clone(), value.clone());
        }
    }
    out
}

fn obj(v: Option<&Value>) -> Map<String, Value> {
    match v {
        Some(Value::Object(m)) => m.clone(),
        _ => Map::new(),
    }
}

/// `translateLegacyOverrides`.
pub fn translate_legacy_overrides(
    preset: &SynthPreset,
    overrides: &Map<String, Value>,
) -> Map<String, Value> {
    let engine = preset.engine();
    let mut out = Map::new();
    for (key, value) in overrides {
        match key.as_str() {
            "envelope" => {
                let Value::Object(v) = value else { continue };
                if engine == "subtractive" || engine == "fm" {
                    let mut amp = obj(out.get("amp"));
                    for (k, x) in v {
                        amp.insert(k.clone(), x.clone());
                    }
                    out.insert("amp".into(), Value::Object(amp));
                } else if let Some(r) = v.get("release") {
                    out.insert("release".into(), r.clone());
                }
            }
            "filterEnvelope" => {
                let Value::Object(v) = value else { continue };
                if engine != "subtractive" {
                    continue;
                }
                let mut adsr = v.clone();
                let base = adsr.remove("baseFrequency");
                let octaves = adsr.remove("octaves");
                out.insert("filterEnv".into(), Value::Object(adsr));
                let mut filter = Map::new();
                if let Some(b) = base.filter(Value::is_number) {
                    filter.insert("cutoff".into(), b);
                }
                if let Some(o) = octaves.filter(Value::is_number) {
                    filter.insert("envAmount".into(), o);
                }
                let mut merged = obj(out.get("filter"));
                merged.extend(filter);
                out.insert("filter".into(), Value::Object(merged));
            }
            "modulationIndex" => {
                if engine == "fm" {
                    out.insert("index".into(), value.clone());
                }
            }
            "harmonicity" => {
                if engine == "fm" {
                    out.insert("ratio".into(), value.clone());
                }
            }
            "modulationEnvelope" => {
                if engine == "fm" && value.is_object() {
                    out.insert("modEnv".into(), value.clone());
                }
            }
            "pitchDecay" => {
                if engine == "subtractive" {
                    let mut m = Map::new();
                    m.insert("decay".into(), value.clone());
                    out.insert("pitchEnv".into(), Value::Object(m));
                }
            }
            _ => {
                out.insert(key.clone(), value.clone());
            }
        }
    }
    out
}

/// `mergePresetParams(base, overrides)`: a new, always-valid preset (the base when there is
/// nothing to merge).
pub fn merge_preset_params(base: &SynthPreset, overrides: Option<&Value>) -> SynthPreset {
    let Some(Value::Object(over)) = overrides else {
        return base.clone();
    };
    if over.is_empty() {
        return base.clone();
    }
    let translated = translate_legacy_overrides(base, over);
    let Value::Object(b) = base.to_json() else {
        return base.clone();
    };
    let merged = merge_into(&b, &translated, "");
    SynthPreset::from_json(&Value::Object(merged)).unwrap_or_else(|| base.clone())
}

/// `pathToObject('filter.cutoff', 800)` → `{ filter: { cutoff: 800 } }`.
pub fn path_to_object(path: &str, value: Value) -> Value {
    let parts: Vec<&str> = path.split('.').filter(|p| !p.is_empty()).collect();
    let mut cur = value;
    for part in parts.iter().rev() {
        let mut m = Map::new();
        m.insert((*part).to_string(), cur);
        cur = Value::Object(m);
    }
    if parts.is_empty() {
        Value::Object(Map::new())
    } else {
        cur
    }
}

/// `mergeOverrides(a, b)`: deep merge of two plain override objects (later wins).
pub fn merge_overrides(
    a: Option<&Map<String, Value>>,
    b: &Map<String, Value>,
) -> Map<String, Value> {
    let mut out = a.cloned().unwrap_or_default();
    for (k, v) in b {
        let merged = match (v, out.get(k)) {
            (Value::Object(vb), Some(Value::Object(va))) => {
                Value::Object(merge_overrides(Some(va), vb))
            }
            _ => v.clone(),
        };
        out.insert(k.clone(), merged);
    }
    out
}

/// `getPath(obj, 'filter.cutoff')`.
pub fn get_path<'a>(obj: &'a Value, path: &str) -> Option<&'a Value> {
    let mut cur = obj;
    for part in path.split('.') {
        cur = cur.as_object()?.get(part)?;
    }
    Some(cur)
}
