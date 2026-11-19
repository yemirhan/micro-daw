//! Shared data types. Ported from `src/types/{sampler,arrangement,effects,project,packs,
//! settings,samples,midi,appMode}.ts`.
//!
//! JSON compatibility rules (so `.mdaw` files move freely between the Electron and Rust apps):
//! - field names are the TypeScript names (camelCase);
//! - `T | null` fields serialize as `null`; optional (`?:`) fields are omitted when `None`;
//! - most fields have a serde default, so data with a missing or older field still decodes, and
//!   the `normalize_*` / `migrate_*` functions then fix it up exactly as the TypeScript code does;
//! - unknown keys are kept (`extra`) and written back unchanged on every object except
//!   `RegionNote` (see its docs);
//! - files and the autosave are NOT decoded with plain serde: they go through
//!   [`crate::open`] (TS migration, then a JS-semantics coercion of ill-typed values), because the
//!   Electron app opens data a strict decode rejects (`"volume": null`, `"lengthBeats": "100"`...).
//!   Plain serde is for data this crate wrote itself (undo snapshots, clipboard, golden args).

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

// ---------------------------------------------------------------------------
// Sampler (src/types/sampler.ts)
// ---------------------------------------------------------------------------

/// Number of sampler pads (2 banks of 8).
pub const PAD_COUNT: usize = 16;
/// Pads per bank. Bank A = pads 0..7, bank B = pads 8..15.
pub const PADS_PER_BANK: usize = 8;
/// Note number used inside drum regions for pad i (36 + i).
pub const PAD_BASE_NOTE: i32 = 36;

/// 0..15
pub type PadIndex = usize;

pub fn pad_to_note(pad: PadIndex) -> i32 {
    PAD_BASE_NOTE + pad as i32
}

/// `noteToPad` for real pad indexing: the pad of an integral note in 36..=51, else None.
///
/// The TS `noteToPad` only range-checks (`pad >= 0 && pad < 16`, no rounding), so
/// `noteToPad(36.5)` returns the non-index 0.5. Callers that index pads either round first
/// (`playDrumNote`, MIDI export) or get `undefined` from `pads[0.5]`, which is what `None` gives
/// here. Where the TS only tests *membership* (`noteToPad(n) !== null`, e.g. the pattern note
/// filter that `clearPattern` uses) use [`is_pad_note_range`] instead, which accepts 36.5.
pub fn note_to_pad(note: f64) -> Option<PadIndex> {
    let pad = note - PAD_BASE_NOTE as f64;
    if pad.fract() != 0.0 || !(0.0..PAD_COUNT as f64).contains(&pad) {
        return None;
    }
    Some(pad as usize)
}

/// The TS range predicate `noteToPad(note) !== null`: `note - 36` in `[0, 16)`, fractional notes
/// included (36.5 → true), NaN → false.
pub fn is_pad_note_range(note: f64) -> bool {
    let pad = note - PAD_BASE_NOTE as f64;
    pad >= 0.0 && pad < PAD_COUNT as f64
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FactoryVoiceId {
    #[serde(rename = "kick")]
    Kick,
    #[serde(rename = "kick-sub")]
    KickSub,
    #[serde(rename = "snare")]
    Snare,
    #[serde(rename = "rim")]
    Rim,
    #[serde(rename = "clap")]
    Clap,
    #[serde(rename = "chh")]
    Chh,
    #[serde(rename = "phh")]
    Phh,
    #[serde(rename = "ohh")]
    Ohh,
    #[serde(rename = "tom-lo")]
    TomLo,
    #[serde(rename = "tom-mid")]
    TomMid,
    #[serde(rename = "tom-hi")]
    TomHi,
    #[serde(rename = "crash")]
    Crash,
    #[serde(rename = "ride")]
    Ride,
    #[serde(rename = "shaker")]
    Shaker,
    #[serde(rename = "perc")]
    Perc,
    #[serde(rename = "snap")]
    Snap,
}

impl FactoryVoiceId {
    /// Declaration order of `FACTORY_VOICE_LABELS` (= `FACTORY_VOICE_IDS`).
    pub const ALL: [FactoryVoiceId; 16] = [
        Self::Kick,
        Self::KickSub,
        Self::Snare,
        Self::Rim,
        Self::Clap,
        Self::Chh,
        Self::Phh,
        Self::Ohh,
        Self::TomLo,
        Self::TomMid,
        Self::TomHi,
        Self::Crash,
        Self::Ride,
        Self::Shaker,
        Self::Perc,
        Self::Snap,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::Kick => "kick",
            Self::KickSub => "kick-sub",
            Self::Snare => "snare",
            Self::Rim => "rim",
            Self::Clap => "clap",
            Self::Chh => "chh",
            Self::Phh => "phh",
            Self::Ohh => "ohh",
            Self::TomLo => "tom-lo",
            Self::TomMid => "tom-mid",
            Self::TomHi => "tom-hi",
            Self::Crash => "crash",
            Self::Ride => "ride",
            Self::Shaker => "shaker",
            Self::Perc => "perc",
            Self::Snap => "snap",
        }
    }

    pub fn from_id(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.id() == s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClassicMachineId {
    Tr808,
    Tr909,
    Tr707,
    Tr606,
    Linn,
    Cr78,
}

impl ClassicMachineId {
    pub const ALL: [ClassicMachineId; 6] = [
        Self::Tr808,
        Self::Tr909,
        Self::Tr707,
        Self::Tr606,
        Self::Linn,
        Self::Cr78,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::Tr808 => "tr808",
            Self::Tr909 => "tr909",
            Self::Tr707 => "tr707",
            Self::Tr606 => "tr606",
            Self::Linn => "linn",
            Self::Cr78 => "cr78",
        }
    }

    pub fn from_id(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.id() == s)
    }
}

/// Where a pad's audio comes from. Tagged by `kind` exactly like the TS union.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum PadSampleSource {
    Factory {
        voice: FactoryVoiceId,
    },
    Classic {
        machine: ClassicMachineId,
        voice: String,
    },
    File {
        path: String,
        /// SampleManager id once loaded (runtime only, may be stale on reload).
        #[serde(rename = "sampleId", default, skip_serializing_if = "Option::is_none")]
        sample_id: Option<String>,
    },
    Pack {
        #[serde(rename = "packId")]
        pack_id: String,
        file: String,
    },
    /// A sound made in the Sound Designer (`Arrangement::sounds`), rendered from its recipe.
    Sound {
        #[serde(rename = "soundId")]
        sound_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PadSample {
    pub source: PadSampleSource,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PadPlayMode {
    #[default]
    Oneshot,
    Gate,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PadSettings {
    #[serde(default)]
    pub sample: Option<PadSample>,
    /// -60..+12 dB
    #[serde(default)]
    pub gain_db: f64,
    /// -1..1
    #[serde(default)]
    pub pan: f64,
    /// Semitones, -24..+24
    #[serde(default)]
    pub pitch: f64,
    #[serde(default)]
    pub start_sec: f64,
    #[serde(default)]
    pub end_sec: Option<f64>,
    /// 0.005..4 s
    #[serde(default = "default_decay")]
    pub decay: f64,
    /// 20..20000 Hz (20000 = open)
    #[serde(default = "default_cutoff")]
    pub filter_cutoff: f64,
    #[serde(default)]
    pub reverse: bool,
    #[serde(default)]
    pub mode: PadPlayMode,
    #[serde(default)]
    pub choke_group: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Sound shaping (amp AHDS envelope, filter type / resonance / envelope, pitch envelope,
    /// drive). Flat keys in the JSON; every key is omitted while it holds its neutral default,
    /// so a pad that was never shaped serializes exactly as before.
    #[serde(flatten)]
    pub shape: PadShape,
    /// Unknown keys, kept for round-trips (`normalizePad` drops them, as in the TS).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// The pad filter's response (`filterType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PadFilterType {
    #[default]
    Lp,
    Hp,
    Bp,
}

impl PadFilterType {
    pub fn is_lp(&self) -> bool {
        *self == Self::Lp
    }
}

/// Per-pad sound shaping (see `docs/rune-port/pad-sound-shaping.md`). Every default is
/// neutral: a pad whose shape is [`PadShape::is_neutral`] renders bit-identically to the
/// pre-shaping engine. The pad's `decay` is the envelope's release; `filterCutoff` is the
/// filter's base cutoff.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PadShape {
    /// Amp attack, 0..2 s (linear rise from silence).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub amp_attack: f64,
    /// Amp hold at full level after the attack, 0..2 s.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub amp_hold: f64,
    /// Amp decay from full level to the sustain level, 0.005..4 s.
    #[serde(
        default = "PadShape::default_amp_decay",
        skip_serializing_if = "PadShape::is_default_amp_decay"
    )]
    pub amp_decay: f64,
    /// Amp sustain level, 0..1 (1 = no decay stage).
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub amp_sustain: f64,
    #[serde(default, skip_serializing_if = "PadFilterType::is_lp")]
    pub filter_type: PadFilterType,
    /// Filter resonance, 0..1 (0 = the classic pad low-pass's Q).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub filter_resonance: f64,
    /// Filter envelope depth, -6..+6 octaves above / below the cutoff at the hit.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub filter_env: f64,
    /// Filter envelope decay, 0.005..4 s.
    #[serde(
        default = "PadShape::default_filter_env_decay",
        skip_serializing_if = "PadShape::is_default_filter_env_decay"
    )]
    pub filter_env_decay: f64,
    /// Pitch envelope depth, -24..+48 semitones at the hit, decaying to 0.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub pitch_env: f64,
    /// Pitch envelope decay, 0.005..2 s.
    #[serde(
        default = "PadShape::default_pitch_env_decay",
        skip_serializing_if = "PadShape::is_default_pitch_env_decay"
    )]
    pub pitch_env_decay: f64,
    /// Soft-clip drive, 0..1 (0 = bypassed, 1 = +30 dB into a tanh).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub drive: f64,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero(v: &f64) -> bool {
    *v == 0.0
}
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_one(v: &f64) -> bool {
    *v == 1.0
}
fn one() -> f64 {
    1.0
}

impl PadShape {
    pub const DEFAULT_AMP_DECAY: f64 = 0.3;
    pub const DEFAULT_FILTER_ENV_DECAY: f64 = 0.25;
    pub const DEFAULT_PITCH_ENV_DECAY: f64 = 0.08;

    fn default_amp_decay() -> f64 {
        Self::DEFAULT_AMP_DECAY
    }
    fn default_filter_env_decay() -> f64 {
        Self::DEFAULT_FILTER_ENV_DECAY
    }
    fn default_pitch_env_decay() -> f64 {
        Self::DEFAULT_PITCH_ENV_DECAY
    }
    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn is_default_amp_decay(v: &f64) -> bool {
        *v == Self::DEFAULT_AMP_DECAY
    }
    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn is_default_filter_env_decay(v: &f64) -> bool {
        *v == Self::DEFAULT_FILTER_ENV_DECAY
    }
    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn is_default_pitch_env_decay(v: &f64) -> bool {
        *v == Self::DEFAULT_PITCH_ENV_DECAY
    }

    /// The amp envelope is flat (no attack, no decay stage).
    pub fn amp_is_neutral(&self) -> bool {
        self.amp_attack <= 0.0 && self.amp_sustain >= 1.0
    }

    /// The filter is the classic pad low-pass (the cutoff alone).
    pub fn filter_is_neutral(&self) -> bool {
        self.filter_type == PadFilterType::Lp
            && self.filter_resonance <= 0.0
            && self.filter_env == 0.0
    }

    /// Nothing to shape: the pad plays through the classic (Chromium-parity) path.
    pub fn is_neutral(&self) -> bool {
        self.amp_is_neutral()
            && self.filter_is_neutral()
            && self.pitch_env == 0.0
            && self.drive <= 0.0
    }

    /// Every key, defaults included (a full patch: paste must overwrite the target's shape).
    pub fn to_full_json(&self) -> Map<String, Value> {
        let mut m = Map::new();
        let mut put = |k: &str, v: Value| {
            m.insert(k.to_owned(), v);
        };
        put("ampAttack", self.amp_attack.into());
        put("ampHold", self.amp_hold.into());
        put("ampDecay", self.amp_decay.into());
        put("ampSustain", self.amp_sustain.into());
        put(
            "filterType",
            serde_json::to_value(self.filter_type).unwrap_or(Value::Null),
        );
        put("filterResonance", self.filter_resonance.into());
        put("filterEnv", self.filter_env.into());
        put("filterEnvDecay", self.filter_env_decay.into());
        put("pitchEnv", self.pitch_env.into());
        put("pitchEnvDecay", self.pitch_env_decay.into());
        put("drive", self.drive.into());
        m
    }
}

impl Default for PadShape {
    fn default() -> Self {
        Self {
            amp_attack: 0.0,
            amp_hold: 0.0,
            amp_decay: Self::DEFAULT_AMP_DECAY,
            amp_sustain: 1.0,
            filter_type: PadFilterType::Lp,
            filter_resonance: 0.0,
            filter_env: 0.0,
            filter_env_decay: Self::DEFAULT_FILTER_ENV_DECAY,
            pitch_env: 0.0,
            pitch_env_decay: Self::DEFAULT_PITCH_ENV_DECAY,
            drive: 0.0,
        }
    }
}

fn default_decay() -> f64 {
    0.05
}
fn default_cutoff() -> f64 {
    20000.0
}

impl Default for PadSettings {
    /// `defaultPadSettings()` (factoryKit.ts:146-160).
    fn default() -> Self {
        Self {
            sample: None,
            gain_db: 0.0,
            pan: 0.0,
            pitch: 0.0,
            start_sec: 0.0,
            end_sec: None,
            decay: 0.05,
            filter_cutoff: 20000.0,
            reverse: false,
            mode: PadPlayMode::Oneshot,
            choke_group: 0,
            color: None,
            shape: PadShape::default(),
            extra: Map::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KitState {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// Always PAD_COUNT entries after normalization.
    #[serde(default)]
    pub pads: Vec<PadSettings>,
    /// Unknown keys, kept for round-trips (`normalizeKit` drops them, as in the TS).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PadParam {
    #[serde(rename = "gainDb")]
    GainDb,
    #[serde(rename = "pan")]
    Pan,
    #[serde(rename = "pitch")]
    Pitch,
    #[serde(rename = "decay")]
    Decay,
    #[serde(rename = "filterCutoff")]
    FilterCutoff,
    #[serde(rename = "startSec")]
    StartSec,
}

impl PadParam {
    pub fn id(self) -> &'static str {
        match self {
            Self::GainDb => "gainDb",
            Self::Pan => "pan",
            Self::Pitch => "pitch",
            Self::Decay => "decay",
            Self::FilterCutoff => "filterCutoff",
            Self::StartSec => "startSec",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum KnobTarget {
    #[serde(rename = "pad")]
    Pad { param: PadParam },
    #[serde(rename = "swing")]
    Swing,
    #[serde(rename = "bpm")]
    Bpm,
    #[serde(rename = "masterVolume")]
    MasterVolume,
    #[serde(rename = "trackVolume")]
    TrackVolume,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CCMapping {
    pub cc: i64,
    /// 1..16, or null = any channel
    pub channel: Option<i64>,
    pub target: KnobTarget,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PadMidiMap {
    /// notes[pad] = incoming MIDI note that triggers that pad (null = unassigned).
    pub notes: Vec<Option<i64>>,
    /// 1..16 or null = any channel
    pub channel: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControllerProfile {
    pub id: String,
    pub name: String,
    pub pad_map: PadMidiMap,
    pub cc_mappings: Vec<CCMapping>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum QuantizeGrid {
    #[serde(rename = "off")]
    Off,
    #[serde(rename = "1/4")]
    Q4,
    #[serde(rename = "1/8")]
    Q8,
    #[serde(rename = "1/8T")]
    Q8T,
    #[default]
    #[serde(rename = "1/16")]
    Q16,
    #[serde(rename = "1/16T")]
    Q16T,
    #[serde(rename = "1/32")]
    Q32,
}

impl QuantizeGrid {
    pub const ALL: [QuantizeGrid; 7] = [
        Self::Off,
        Self::Q4,
        Self::Q8,
        Self::Q8T,
        Self::Q16,
        Self::Q16T,
        Self::Q32,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Q4 => "1/4",
            Self::Q8 => "1/8",
            Self::Q8T => "1/8T",
            Self::Q16 => "1/16",
            Self::Q16T => "1/16T",
            Self::Q32 => "1/32",
        }
    }

    pub fn from_label(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|g| g.label() == s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum SwingGrid {
    #[serde(rename = "1/8")]
    S8,
    #[default]
    #[serde(rename = "1/16")]
    S16,
}

impl SwingGrid {
    pub fn label(self) -> &'static str {
        match self {
            Self::S8 => "1/8",
            Self::S16 => "1/16",
        }
    }
}

/// `LoopRecordSettings`. `bars` ∈ {1,2,4,8}, `countInBars` ∈ {0,1,2}. Deserializing goes through
/// `normalize_loop_settings`, so any JSON value yields valid settings (as in TS).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoopRecordSettings {
    pub bars: u32,
    pub quantize: QuantizeGrid,
    /// 0..0.75
    pub swing: f64,
    pub swing_grid: SwingGrid,
    pub count_in_bars: u32,
    pub metronome: bool,
    pub overdub: bool,
}

impl Default for LoopRecordSettings {
    /// `DEFAULT_LOOP_SETTINGS` (arrangementHelpers.ts:42-50).
    fn default() -> Self {
        Self {
            bars: 2,
            quantize: QuantizeGrid::Q16,
            swing: 0.0,
            swing_grid: SwingGrid::S16,
            count_in_bars: 1,
            metronome: true,
            overdub: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum NoteRepeatRate {
    #[serde(rename = "1/4")]
    R4,
    #[serde(rename = "1/8")]
    R8,
    #[serde(rename = "1/8T")]
    R8T,
    #[default]
    #[serde(rename = "1/16")]
    R16,
    #[serde(rename = "1/16T")]
    R16T,
    #[serde(rename = "1/32")]
    R32,
}

impl NoteRepeatRate {
    pub const ALL: [NoteRepeatRate; 6] = [
        Self::R4,
        Self::R8,
        Self::R8T,
        Self::R16,
        Self::R16T,
        Self::R32,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::R4 => "1/4",
            Self::R8 => "1/8",
            Self::R8T => "1/8T",
            Self::R16 => "1/16",
            Self::R16T => "1/16T",
            Self::R32 => "1/32",
        }
    }

    /// Step length in beats (NoteRepeat.ts:23-34).
    pub fn beats(self) -> f64 {
        match self {
            Self::R4 => 1.0,
            Self::R8 => 0.5,
            Self::R8T => 1.0 / 3.0,
            Self::R16 => 0.25,
            Self::R16T => 1.0 / 6.0,
            Self::R32 => 0.125,
        }
    }
}

// ---------------------------------------------------------------------------
// Effects (src/types/effects.ts)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FilterType {
    #[default]
    Lowpass,
    Highpass,
    Bandpass,
    Notch,
    Allpass,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LfoWaveform {
    #[default]
    Sine,
    Triangle,
    Square,
    Sawtooth,
}

/// Live-engine (global) effect parameters.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EffectParams {
    pub reverb_wet: f64,
    pub chorus_depth: f64,
    pub filter_cutoff: f64,
    pub filter_resonance: f64,
    pub filter_type: FilterType,
    pub filter_lfo_rate: f64,
    pub filter_lfo_depth: f64,
    pub filter_lfo_wave: LfoWaveform,
    pub delay_time: f64,
    pub delay_feedback: f64,
    pub delay_wet: f64,
    pub distortion_amount: f64,
    pub distortion_wet: f64,
    pub eq_low: f64,
    pub eq_mid: f64,
    pub eq_high: f64,
    pub comp_threshold: f64,
    pub comp_ratio: f64,
    pub comp_attack: f64,
    pub comp_release: f64,
}

impl Default for EffectParams {
    /// `DEFAULT_EFFECT_PARAMS`.
    fn default() -> Self {
        Self {
            reverb_wet: 0.12,
            chorus_depth: 0.0,
            filter_cutoff: 18000.0,
            filter_resonance: 1.0,
            filter_type: FilterType::Lowpass,
            filter_lfo_rate: 1.0,
            filter_lfo_depth: 0.0,
            filter_lfo_wave: LfoWaveform::Sine,
            delay_time: 0.25,
            delay_feedback: 0.3,
            delay_wet: 0.0,
            distortion_amount: 0.0,
            distortion_wet: 0.0,
            eq_low: 0.0,
            eq_mid: 0.0,
            eq_high: 0.0,
            comp_threshold: -24.0,
            comp_ratio: 1.0,
            comp_attack: 0.003,
            comp_release: 0.25,
        }
    }
}

// Every per-track effect struct defaults field by field (`#[serde(default)]` + the struct's
// `Default` = its `DEFAULT_TRACK_EFFECTS` entry), so the v1.2 / v1.3 shapes load: those versions
// wrote `filter` as `{ enabled, cutoff, resonance }`, and Electron 1.4 reads the missing fields
// through `?? 'lowpass'` / `?? 1` / `?? 'sine'` / `?? 0` fallbacks (src/utils/trackAudioFactory.ts:78-93,
// ArrangementEngine.ts:610-622), which are exactly these defaults. Unknown keys are kept (`extra`).

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReverbFx {
    pub enabled: bool,
    pub wet: f64,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
impl Default for ReverbFx {
    fn default() -> Self {
        Self {
            enabled: false,
            wet: 0.25,
            extra: Map::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DelayFx {
    pub enabled: bool,
    pub time: f64,
    pub feedback: f64,
    pub wet: f64,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
impl Default for DelayFx {
    fn default() -> Self {
        Self {
            enabled: false,
            time: 0.25,
            feedback: 0.3,
            wet: 0.3,
            extra: Map::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChorusFx {
    pub enabled: bool,
    pub depth: f64,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
impl Default for ChorusFx {
    fn default() -> Self {
        Self {
            enabled: false,
            depth: 0.5,
            extra: Map::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DistortionFx {
    pub enabled: bool,
    pub amount: f64,
    pub wet: f64,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
impl Default for DistortionFx {
    fn default() -> Self {
        Self {
            enabled: false,
            amount: 0.3,
            wet: 0.5,
            extra: Map::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EqFx {
    pub enabled: bool,
    pub low: f64,
    pub mid: f64,
    pub high: f64,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
impl Default for EqFx {
    fn default() -> Self {
        Self {
            enabled: false,
            low: 0.0,
            mid: 0.0,
            high: 0.0,
            extra: Map::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CompressorFx {
    pub enabled: bool,
    pub threshold: f64,
    pub ratio: f64,
    pub attack: f64,
    pub release: f64,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
impl Default for CompressorFx {
    fn default() -> Self {
        Self {
            enabled: false,
            threshold: -24.0,
            ratio: 4.0,
            attack: 0.003,
            release: 0.25,
            extra: Map::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FilterFx {
    pub enabled: bool,
    #[serde(rename = "type")]
    pub kind: FilterType,
    pub cutoff: f64,
    pub resonance: f64,
    pub lfo_rate: f64,
    pub lfo_depth: f64,
    pub lfo_wave: LfoWaveform,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
impl Default for FilterFx {
    fn default() -> Self {
        Self {
            enabled: false,
            kind: FilterType::Lowpass,
            cutoff: 18000.0,
            resonance: 1.0,
            lfo_rate: 1.0,
            lfo_depth: 0.0,
            lfo_wave: LfoWaveform::Sine,
            extra: Map::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TrackEffectState {
    pub reverb: ReverbFx,
    pub delay: DelayFx,
    pub chorus: ChorusFx,
    pub distortion: DistortionFx,
    pub eq: EqFx,
    pub compressor: CompressorFx,
    pub filter: FilterFx,
    /// Unknown keys, kept for round-trips.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

// ---------------------------------------------------------------------------
// Arrangement (src/types/arrangement.ts)
// ---------------------------------------------------------------------------

/// A note. Deliberately WITHOUT an `extra` map (unlike the other model structs): notes are by far
/// the most numerous objects (cloned on every edit / undo snapshot / Part rebuild), the TS never
/// writes keys beyond these five, and a `#[serde(flatten)]` map would force serde's buffered
/// (slower) path and 24+ bytes per note. Unknown note keys (only ever seen in hand-edited files)
/// are dropped on load.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionNote {
    pub note: f64,
    pub velocity: f64,
    /// relative to region start
    pub start_beat: f64,
    pub duration_beats: f64,
    #[serde(default)]
    pub is_drum: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioRegionData {
    pub sample_id: String,
    pub sample_path: String,
    #[serde(default)]
    pub offset_seconds: f64,
    #[serde(default)]
    pub gain_db: f64,
    /// seconds
    #[serde(default)]
    pub original_duration: f64,
    /// Unknown keys, kept for round-trips.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Region {
    pub id: String,
    pub start_beat: f64,
    pub length_beats: f64,
    #[serde(default)]
    pub notes: Vec<RegionNote>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<AudioRegionData>,
    /// Unknown keys, kept for round-trips.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InstrumentType {
    #[default]
    Synth,
    Drums,
    Audio,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackInstrument {
    #[serde(rename = "type")]
    pub kind: InstrumentType,
    #[serde(default)]
    pub preset_index: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub synth_overrides: Option<Map<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kit: Option<KitState>,
    /// A synth track played by a hosted plugin instrument instead of the built-in preset
    /// (`presetIndex` is kept: it comes back when the plugin is removed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin: Option<PluginSlot>,
    /// Unknown keys, kept for round-trips.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// A hosted plugin on a track: an insert effect ([`Track::plugins`]) or the track's instrument
/// ([`TrackInstrument::plugin`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginSlot {
    /// This instance's id (unique in the project; the loaded plugin is keyed by it).
    pub id: String,
    /// The plugin format: `"au"` (Audio Unit).
    #[serde(default = "default_plugin_format")]
    pub format: String,
    /// The component's four-character codes (`"aufx"`, `"Ni$R"`, `"-NI-"`).
    #[serde(rename = "type")]
    pub kind: String,
    pub subtype: String,
    pub manufacturer: String,
    /// Shown while the plugin is not installed, too.
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub vendor: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bypassed: bool,
    /// The plugin's own state (its preset blob), base64. None: the plugin's defaults.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// Unknown keys, kept for round-trips.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

fn default_plugin_format() -> String {
    "au".to_owned()
}

/// Which device input channels an audio track listens to (and records) while armed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackInput {
    /// First channel, 0-based ("Input 1" = 0).
    pub channel: u32,
    /// Also the next channel, as the right side of a stereo pair.
    #[serde(default)]
    pub stereo: bool,
}

impl TrackInput {
    /// "In 1", "In 3/4".
    pub fn label(&self) -> String {
        if self.stereo {
            format!("In {}/{}", self.channel + 1, self.channel + 2)
        } else {
            format!("In {}", self.channel + 1)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AutomationParameter {
    #[serde(rename = "volume")]
    Volume,
    #[serde(rename = "pan")]
    Pan,
    #[serde(rename = "reverbWet")]
    ReverbWet,
    #[serde(rename = "delayWet")]
    DelayWet,
    #[serde(rename = "chorusDepth")]
    ChorusDepth,
    #[serde(rename = "distortionWet")]
    DistortionWet,
    #[serde(rename = "filterCutoff")]
    FilterCutoff,
    #[serde(rename = "eqLow")]
    EqLow,
    #[serde(rename = "eqMid")]
    EqMid,
    #[serde(rename = "eqHigh")]
    EqHigh,
}

impl AutomationParameter {
    pub const ALL: [AutomationParameter; 10] = [
        Self::Volume,
        Self::Pan,
        Self::ReverbWet,
        Self::DelayWet,
        Self::ChorusDepth,
        Self::DistortionWet,
        Self::FilterCutoff,
        Self::EqLow,
        Self::EqMid,
        Self::EqHigh,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AutomationPoint {
    pub beat: f64,
    /// normalized 0-1
    pub value: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutomationLane {
    pub parameter: AutomationParameter,
    #[serde(default)]
    pub points: Vec<AutomationPoint>,
    /// A missing flag is falsy in the TS (`lanes.filter(l => l.visible)`): hidden.
    #[serde(default)]
    pub visible: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub instrument: TrackInstrument,
    #[serde(default)]
    pub regions: Vec<Region>,
    #[serde(default)]
    pub volume: f64,
    /// -1 to 1, 0 = center
    #[serde(default)]
    pub pan: f64,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub solo: bool,
    #[serde(default)]
    pub color: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effects: Option<TrackEffectState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub automation: Option<Vec<AutomationLane>>,
    /// Insert effects (hosted plugins), in signal order: after the instrument / the monitored
    /// input, before the track's own effects.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plugins: Vec<PluginSlot>,
    /// Audio tracks: the device input heard and recorded while the track is armed (None:
    /// input 1, mono).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<TrackInput>,
    /// Unknown keys, kept for round-trips.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Track {
    /// The input an armed audio track listens to.
    pub fn input_or_default(&self) -> TrackInput {
        self.input.unwrap_or_default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoopMarkers {
    pub start_beat: f64,
    pub end_beat: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    pub id: String,
    pub name: String,
    pub beat: f64,
    pub color: String,
    /// Unknown keys, kept for round-trips.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackGroup {
    pub id: String,
    pub name: String,
    pub color: String,
    pub track_ids: Vec<String>,
    #[serde(default)]
    pub collapsed: bool,
    /// Unknown keys, kept for round-trips.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Deserializing is lenient (any JSON value works). The active id is checked against the tracks
/// later by `normalize_beat_state` during migration.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BeatState {
    pub active_drum_track_id: Option<String>,
    pub r#loop: LoopRecordSettings,
    pub selected_pad: PadIndex,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Arrangement {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub bpm: f64,
    /// Missing → `DEFAULT_ARRANGEMENT_LENGTH` (as `crate::open` coerces it).
    #[serde(default = "default_length_beats")]
    pub length_beats: f64,
    #[serde(default)]
    pub tracks: Vec<Track>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_markers: Option<LoopMarkers>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub markers: Option<Vec<Marker>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub groups: Option<Vec<TrackGroup>>,
    /// Beat-mode state (active drum track, loop recorder settings, selected pad).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beat: Option<BeatState>,
    /// Sounds made in the Sound Designer (their recipes; pads and audio regions refer to them
    /// by id). Missing in older projects.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sounds: Vec<crate::sound_design::DesignedSound>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

fn default_length_beats() -> f64 {
    64.0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransportState {
    Stopped,
    Playing,
    Recording,
}

// ---------------------------------------------------------------------------
// Project (src/types/project.ts)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectAudioConfig {
    /// dB
    pub master_volume: f64,
    pub effect_params: EffectParams,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SampleManifestEntry {
    pub sample_id: String,
    pub path: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFile {
    /// Always "1.0.0".
    pub version: String,
    pub arrangement: Arrangement,
    pub audio_config: ProjectAudioConfig,
    /// ISO 8601
    pub created_at: String,
    /// ISO 8601
    pub modified_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_manifest: Option<Vec<SampleManifestEntry>>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

pub const PROJECT_VERSION: &str = "1.0.0";
pub const PROJECT_EXTENSION: &str = "mdaw";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMeta {
    pub file_path: String,
    pub name: String,
    pub modified_at: String,
}

// ---------------------------------------------------------------------------
// Templates (src/types/templates.ts)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTemplate {
    pub id: String,
    pub name: String,
    pub category: String,
    pub description: String,
    pub bpm: f64,
    pub track_count: u32,
    pub arrangement: Arrangement,
}

// ---------------------------------------------------------------------------
// App mode, settings, samples, MIDI
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppMode {
    #[default]
    Daw,
    Samples,
    Packs,
    Settings,
    Dev,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DawView {
    #[default]
    Beat,
    Song,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneralSettings {
    pub auto_check_updates: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioSettings {
    pub default_master_volume: f64,
    /// 128 | 256 | 512 | 1024 | 2048
    pub buffer_size_hint: u32,
    /// The audio output device by name (None: the system default, followed when it changes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_device: Option<String>,
    /// The device channel the left side plays on (0 = outputs 1/2, 2 = outputs 3/4).
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub output_channel: u32,
    /// The audio input device by name (None: the system default input).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_device: Option<String>,
}

fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiSettings {
    pub auto_connect_last_device: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppSettings {
    pub general: GeneralSettings,
    pub audio: AudioSettings,
    pub midi: MidiSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum UpdateStatus {
    Idle,
    Checking,
    Available { version: String },
    NotAvailable,
    Downloading { percent: f64 },
    Downloaded { version: String },
    Error { message: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SampleRef {
    pub id: String,
    pub name: String,
    pub path: String,
    pub duration_seconds: f64,
    pub sample_rate: f64,
    pub channels: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SampleTrim {
    pub start_seconds: f64,
    pub end_seconds: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SampleLibraryEntry {
    pub sample: SampleRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trim: Option<SampleTrim>,
    pub added_at: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MidiDeviceInfo {
    pub id: String,
    pub name: String,
    pub manufacturer: String,
    pub connected: bool,
}

impl<'de> Deserialize<'de> for LoopRecordSettings {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        Ok(crate::loop_settings::normalize_loop_settings(
            &v,
            &LoopRecordSettings::default(),
        ))
    }
}

impl<'de> Deserialize<'de> for BeatState {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        Ok(crate::loop_settings::beat_state_from_value(&v))
    }
}
