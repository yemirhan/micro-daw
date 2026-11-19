//! Factory kit, pad defaults, kit validation and the classic built-in kits.
//! Port of `src/audio/sampler/factoryKit.ts` and `src/audio/sampler/classic/kit.ts`, plus the pure
//! source helpers of `src/audio/sampler/bufferCache.ts` (`packIdOfSource`, `padSourceKey`).
//!
//! The TS reads the packs folder from a global (`currentPacksDir()`); here it is a parameter.
//! `normalizeKit` draws a `crypto.randomUUID()` for a kit without an id; here the id generator is
//! injected (`new_id`).

use serde_json::{Map, Value};

use crate::classic_meta::{
    classic_kit_id, classic_sample_name, get_classic_machine, get_classic_voice,
    is_classic_machine_id, is_classic_voice,
};
use crate::js::round as js_round;
use crate::pack_source::pack_ref_from_path;
use crate::packs::validate::{is_safe_pack_path, is_valid_pack_id};
use crate::paths::{basename, display_name};
use crate::types::{
    ClassicMachineId, FactoryVoiceId, KitState, PAD_COUNT, PadIndex, PadPlayMode, PadSample,
    PadSampleSource, PadSettings,
};

/// Id of the built-in kit (user kits get their own ids).
pub const FACTORY_KIT_ID: &str = "factory";

/// `FACTORY_VOICE_LABELS`.
pub fn factory_voice_label(voice: FactoryVoiceId) -> &'static str {
    use FactoryVoiceId::*;
    match voice {
        Kick => "Kick",
        KickSub => "Sub Kick",
        Snare => "Snare",
        Rim => "Rim",
        Clap => "Clap",
        Chh => "Closed HH",
        Phh => "Pedal HH",
        Ohh => "Open HH",
        TomLo => "Low Tom",
        TomMid => "Mid Tom",
        TomHi => "High Tom",
        Crash => "Crash",
        Ride => "Ride",
        Shaker => "Shaker",
        Perc => "Cowbell",
        Snap => "Snap",
    }
}

/// `FACTORY_VOICE_IDS` (declaration order of the labels).
pub const FACTORY_VOICE_IDS: [FactoryVoiceId; 16] = FactoryVoiceId::ALL;

/// `FACTORY_PAD_VOICES`: bank A keeps the order of the old 8-pad kit.
pub const FACTORY_PAD_VOICES: [FactoryVoiceId; PAD_COUNT] = {
    use FactoryVoiceId::*;
    [
        Kick, Snare, Chh, Ohh, Clap, TomMid, Crash, Ride, KickSub, Rim, Phh, Shaker, Snap, TomLo,
        TomHi, Perc,
    ]
};

/// `FACTORY_CHOKE`: hi-hats choke each other.
fn factory_choke(voice: FactoryVoiceId) -> i64 {
    match voice {
        FactoryVoiceId::Chh | FactoryVoiceId::Phh | FactoryVoiceId::Ohh => 1,
        _ => 0,
    }
}

/// `PAD_COLORS`: one color per pad; bank A matches the old pad colors.
pub const PAD_COLORS: [&str; PAD_COUNT] = [
    "oklch(0.62 0.22 25)",
    "oklch(0.70 0.18 65)",
    "oklch(0.70 0.20 150)",
    "oklch(0.65 0.18 180)",
    "oklch(0.62 0.22 310)",
    "oklch(0.65 0.20 265)",
    "oklch(0.75 0.17 90)",
    "oklch(0.65 0.18 210)",
    "oklch(0.58 0.20 10)",
    "oklch(0.72 0.16 50)",
    "oklch(0.74 0.17 135)",
    "oklch(0.70 0.15 195)",
    "oklch(0.66 0.20 330)",
    "oklch(0.60 0.18 285)",
    "oklch(0.78 0.15 105)",
    "oklch(0.68 0.16 235)",
];

/// `FACTORY_VOICE_GM_NOTE`: GM drum note that best represents each factory voice (MIDI export).
pub fn factory_voice_gm_note(voice: FactoryVoiceId) -> i32 {
    use FactoryVoiceId::*;
    match voice {
        Kick => 36,
        KickSub => 35,
        Snare => 38,
        Rim => 37,
        Clap => 39,
        Chh => 42,
        Phh => 44,
        Ohh => 46,
        TomLo => 41,
        TomMid => 45,
        TomHi => 48,
        Crash => 49,
        Ride => 51,
        Shaker => 82,
        Perc => 56,
        Snap => 75,
    }
}

/// `GM_TO_FACTORY_VOICE`: GM drum note → closest factory voice.
fn gm_to_factory_voice(note: i64) -> Option<FactoryVoiceId> {
    use FactoryVoiceId::*;
    Some(match note {
        35 | 36 => Kick,
        37 => Rim,
        38 | 40 => Snare,
        39 => Clap,
        42 => Chh,
        44 => Phh,
        46 => Ohh,
        41 | 43 => TomLo,
        45 | 47 => TomMid,
        48 | 50 => TomHi,
        49 | 52 | 55 | 57 => Crash,
        51 | 53 | 59 => Ride,
        54 | 69 | 70 | 82 => Shaker,
        56 | 76 | 77 => Perc,
        75 => Snap,
        _ => return None,
    })
}

/// `gmNoteToFactoryVoice(note)` (`Math.round(note)` first).
pub fn gm_note_to_factory_voice(note: f64) -> Option<FactoryVoiceId> {
    let n = js_round(note);
    if !n.is_finite() || n.abs() > 1e6 {
        return None;
    }
    gm_to_factory_voice(n as i64)
}

/// `factoryPadForGmNote`: factory-kit pad that plays the voice closest to a GM drum note.
pub fn factory_pad_for_gm_note(note: f64) -> Option<PadIndex> {
    let voice = gm_note_to_factory_voice(note)?;
    FACTORY_PAD_VOICES.iter().position(|&v| v == voice)
}

/// `isFactoryVoiceId` on untyped input.
///
/// TS quirk NOT kept: the TS uses `v in FACTORY_VOICE_LABELS`, which is also true for inherited
/// `Object.prototype` keys ("toString", "constructor", ...). `FactoryVoiceId` cannot hold those,
/// so such voices are rejected here (the pad sample becomes null).
pub fn is_factory_voice_id(v: &Value) -> Option<FactoryVoiceId> {
    v.as_str().and_then(FactoryVoiceId::from_id)
}

/// `factorySample(voice)`.
pub fn factory_sample(voice: FactoryVoiceId) -> PadSample {
    PadSample {
        source: PadSampleSource::Factory { voice },
        name: factory_voice_label(voice).to_owned(),
    }
}

/// `defaultPadSettings()`.
pub fn default_pad_settings() -> PadSettings {
    PadSettings::default()
}

/// `createFactoryKit()`.
pub fn create_factory_kit() -> KitState {
    KitState {
        id: FACTORY_KIT_ID.to_owned(),
        name: "Factory".to_owned(),
        pads: FACTORY_PAD_VOICES
            .iter()
            .map(|&voice| PadSettings {
                sample: Some(factory_sample(voice)),
                choke_group: factory_choke(voice),
                ..default_pad_settings()
            })
            .collect(),
        extra: Default::default(),
    }
}

// ---------------------------------------------------------------------------
// Validation / cloning (for persisted kits and project files)
// ---------------------------------------------------------------------------

/// `clamp(v, lo, hi)` = `Math.min(hi, Math.max(lo, v))` (inputs are always finite here).
fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    hi.min(lo.max(v))
}

/// `num(v, fallback)`: a finite JSON number, else the fallback.
fn num(v: Option<&Value>, fallback: f64) -> f64 {
    v.and_then(Value::as_f64)
        .filter(|n| n.is_finite())
        .unwrap_or(fallback)
}

/// Truthy non-empty string.
fn non_empty_str(v: Option<&Value>) -> Option<&str> {
    v.and_then(Value::as_str).filter(|s| !s.is_empty())
}

/// `normalizeSource` on untyped input.
fn normalize_source(src: Option<&Value>, packs_dir: Option<&str>) -> Option<PadSampleSource> {
    let s = src?.as_object()?;
    let kind = s.get("kind").and_then(Value::as_str);
    let null = Value::Null;
    let field = |k: &str| s.get(k).unwrap_or(&null);
    match kind {
        Some("factory") => {
            if let Some(voice) = is_factory_voice_id(field("voice")) {
                return Some(PadSampleSource::Factory { voice });
            }
        }
        Some("classic") => {
            if let Some(machine) = is_classic_machine_id(field("machine"))
                && is_classic_voice(field("machine"), field("voice"))
            {
                let voice = field("voice").as_str().unwrap_or_default().to_owned();
                return Some(PadSampleSource::Classic { machine, voice });
            }
        }
        Some("sound") => {
            if let Some(id) = non_empty_str(s.get("soundId")) {
                return Some(PadSampleSource::Sound {
                    sound_id: id.to_owned(),
                });
            }
        }
        Some("pack") => {
            if let (Some(pack_id), Some(file)) = (field("packId").as_str(), field("file").as_str())
                && is_valid_pack_id(pack_id)
                && is_safe_pack_path(file)
            {
                return Some(PadSampleSource::Pack {
                    pack_id: pack_id.to_owned(),
                    file: file.to_owned(),
                });
            }
        }
        Some("file") => {
            if let Some(path) = non_empty_str(s.get("path")) {
                // Migration: absolute paths into the packs folder (older pack kits) become pack sources.
                if let Some(r) = pack_ref_from_path(packs_dir, path) {
                    return Some(PadSampleSource::Pack {
                        pack_id: r.pack_id,
                        file: r.file,
                    });
                }
                let sample_id = s.get("sampleId").and_then(Value::as_str).map(str::to_owned);
                return Some(PadSampleSource::File {
                    path: path.to_owned(),
                    sample_id,
                });
            }
        }
        _ => {}
    }
    None
}

/// `sourceFallbackName`: display name for a sample source without a stored name.
pub fn source_fallback_name(source: &PadSampleSource) -> String {
    let file_name = |p: &str| {
        let n = display_name(&basename(p));
        if n.is_empty() { "Sample".to_owned() } else { n }
    };
    match source {
        PadSampleSource::Factory { voice } => factory_voice_label(*voice).to_owned(),
        PadSampleSource::Classic { machine, voice } => classic_sample_name(*machine, voice),
        PadSampleSource::Pack { file, .. } => file_name(file),
        PadSampleSource::File { path, .. } => file_name(path),
        PadSampleSource::Sound { .. } => "Sound".to_owned(),
    }
}

fn normalize_sample(sample: Option<&Value>, packs_dir: Option<&str>) -> Option<PadSample> {
    let s = sample?.as_object()?;
    let source = normalize_source(s.get("source"), packs_dir)?;
    let name =
        non_empty_str(s.get("name")).map_or_else(|| source_fallback_name(&source), str::to_owned);
    Some(PadSample { source, name })
}

/// `normalizePad(raw)`: coerce untrusted pad data (old/foreign JSON) into valid, clamped settings.
pub fn normalize_pad(raw: &Value, packs_dir: Option<&str>) -> PadSettings {
    let d = default_pad_settings();
    let Some(p) = raw.as_object() else { return d };
    let start_sec = 0f64.max(num(p.get("startSec"), d.start_sec));
    let end_sec = p
        .get("endSec")
        .and_then(Value::as_f64)
        .filter(|e| e.is_finite() && *e > start_sec);
    let mode = if p.get("mode").and_then(Value::as_str) == Some("gate") {
        PadPlayMode::Gate
    } else {
        PadPlayMode::Oneshot
    };
    // `Math.max(0, Math.round(n))`; the i64 cast saturates beyond ±9.2e18 (TS keeps the double).
    let choke_group = 0f64.max(js_round(num(p.get("chokeGroup"), 0.0))) as i64;
    PadSettings {
        sample: normalize_sample(p.get("sample"), packs_dir),
        gain_db: clamp(num(p.get("gainDb"), d.gain_db), -60.0, 12.0),
        pan: clamp(num(p.get("pan"), d.pan), -1.0, 1.0),
        pitch: clamp(num(p.get("pitch"), d.pitch), -24.0, 24.0),
        start_sec,
        end_sec,
        decay: clamp(num(p.get("decay"), d.decay), 0.005, 4.0),
        filter_cutoff: clamp(num(p.get("filterCutoff"), d.filter_cutoff), 20.0, 20000.0),
        reverse: p.get("reverse") == Some(&Value::Bool(true)),
        mode,
        choke_group,
        color: non_empty_str(p.get("color")).map(str::to_owned),
        extra: Default::default(),
    }
}

/// `normalizeKit(raw)`: valid kit with exactly PAD_COUNT pads (missing pads are empty).
pub fn normalize_kit(
    raw: &Value,
    packs_dir: Option<&str>,
    new_id: &mut dyn FnMut() -> String,
) -> KitState {
    let empty = Map::new();
    let k = raw.as_object().unwrap_or(&empty);
    let pads: &[Value] = k
        .get("pads")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice);
    KitState {
        id: non_empty_str(k.get("id")).map_or_else(new_id, str::to_owned),
        name: non_empty_str(k.get("name")).unwrap_or("Kit").to_owned(),
        pads: (0..PAD_COUNT)
            .map(|i| normalize_pad(pads.get(i).unwrap_or(&Value::Null), packs_dir))
            .collect(),
        extra: Default::default(),
    }
}

/// `cloneKit`: a deep copy (Rust values are already independent).
pub fn clone_kit(kit: &KitState) -> KitState {
    kit.clone()
}

// ---------------------------------------------------------------------------
// Classic kits (src/audio/sampler/classic/kit.ts)
// ---------------------------------------------------------------------------

/// `classicSample(machine, voice)`: classic source named like "808 Kick".
pub fn classic_sample(machine: ClassicMachineId, voice: &str) -> PadSample {
    PadSample {
        source: PadSampleSource::Classic {
            machine,
            voice: voice.to_owned(),
        },
        name: classic_sample_name(machine, voice),
    }
}

/// `createClassicKit(machine)`: default layout, per-voice gain, hats in choke group 1, pads named
/// by the bare voice label.
pub fn create_classic_kit(machine: ClassicMachineId) -> KitState {
    let m = get_classic_machine(machine);
    KitState {
        id: classic_kit_id(machine),
        name: m.name.to_owned(),
        pads: (0..PAD_COUNT)
            .map(|i| {
                let voice = m.pads[i];
                let Some(info) = get_classic_voice(machine, voice) else {
                    return default_pad_settings();
                };
                PadSettings {
                    sample: Some(PadSample {
                        name: info.label.to_owned(),
                        ..classic_sample(machine, voice)
                    }),
                    gain_db: info.gain_db.unwrap_or(0.0),
                    choke_group: info.choke.unwrap_or(0),
                    mode: info.mode.unwrap_or(PadPlayMode::Oneshot),
                    ..default_pad_settings()
                }
            })
            .collect(),
        extra: Default::default(),
    }
}

// ---------------------------------------------------------------------------
// Sample source keys (pure part of src/audio/sampler/bufferCache.ts)
// ---------------------------------------------------------------------------

/// `packIdOfSource`: pack sources, and file sources pointing into the packs folder.
pub fn pack_id_of_source(source: &PadSampleSource, packs_dir: Option<&str>) -> Option<String> {
    match source {
        PadSampleSource::Pack { pack_id, .. } => Some(pack_id.clone()),
        PadSampleSource::File { path, .. } => {
            pack_ref_from_path(packs_dir, path).map(|r| r.pack_id)
        }
        _ => None,
    }
}

/// The generation id of a designed sound (bumped each time it is re-rendered; distinct from
/// pack ids, which never contain a colon).
pub fn sound_generation_id(sound_id: &str) -> String {
    format!("sound:{sound_id}")
}

/// `padSourceKey`: stable cache key of a sample source. Pack sounds include the pack's generation
/// (`generation(packId)`, 0 when never bumped).
pub fn pad_source_key(
    source: &PadSampleSource,
    packs_dir: Option<&str>,
    generation: &dyn Fn(&str) -> u64,
) -> String {
    match source {
        PadSampleSource::Factory { voice } => format!("factory:{}", voice.id()),
        PadSampleSource::Classic { machine, voice } => format!("classic:{}:{voice}", machine.id()),
        PadSampleSource::Pack { pack_id, file } => {
            format!("pack:{pack_id}@{}:{file}", generation(pack_id))
        }
        PadSampleSource::File { path, .. } => match pack_id_of_source(source, packs_dir) {
            Some(id) => format!("file@{}:{path}", generation(&id)),
            None => format!("file:{path}"),
        },
        // Re-rendering a sound bumps the generation of `sound:<id>` (pads reload).
        PadSampleSource::Sound { sound_id } => {
            format!(
                "sound:{sound_id}@{}",
                generation(&sound_generation_id(sound_id))
            )
        }
    }
}

#[cfg(test)]
#[path = "kit_tests.rs"]
mod tests;
