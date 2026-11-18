//! Metadata of the built-in classic-machine kits (no DSP). Port of
//! `src/audio/sampler/classic/machines.ts`. These are ORIGINAL syntheses of the *style* of classic
//! drum machines: always named "808-style" etc., never as the trademarked product.

use serde_json::Value;

use crate::types::{ClassicMachineId, PadPlayMode};

/// `ClassicVoiceInfo`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClassicVoiceInfo {
    /// Voice id, unique within its machine (stored in `{ kind: 'classic', voice }`).
    pub id: &'static str,
    pub label: &'static str,
    /// General MIDI drum note that best represents the voice (MIDI export).
    pub gm: i32,
    /// Default pad gain in dB (balances the kit).
    pub gain_db: Option<f64>,
    /// Choke group on the default kit (1 = hi-hats).
    pub choke: Option<i64>,
    pub mode: Option<PadPlayMode>,
}

/// `ClassicMachine`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClassicMachine {
    pub id: ClassicMachineId,
    /// Display name, e.g. "808-style".
    pub name: &'static str,
    pub tagline: &'static str,
    pub description: &'static str,
    /// Accent color (CSS).
    pub color: &'static str,
    pub voices: &'static [ClassicVoiceInfo],
    /// Default 16-pad layout (voice ids).
    pub pads: [&'static str; 16],
}

const fn v(
    id: &'static str,
    label: &'static str,
    gm: i32,
    gain_db: Option<f64>,
    choke: Option<i64>,
    mode: Option<PadPlayMode>,
) -> ClassicVoiceInfo {
    ClassicVoiceInfo { id, label, gm, gain_db, choke, mode }
}

/// `CLASSIC_MACHINES` (declaration order is the UI order). Hats use `HAT = { choke: 1, gainDb: -3 }`.
pub static CLASSIC_MACHINES: [ClassicMachine; 6] = [
    ClassicMachine {
        id: ClassicMachineId::Tr808,
        name: "808-style",
        tagline: "Booming analog kick, snappy snare, metallic hats",
        description: "Analog-modelled voices in the spirit of the 1980 classic: resonator kick with a long tuned tail, two-tone snare with noise, six-oscillator metallic hats and cymbal, multi-burst clap, cowbell, congas and clave.",
        color: "oklch(0.70 0.18 45)",
        voices: &[
            v("kick", "Kick", 36, None, None, None),
            v("kick-long", "Long Boom", 35, None, None, Some(PadPlayMode::Oneshot)),
            v("snare", "Snare", 38, None, None, None),
            v("rim", "Rimshot", 37, Some(-2.0), None, None),
            v("clap", "Clap", 39, None, None, None),
            v("chh", "Closed Hat", 42, Some(-3.0), Some(1), None),
            v("ohh", "Open Hat", 46, Some(-3.0), Some(1), None),
            v("cymbal", "Cymbal", 49, Some(-4.0), None, None),
            v("cowbell", "Cowbell", 56, Some(-3.0), None, None),
            v("clave", "Clave", 75, Some(-4.0), None, None),
            v("maracas", "Maracas", 70, Some(-5.0), None, None),
            v("tom-lo", "Low Tom", 41, None, None, None),
            v("tom-mid", "Mid Tom", 45, None, None, None),
            v("tom-hi", "High Tom", 48, None, None, None),
            v("conga-lo", "Low Conga", 64, Some(-2.0), None, None),
            v("conga-mid", "Mid Conga", 63, Some(-2.0), None, None),
            v("conga-hi", "High Conga", 62, Some(-2.0), None, None),
        ],
        pads: [
            "kick",
            "snare",
            "chh",
            "ohh",
            "clap",
            "tom-mid",
            "cymbal",
            "cowbell",
            "kick-long",
            "rim",
            "maracas",
            "clave",
            "tom-lo",
            "tom-hi",
            "conga-mid",
            "conga-hi",
        ],
    },
    ClassicMachine {
        id: ClassicMachineId::Tr909,
        name: "909-style",
        tagline: "Punchy kick with click, noisy snare, gritty hats",
        description: "Hybrid analog/digital style: kick with a fast pitch drop and beater click, noise-heavy snares, tuned toms, and crunchy low-bit-depth hats, crash and ride.",
        color: "oklch(0.76 0.15 85)",
        voices: &[
            v("kick", "Kick", 36, None, None, None),
            v("kick-long", "Long Kick", 35, None, None, None),
            v("kick-hard", "Hard Kick", 36, None, None, None),
            v("snare", "Snare", 38, None, None, None),
            v("snare-tight", "Tight Snare", 40, None, None, None),
            v("snare-low", "Low Snare", 38, None, None, None),
            v("rim", "Rimshot", 37, Some(-2.0), None, None),
            v("clap", "Clap", 39, None, None, None),
            v("chh", "Closed Hat", 42, Some(-3.0), Some(1), None),
            v("phh", "Pedal Hat", 44, Some(-3.0), Some(1), None),
            v("ohh", "Open Hat", 46, Some(-3.0), Some(1), None),
            v("crash", "Crash", 49, Some(-4.0), None, None),
            v("ride", "Ride", 51, Some(-5.0), None, None),
            v("tom-lo", "Low Tom", 41, None, None, None),
            v("tom-mid", "Mid Tom", 45, None, None, None),
            v("tom-hi", "High Tom", 48, None, None, None),
        ],
        pads: [
            "kick",
            "snare",
            "chh",
            "ohh",
            "clap",
            "tom-mid",
            "crash",
            "ride",
            "kick-long",
            "rim",
            "phh",
            "snare-tight",
            "kick-hard",
            "tom-lo",
            "tom-hi",
            "snare-low",
        ],
    },
    ClassicMachine {
        id: ClassicMachineId::Tr707,
        name: "707-style",
        tagline: "Crisp digital drums with an 8-bit edge",
        description: "Punchy, sample-like voices with short, natural envelopes and a gentle early-digital crunch: two kicks, two snares, toms, rim, cowbell, clap, tambourine, hats, crash and ride.",
        color: "oklch(0.64 0.19 22)",
        voices: &[
            v("kick", "Kick 1", 36, None, None, None),
            v("kick-2", "Kick 2", 35, None, None, None),
            v("snare", "Snare 1", 38, None, None, None),
            v("snare-2", "Snare 2", 40, None, None, None),
            v("rim", "Rimshot", 37, Some(-2.0), None, None),
            v("clap", "Clap", 39, None, None, None),
            v("cowbell", "Cowbell", 56, Some(-3.0), None, None),
            v("tambourine", "Tambourine", 54, Some(-4.0), None, None),
            v("chh", "Closed Hat", 42, Some(-3.0), Some(1), None),
            v("ohh", "Open Hat", 46, Some(-3.0), Some(1), None),
            v("crash", "Crash", 49, Some(-4.0), None, None),
            v("ride", "Ride", 51, Some(-5.0), None, None),
            v("ride-bell", "Ride Bell", 53, Some(-5.0), None, None),
            v("tom-lo", "Low Tom", 41, None, None, None),
            v("tom-mid", "Mid Tom", 45, None, None, None),
            v("tom-hi", "High Tom", 48, None, None, None),
        ],
        pads: [
            "kick",
            "snare",
            "chh",
            "ohh",
            "clap",
            "tom-mid",
            "crash",
            "ride",
            "kick-2",
            "rim",
            "snare-2",
            "tambourine",
            "cowbell",
            "tom-lo",
            "tom-hi",
            "ride-bell",
        ],
    },
    ClassicMachine {
        id: ClassicMachineId::Tr606,
        name: "606-style",
        tagline: "Thin, tight, ticking analog drums",
        description: "Lean analog voices: short tight kick, bright noisy snare, very tight metallic hats and a splashy cymbal, plus a few companions (clap, rim, shaker, zap) in the same thin style.",
        color: "oklch(0.72 0.13 205)",
        voices: &[
            v("kick", "Kick", 36, None, None, None),
            v("kick-long", "Long Kick", 35, None, None, None),
            v("kick-short", "Tight Kick", 36, None, None, None),
            v("snare", "Snare", 38, None, None, None),
            v("snare-snappy", "Snappy Snare", 40, None, None, None),
            v("rim", "Rim", 37, Some(-2.0), None, None),
            v("clap", "Clap", 39, None, None, None),
            v("chh", "Closed Hat", 42, Some(-3.0), Some(1), None),
            v("phh", "Pedal Hat", 44, Some(-3.0), Some(1), None),
            v("ohh", "Open Hat", 46, Some(-3.0), Some(1), None),
            v("cymbal", "Cymbal", 49, Some(-4.0), None, None),
            v("shaker", "Shaker", 82, Some(-5.0), None, None),
            v("zap", "Zap", 76, Some(-3.0), None, None),
            v("tom-lo", "Low Tom", 41, None, None, None),
            v("tom-mid", "Mid Tom", 45, None, None, None),
            v("tom-hi", "High Tom", 48, None, None, None),
        ],
        pads: [
            "kick",
            "snare",
            "chh",
            "ohh",
            "clap",
            "tom-mid",
            "cymbal",
            "phh",
            "kick-long",
            "rim",
            "snare-snappy",
            "shaker",
            "zap",
            "tom-lo",
            "tom-hi",
            "kick-short",
        ],
    },
    ClassicMachine {
        id: ClassicMachineId::Linn,
        name: "LinnDrum-style",
        tagline: "80s studio drums, 8-bit companded",
        description: "Acoustic-modelled drums with the companded 8-bit sheen of early-80s sample machines: dry punchy kick, fat snare, sidestick, real-sounding claps, hats, ride, crash, toms, congas, cabasa, tambourine and cowbell.",
        color: "oklch(0.62 0.17 295)",
        voices: &[
            v("kick", "Kick", 36, None, None, None),
            v("snare", "Snare", 38, None, None, None),
            v("sidestick", "Sidestick", 37, Some(-2.0), None, None),
            v("clap", "Claps", 39, None, None, None),
            v("chh", "Closed Hat", 42, Some(-3.0), Some(1), None),
            v("ohh", "Open Hat", 46, Some(-3.0), Some(1), None),
            v("ride", "Ride", 51, Some(-5.0), None, None),
            v("crash", "Crash", 49, Some(-4.0), None, None),
            v("tom-lo", "Low Tom", 41, None, None, None),
            v("tom-mid", "Mid Tom", 45, None, None, None),
            v("tom-hi", "High Tom", 48, None, None, None),
            v("conga-lo", "Low Conga", 64, Some(-2.0), None, None),
            v("conga-hi", "High Conga", 63, Some(-2.0), None, None),
            v("cabasa", "Cabasa", 69, Some(-5.0), None, None),
            v("tambourine", "Tambourine", 54, Some(-4.0), None, None),
            v("cowbell", "Cowbell", 56, Some(-3.0), None, None),
        ],
        pads: [
            "kick",
            "snare",
            "chh",
            "ohh",
            "clap",
            "tom-mid",
            "crash",
            "ride",
            "sidestick",
            "cabasa",
            "tambourine",
            "cowbell",
            "tom-lo",
            "tom-hi",
            "conga-lo",
            "conga-hi",
        ],
    },
    ClassicMachine {
        id: ClassicMachineId::Cr78,
        name: "CR-78-style",
        tagline: "Soft, organic preset-rhythm percussion",
        description: "Gentle, rounded voices from the preset rhythm-box era: soft resonant kick and snare, ticking hats, guiro, maracas, tambourine, bongos, congas, clave, cowbell and the metallic \"metal beat\".",
        color: "oklch(0.72 0.12 145)",
        voices: &[
            v("kick", "Kick", 36, None, None, None),
            v("snare", "Snare", 38, None, None, None),
            v("rim", "Rim", 37, Some(-2.0), None, None),
            v("chh", "Hi-Hat", 42, Some(-3.0), Some(1), None),
            v("ohh", "Open Hat", 46, Some(-3.0), Some(1), None),
            v("cymbal", "Cymbal", 49, Some(-4.0), None, None),
            v("metal", "Metal Beat", 80, Some(-4.0), None, None),
            v("maracas", "Maracas", 70, Some(-5.0), None, None),
            v("guiro", "Guiro", 73, Some(-4.0), None, None),
            v("tambourine", "Tambourine", 54, Some(-4.0), None, None),
            v("clave", "Clave", 75, Some(-4.0), None, None),
            v("cowbell", "Cowbell", 56, Some(-3.0), None, None),
            v("bongo-lo", "Low Bongo", 61, Some(-2.0), None, None),
            v("bongo-hi", "High Bongo", 60, Some(-2.0), None, None),
            v("conga-lo", "Low Conga", 64, Some(-2.0), None, None),
            v("conga-hi", "High Conga", 63, Some(-2.0), None, None),
        ],
        pads: [
            "kick",
            "snare",
            "chh",
            "ohh",
            "clave",
            "conga-lo",
            "cymbal",
            "cowbell",
            "rim",
            "maracas",
            "guiro",
            "tambourine",
            "metal",
            "bongo-lo",
            "bongo-hi",
            "conga-hi",
        ],
    },
];

/// `CLASSIC_MACHINE_IDS`.
pub fn classic_machine_ids() -> [ClassicMachineId; 6] {
    CLASSIC_MACHINES.map(|m| m.id)
}

/// `isClassicMachineId` on untyped input (the typed id when valid).
pub fn is_classic_machine_id(v: &Value) -> Option<ClassicMachineId> {
    v.as_str().and_then(ClassicMachineId::from_id)
}

/// `getClassicMachine` (total over the typed id).
pub fn get_classic_machine(id: ClassicMachineId) -> &'static ClassicMachine {
    CLASSIC_MACHINES.iter().find(|m| m.id == id).expect("every ClassicMachineId has a machine")
}

/// `getClassicVoice`.
pub fn get_classic_voice(machine: ClassicMachineId, voice: &str) -> Option<&'static ClassicVoiceInfo> {
    get_classic_machine(machine).voices.iter().find(|v| v.id == voice)
}

/// `isClassicVoice(machine, voice)` on untyped input.
pub fn is_classic_voice(machine: &Value, voice: &Value) -> bool {
    match (is_classic_machine_id(machine), voice.as_str()) {
        (Some(m), Some(v)) => get_classic_voice(m, v).is_some(),
        _ => false,
    }
}

/// `classicVoiceLabel`: display label ("Kick"), or the raw id when unknown.
pub fn classic_voice_label(machine: ClassicMachineId, voice: &str) -> String {
    get_classic_voice(machine, voice).map_or_else(|| voice.to_owned(), |v| v.label.to_owned())
}

/// `classicSampleName`: e.g. "808 Kick" (machine name without "-style" + voice label).
pub fn classic_sample_name(machine: ClassicMachineId, voice: &str) -> String {
    let name = get_classic_machine(machine).name;
    let short = name.strip_suffix("-style").unwrap_or(name);
    format!("{short} {}", classic_voice_label(machine, voice))
}

/// `classicKitId`: id of a machine's built-in kit.
pub fn classic_kit_id(machine: ClassicMachineId) -> String {
    format!("classic-{}", machine.id())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashSet;

    // classic.test.ts: 'has six "-style" machines with ≥ 12 unique voices'
    #[test]
    fn six_style_machines_with_at_least_12_unique_voices() {
        let ids: Vec<&str> = CLASSIC_MACHINES.iter().map(|m| m.id.id()).collect();
        assert_eq!(ids, ["tr808", "tr909", "tr707", "tr606", "linn", "cr78"]);
        for m in &CLASSIC_MACHINES {
            assert!(m.name.ends_with("-style"));
            assert!(m.voices.len() >= 12);
            assert_eq!(m.voices.iter().map(|v| v.id).collect::<HashSet<_>>().len(), m.voices.len());
            for v in m.voices {
                assert!(!v.label.is_empty());
                assert!((35..=87).contains(&v.gm));
            }
            assert!(m.pads.iter().all(|p| get_classic_voice(m.id, p).is_some()));
            assert_eq!(m.pads.iter().collect::<HashSet<_>>().len(), 16);
        }
        assert_eq!(CLASSIC_MACHINES.iter().map(|m| m.voices.len()).sum::<usize>(), 97);
    }

    // classic.test.ts: 'never claims a trademark in names or taglines'
    #[test]
    fn never_claims_a_trademark() {
        for m in &CLASSIC_MACHINES {
            let text = format!("{} {} {}", m.name, m.tagline, m.description).to_lowercase();
            for bad in ["roland", "linn electronics", "sequential", "oberheim"] {
                assert!(!text.contains(bad), "{}", m.name);
            }
        }
    }

    #[test]
    fn names_and_lookups() {
        assert_eq!(classic_sample_name(ClassicMachineId::Tr808, "kick"), "808 Kick");
        assert_eq!(classic_sample_name(ClassicMachineId::Linn, "sidestick"), "LinnDrum Sidestick");
        assert_eq!(classic_sample_name(ClassicMachineId::Tr909, "clap"), "909 Clap");
        assert_eq!(classic_sample_name(ClassicMachineId::Cr78, "nope"), "CR-78 nope");
        assert_eq!(classic_voice_label(ClassicMachineId::Tr808, "zzz"), "zzz");
        assert_eq!(classic_kit_id(ClassicMachineId::Tr606), "classic-tr606");
        assert!(is_classic_voice(&json!("tr808"), &json!("kick")));
        assert!(!is_classic_voice(&json!("tr999"), &json!("kick")));
        assert!(!is_classic_voice(&json!("tr808"), &json!("nope")));
        assert!(!is_classic_voice(&json!("tr808"), &json!(42)));
        let hat = get_classic_voice(ClassicMachineId::Tr909, "phh").unwrap();
        assert_eq!((hat.gain_db, hat.choke, hat.gm), (Some(-3.0), Some(1), 44));
        let boom = get_classic_voice(ClassicMachineId::Tr808, "kick-long").unwrap();
        assert_eq!(boom.mode, Some(PadPlayMode::Oneshot));
        assert_eq!(classic_machine_ids(), ClassicMachineId::ALL);
    }
}
