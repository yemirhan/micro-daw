//! The instrument preset model (`src/types/audio.ts`) and bank lookups (`src/audio/presets.ts`,
//! `src/utils/synthFactory.ts` `presetAt` / `resolvePreset`).
//!
//! Every preset is a flat, JSON-serialisable object (serde names = the TS keys), so per-track
//! `synthOverrides` merge on its JSON form ([`super::params::merge_preset_params`]). Optional keys
//! the TS only sets on some presets (`width`, `partials`, `description`) are skipped when absent,
//! which keeps the merge's "only keys that exist on the base" rule identical.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use super::wa::biquad::FilterType;
pub use super::wa::noise::NoiseType;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PresetCategory {
    Keys,
    Bass,
    Lead,
    Pad,
    Pluck,
    Brass,
    Bell,
    Percussion,
    Fx,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PresetVibe {
    Chill,
    Bright,
    Dark,
    Punchy,
    Dreamy,
    Aggressive,
    Organic,
    Experimental,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OscWave {
    Sine,
    Triangle,
    Sawtooth,
    Square,
    Pulse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SubWave {
    Sine,
    Triangle,
    Square,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CarrierWave {
    Sine,
    Triangle,
    Square,
    Sawtooth,
}

/// `CharacterParams`: one type for the presets and the chain that plays them
/// ([`crate::character::CharacterParams`], same serde form as the TS object).
pub use crate::character::CharacterParams;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OscParams {
    pub wave: OscWave,
    pub level: f64,
    pub semi: f64,
    pub fine: f64,
    pub unison: f64,
    pub spread: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partials: Option<Vec<f64>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubOscParams {
    pub level: f64,
    pub wave: SubWave,
    pub octave: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoiseParams {
    pub level: f64,
    #[serde(rename = "type")]
    pub kind: NoiseType,
    pub decay: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceFilterParams {
    #[serde(rename = "type")]
    pub kind: FilterType,
    pub cutoff: f64,
    pub resonance: f64,
    pub rolloff: f64,
    pub key_track: f64,
    pub env_amount: f64,
    pub velocity: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Adsr {
    pub attack: f64,
    pub decay: f64,
    pub sustain: f64,
    pub release: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AmpParams {
    pub attack: f64,
    pub decay: f64,
    pub sustain: f64,
    pub release: f64,
    pub velocity: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PitchEnvParams {
    pub amount: f64,
    pub decay: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LfoParams {
    pub rate: f64,
    pub vibrato: f64,
    pub filter: f64,
    pub delay: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Carrier {
    pub wave: CarrierWave,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PingParams {
    pub ratio: f64,
    pub level: f64,
    pub decay: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SampleKind {
    Piano,
}

/// Identity + common fields (`PresetBase`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PresetMeta {
    pub id: String,
    pub name: String,
    pub category: PresetCategory,
    pub vibe: PresetVibe,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub gain: f64,
    pub polyphony: f64,
    pub character: CharacterParams,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtractivePreset {
    #[serde(flatten)]
    pub meta: PresetMeta,
    pub osc_a: OscParams,
    pub osc_b: OscParams,
    pub sub: SubOscParams,
    pub noise: NoiseParams,
    pub filter: VoiceFilterParams,
    pub filter_env: Adsr,
    pub amp: AmpParams,
    pub pitch_env: PitchEnvParams,
    pub lfo: LfoParams,
    pub mono: bool,
    pub legato: bool,
    pub glide: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FmPreset {
    #[serde(flatten)]
    pub meta: PresetMeta,
    pub carrier: Carrier,
    pub ratio: f64,
    pub index: f64,
    pub mod_env: Adsr,
    pub velocity_to_index: f64,
    pub key_scale: f64,
    pub amp: AmpParams,
    pub ping: PingParams,
    pub detune: f64,
    pub lfo: LfoParams,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluckPreset {
    #[serde(flatten)]
    pub meta: PresetMeta,
    pub attack_noise: f64,
    pub dampening: f64,
    pub resonance: f64,
    pub release: f64,
    pub velocity: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SamplerPreset {
    #[serde(flatten)]
    pub meta: PresetMeta,
    pub sample: SampleKind,
    pub release: f64,
    pub velocity_curve: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "engine", rename_all = "lowercase")]
#[allow(clippy::large_enum_variant)] // control-side data, mirrors the TS union
pub enum SynthPreset {
    Subtractive(SubtractivePreset),
    Fm(FmPreset),
    Pluck(PluckPreset),
    Sampler(SamplerPreset),
}

impl SynthPreset {
    pub fn meta(&self) -> &PresetMeta {
        match self {
            SynthPreset::Subtractive(p) => &p.meta,
            SynthPreset::Fm(p) => &p.meta,
            SynthPreset::Pluck(p) => &p.meta,
            SynthPreset::Sampler(p) => &p.meta,
        }
    }

    pub fn meta_mut(&mut self) -> &mut PresetMeta {
        match self {
            SynthPreset::Subtractive(p) => &mut p.meta,
            SynthPreset::Fm(p) => &mut p.meta,
            SynthPreset::Pluck(p) => &mut p.meta,
            SynthPreset::Sampler(p) => &mut p.meta,
        }
    }

    pub fn id(&self) -> &str {
        &self.meta().id
    }

    pub fn engine(&self) -> &'static str {
        match self {
            SynthPreset::Subtractive(_) => "subtractive",
            SynthPreset::Fm(_) => "fm",
            SynthPreset::Pluck(_) => "pluck",
            SynthPreset::Sampler(_) => "sampler",
        }
    }

    /// The release tail (`amp.release`, or `release` for pluck / sampler).
    pub fn release_tail(&self) -> f64 {
        match self {
            SynthPreset::Subtractive(p) => p.amp.release,
            SynthPreset::Fm(p) => p.amp.release,
            SynthPreset::Pluck(p) => p.release,
            SynthPreset::Sampler(p) => p.release,
        }
    }

    /// The preset as its TS JSON object.
    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).expect("presets serialise")
    }

    pub fn from_json(v: &Value) -> Option<SynthPreset> {
        serde_json::from_value(v.clone()).ok()
    }
}

/// `PRESET_CATEGORIES`.
pub const PRESET_CATEGORIES: [(PresetCategory, &str); 9] = [
    (PresetCategory::Keys, "Keys"),
    (PresetCategory::Bass, "Bass"),
    (PresetCategory::Lead, "Lead"),
    (PresetCategory::Pad, "Pad"),
    (PresetCategory::Pluck, "Pluck"),
    (PresetCategory::Brass, "Brass"),
    (PresetCategory::Bell, "Bells & Mallets"),
    (PresetCategory::Percussion, "Percussion"),
    (PresetCategory::Fx, "FX"),
];

/// `PRESET_VIBES` (id, label, emoji).
pub const PRESET_VIBES: [(PresetVibe, &str, &str); 8] = [
    (PresetVibe::Chill, "Chill", "🌊"),
    (PresetVibe::Bright, "Bright & Sparkly", "✨"),
    (PresetVibe::Dark, "Dark & Moody", "🌙"),
    (PresetVibe::Punchy, "Punchy & Bold", "🔥"),
    (PresetVibe::Dreamy, "Dreamy & Lush", "☁️"),
    (PresetVibe::Aggressive, "Aggressive", "⚡"),
    (PresetVibe::Organic, "Organic & Acoustic", "🌿"),
    (PresetVibe::Experimental, "Experimental", "🔮"),
];

/// Number of presets the previous engine shipped (indices 0..35 keep their role).
pub const LEGACY_PRESET_COUNT: usize = 36;

/// `getPresetIndexById` (-1 when unknown).
pub fn preset_index_by_id(id: &str) -> i64 {
    super::bank::synth_presets()
        .iter()
        .position(|p| p.id() == id)
        .map_or(-1, |i| i as i64)
}

/// `getPresetById`.
pub fn preset_by_id(id: &str) -> Option<&'static SynthPreset> {
    super::bank::synth_presets().iter().find(|p| p.id() == id)
}

/// `presetAt`: a (possibly out-of-range / non-integer / NaN) saved index, truncated; anything
/// outside the bank is preset 0.
pub fn preset_at(index: f64) -> &'static SynthPreset {
    let bank = super::bank::synth_presets();
    let i = if index.is_finite() {
        index.trunc()
    } else {
        0.0
    };
    if i >= 0.0 && (i as usize) < bank.len() {
        &bank[i as usize]
    } else {
        &bank[0]
    }
}

/// `resolvePreset(index, overrides)`.
pub fn resolve_preset(index: f64, overrides: Option<&Value>) -> SynthPreset {
    super::params::merge_preset_params(preset_at(index), overrides)
}
