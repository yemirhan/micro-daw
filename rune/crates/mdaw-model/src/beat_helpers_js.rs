//! JavaScript numeric semantics and small private copies of helpers owned by other modules,
//! used by the beat helpers so their output matches the TypeScript app byte for byte.
//!
//! - `js_min` / `js_max`: `Math.min` / `Math.max` (NaN propagates, `+0 > -0`).
//! - `js_number_to_string`: `String(x)` for a number.
//! - `js_to_fixed`: `Number.prototype.toFixed` (exact decimal value, ties away from zero).
//!
//! Private copies (the owning modules are ported separately; keep in sync):
//! `displayName` / `isAudioFileName` (src/shared/paths.ts, src/shared/fsEntries.ts),
//! `STEP_EPSILON` / `patternStepAt` (src/utils/arrangementHelpers.ts),
//! `FACTORY_VOICE_LABELS` / `PAD_COLORS` / `FACTORY_PAD_VOICES` (src/audio/sampler/factoryKit.ts),
//! `classicSampleName` (src/audio/sampler/classic/machines.ts).

use crate::loop_settings::js_round;
use crate::types::{ClassicMachineId, FactoryVoiceId};

/// `Math.max(a, b)`.
pub(crate) fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if a == b {
        // +0 wins over -0.
        return if a.is_sign_positive() { a } else { b };
    }
    if a > b { a } else { b }
}

/// `Math.min(a, b)`.
pub(crate) fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if a == b {
        return if a.is_sign_negative() { a } else { b };
    }
    if a < b { a } else { b }
}

/// `Math.min(hi, Math.max(lo, v))`.
pub(crate) fn js_clamp(v: f64, lo: f64, hi: f64) -> f64 {
    js_min(hi, js_max(lo, v))
}

/// `String(x)` / template-literal formatting of a JS number.
pub fn js_number_to_string(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    if x == 0.0 {
        return "0".into();
    }
    let sign = if x < 0.0 { "-" } else { "" };
    // Shortest round-trip digits, e.g. "1.2345e-7".
    let sci = format!("{:e}", x.abs());
    let (mant, exp) = sci.split_once('e').expect("LowerExp always has an exponent");
    let exp: i32 = exp.parse().expect("integer exponent");
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let n = exp + 1;
    let body = if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let e = n - 1;
        let es = if e >= 0 { format!("+{e}") } else { format!("-{}", -e) };
        if k == 1 { format!("{digits}e{es}") } else { format!("{}.{}e{es}", &digits[..1], &digits[1..]) }
    };
    format!("{sign}{body}")
}

/// `x.toFixed(digits)` (0 ≤ digits ≤ 100).
pub fn js_to_fixed(x: f64, digits: usize) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.abs() >= 1e21 || x.is_infinite() {
        return js_number_to_string(x);
    }
    let neg = x < 0.0;
    let a = x.abs();
    // Exact decimal expansion (a double has at most 1074 fractional binary digits).
    let exact = format!("{a:.1100}");
    let (int_part, frac_part) = exact.split_once('.').expect("has a fraction");
    let mut kept: Vec<u8> = int_part.bytes().chain(frac_part.bytes().take(digits)).collect();
    // Round half up on the exact value ("if there are two such n, pick the larger n").
    if frac_part.as_bytes()[digits] >= b'5' {
        let mut i = kept.len();
        loop {
            if i == 0 {
                kept.insert(0, b'1');
                break;
            }
            i -= 1;
            if kept[i] == b'9' {
                kept[i] = b'0';
            } else {
                kept[i] += 1;
                break;
            }
        }
    }
    let int_len = kept.len() - digits;
    let s = String::from_utf8(kept).expect("ascii digits");
    let body = if digits == 0 { s } else { format!("{}.{}", &s[..int_len], &s[int_len..]) };
    if neg { format!("-{body}") } else { body }
}

/// `Math.round(x)` printed with `String()`.
pub(crate) fn js_round_string(x: f64) -> String {
    js_number_to_string(js_round(x))
}

// ---------------------------------------------------------------------------
// Private copies of helpers owned by other modules
// ---------------------------------------------------------------------------

/// `STEP_EPSILON` (arrangementHelpers.ts:37).
pub const STEP_EPSILON: f64 = 1e-4;

/// `clean` (arrangementHelpers.ts:78): round to 1e-9.
fn clean(beat: f64) -> f64 {
    js_round(beat * 1e9) / 1e9
}

/// `patternStepAt` (arrangementHelpers.ts:86-89).
pub(crate) fn pattern_step_at(beat: f64, step_beats: f64, steps: f64) -> f64 {
    let step = js_round(clean(js_max(0.0, beat) / step_beats));
    js_min(js_max(0.0, steps - 1.0), js_max(0.0, step))
}

/// `displayName` (shared/paths.ts): file name without its last extension (unless the dot is at 0).
pub(crate) fn display_name(file_name: &str) -> String {
    // `lastIndexOf('.') > 0` — byte index 0 is the same position as UTF-16 index 0.
    match file_name.rfind('.') {
        Some(dot) if dot > 0 => file_name[..dot].to_string(),
        _ => file_name.to_string(),
    }
}

/// `AUDIO_EXTENSIONS` (shared/fsEntries.ts:9).
const AUDIO_EXTENSIONS: [&str; 7] = ["wav", "aif", "aiff", "mp3", "flac", "ogg", "m4a"];

/// `extOf` + `isAudioFileName` (shared/fsEntries.ts:12-20).
pub(crate) fn is_audio_file_name(name: &str) -> bool {
    let ext = match name.rfind('.') {
        Some(dot) if dot > 0 && dot != name.len() - 1 => name[dot + 1..].to_lowercase(),
        _ => String::new(),
    };
    AUDIO_EXTENSIONS.contains(&ext.as_str())
}

/// `FACTORY_VOICE_LABELS` (factoryKit.ts:41-58).
pub(crate) fn factory_voice_label(voice: FactoryVoiceId) -> &'static str {
    match voice {
        FactoryVoiceId::Kick => "Kick",
        FactoryVoiceId::KickSub => "Sub Kick",
        FactoryVoiceId::Snare => "Snare",
        FactoryVoiceId::Rim => "Rim",
        FactoryVoiceId::Clap => "Clap",
        FactoryVoiceId::Chh => "Closed HH",
        FactoryVoiceId::Phh => "Pedal HH",
        FactoryVoiceId::Ohh => "Open HH",
        FactoryVoiceId::TomLo => "Low Tom",
        FactoryVoiceId::TomMid => "Mid Tom",
        FactoryVoiceId::TomHi => "High Tom",
        FactoryVoiceId::Crash => "Crash",
        FactoryVoiceId::Ride => "Ride",
        FactoryVoiceId::Shaker => "Shaker",
        FactoryVoiceId::Perc => "Cowbell",
        FactoryVoiceId::Snap => "Snap",
    }
}

/// `FACTORY_PAD_VOICES` (factoryKit.ts:66-69).
pub(crate) const FACTORY_PAD_VOICES: [FactoryVoiceId; 16] = {
    use FactoryVoiceId::*;
    [Kick, Snare, Chh, Ohh, Clap, TomMid, Crash, Ride, KickSub, Rim, Phh, Shaker, Snap, TomLo, TomHi, Perc]
};

/// `PAD_COLORS` (factoryKit.ts:75-92).
pub const PAD_COLORS: [&str; 16] = [
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

type VoiceLabels = &'static [(&'static str, &'static str)];

/// Machine display names and voice labels (classic/machines.ts `CLASSIC_MACHINES`).
const CLASSIC_LABELS: [(ClassicMachineId, &str, VoiceLabels); 6] = [
    (
        ClassicMachineId::Tr808,
        "808-style",
        &[
            ("kick", "Kick"),
            ("kick-long", "Long Boom"),
            ("snare", "Snare"),
            ("rim", "Rimshot"),
            ("clap", "Clap"),
            ("chh", "Closed Hat"),
            ("ohh", "Open Hat"),
            ("cymbal", "Cymbal"),
            ("cowbell", "Cowbell"),
            ("clave", "Clave"),
            ("maracas", "Maracas"),
            ("tom-lo", "Low Tom"),
            ("tom-mid", "Mid Tom"),
            ("tom-hi", "High Tom"),
            ("conga-lo", "Low Conga"),
            ("conga-mid", "Mid Conga"),
            ("conga-hi", "High Conga"),
        ],
    ),
    (
        ClassicMachineId::Tr909,
        "909-style",
        &[
            ("kick", "Kick"),
            ("kick-long", "Long Kick"),
            ("kick-hard", "Hard Kick"),
            ("snare", "Snare"),
            ("snare-tight", "Tight Snare"),
            ("snare-low", "Low Snare"),
            ("rim", "Rimshot"),
            ("clap", "Clap"),
            ("chh", "Closed Hat"),
            ("phh", "Pedal Hat"),
            ("ohh", "Open Hat"),
            ("crash", "Crash"),
            ("ride", "Ride"),
            ("tom-lo", "Low Tom"),
            ("tom-mid", "Mid Tom"),
            ("tom-hi", "High Tom"),
        ],
    ),
    (
        ClassicMachineId::Tr707,
        "707-style",
        &[
            ("kick", "Kick 1"),
            ("kick-2", "Kick 2"),
            ("snare", "Snare 1"),
            ("snare-2", "Snare 2"),
            ("rim", "Rimshot"),
            ("clap", "Clap"),
            ("cowbell", "Cowbell"),
            ("tambourine", "Tambourine"),
            ("chh", "Closed Hat"),
            ("ohh", "Open Hat"),
            ("crash", "Crash"),
            ("ride", "Ride"),
            ("ride-bell", "Ride Bell"),
            ("tom-lo", "Low Tom"),
            ("tom-mid", "Mid Tom"),
            ("tom-hi", "High Tom"),
        ],
    ),
    (
        ClassicMachineId::Tr606,
        "606-style",
        &[
            ("kick", "Kick"),
            ("kick-long", "Long Kick"),
            ("kick-short", "Tight Kick"),
            ("snare", "Snare"),
            ("snare-snappy", "Snappy Snare"),
            ("rim", "Rim"),
            ("clap", "Clap"),
            ("chh", "Closed Hat"),
            ("phh", "Pedal Hat"),
            ("ohh", "Open Hat"),
            ("cymbal", "Cymbal"),
            ("shaker", "Shaker"),
            ("zap", "Zap"),
            ("tom-lo", "Low Tom"),
            ("tom-mid", "Mid Tom"),
            ("tom-hi", "High Tom"),
        ],
    ),
    (
        ClassicMachineId::Linn,
        "LinnDrum-style",
        &[
            ("kick", "Kick"),
            ("snare", "Snare"),
            ("sidestick", "Sidestick"),
            ("clap", "Claps"),
            ("chh", "Closed Hat"),
            ("ohh", "Open Hat"),
            ("ride", "Ride"),
            ("crash", "Crash"),
            ("tom-lo", "Low Tom"),
            ("tom-mid", "Mid Tom"),
            ("tom-hi", "High Tom"),
            ("conga-lo", "Low Conga"),
            ("conga-hi", "High Conga"),
            ("cabasa", "Cabasa"),
            ("tambourine", "Tambourine"),
            ("cowbell", "Cowbell"),
        ],
    ),
    (
        ClassicMachineId::Cr78,
        "CR-78-style",
        &[
            ("kick", "Kick"),
            ("snare", "Snare"),
            ("rim", "Rim"),
            ("chh", "Hi-Hat"),
            ("ohh", "Open Hat"),
            ("cymbal", "Cymbal"),
            ("metal", "Metal Beat"),
            ("maracas", "Maracas"),
            ("guiro", "Guiro"),
            ("tambourine", "Tambourine"),
            ("clave", "Clave"),
            ("cowbell", "Cowbell"),
            ("bongo-lo", "Low Bongo"),
            ("bongo-hi", "High Bongo"),
            ("conga-lo", "Low Conga"),
            ("conga-hi", "High Conga"),
        ],
    ),
];

/// `classicSampleName(machine, voice)`: "808 Kick"; an unknown voice id is used as its label.
pub(crate) fn classic_sample_name(machine: ClassicMachineId, voice: &str) -> String {
    let (_, name, voices) = CLASSIC_LABELS.iter().find(|(m, _, _)| *m == machine).expect("all machines listed");
    let short = name.strip_suffix("-style").unwrap_or(name);
    let label = voices.iter().find(|(id, _)| *id == voice).map_or(voice, |(_, l)| l);
    format!("{short} {label}")
}
