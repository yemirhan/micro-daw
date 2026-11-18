//! Port of `src/audio/presets.test.ts` (bank integrity, construct/release of every preset,
//! `resolvePreset` / override merging).

use serde_json::json;

use super::SynthEngine;
use super::bank::{fallback_piano, synth_presets};
use super::params::{get_path, merge_overrides, merge_preset_params, path_to_object};
use super::presets::*;
use crate::contract::Instrument;
use crate::core::SynthInstrument;

fn release_tail(i: usize) -> f64 {
    synth_presets()[i].release_tail()
}

#[test]
fn has_30_to_45_presets_with_unique_ids_and_names() {
    let bank = synth_presets();
    assert!(bank.len() >= 30 && bank.len() <= 45);
    assert_eq!(bank.len(), 41);
    let ids: std::collections::HashSet<_> = bank.iter().map(|p| p.id()).collect();
    let names: std::collections::HashSet<_> = bank.iter().map(|p| p.meta().name.as_str()).collect();
    assert_eq!(ids.len(), bank.len());
    assert_eq!(names.len(), bank.len());
}

#[test]
fn keeps_every_legacy_index_loadable() {
    assert!(synth_presets().len() > LEGACY_PRESET_COUNT);
}

#[test]
fn legacy_slots_keep_their_musical_role() {
    let b = synth_presets();
    assert_eq!(b[0].meta().category, PresetCategory::Keys);
    assert_eq!(b[0].engine(), "sampler");
    assert_eq!(b[7].meta().category, PresetCategory::Bass);
    assert_eq!(b[8].meta().category, PresetCategory::Bass);
    assert_eq!(b[9].id(), "drawbar-organ");
    assert_eq!(b[12].meta().category, PresetCategory::Pad);
    assert_eq!(b[13].id(), "rhodes");
    assert_eq!(b[14].meta().category, PresetCategory::Brass);
    assert_eq!(b[20].id(), "acid-bass");
    assert_eq!(b[28].id(), "nylon-guitar");
}

#[test]
fn legacy_polyphonic_slots_stay_polyphonic() {
    for i in (0..24).chain([33, 34, 35]) {
        let p = &synth_presets()[i];
        let mono = matches!(p, SynthPreset::Subtractive(s) if s.mono);
        assert!(!mono, "{i} {}", p.id());
        assert!(p.meta().polyphony > 1.0, "{i} {}", p.id());
    }
}

#[test]
fn every_template_preset_index_resolves() {
    let dir = format!("{}/../../../src/data/templates", env!("CARGO_MANIFEST_DIR"));
    let mut seen = 0;
    for entry in std::fs::read_dir(&dir).expect("templates") {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let t: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        for track in t["arrangement"]["tracks"].as_array().unwrap() {
            if track["instrument"]["type"] == "synth" {
                let i = track["instrument"]["presetIndex"].as_u64().unwrap() as usize;
                assert!(i < synth_presets().len(), "{path:?}: {i}");
                seen += 1;
            }
        }
    }
    assert!(seen > 0);
}

#[test]
fn covers_the_required_families() {
    for id in [
        "grand-piano",
        "bright-piano",
        "rhodes",
        "wurli",
        "drawbar-organ",
        "warm-pad",
        "supersaw-pad",
        "airy-pad",
        "string-ensemble",
        "synth-brass",
        "sub-bass",
        "reese-bass",
        "acid-bass",
        "pluck-bass",
        "saw-lead",
        "square-lead",
        "sync-lead",
        "synth-pluck",
        "tubular-bell",
        "marimba",
        "vibraphone",
        "laser-zap",
    ] {
        assert!(preset_by_id(id).is_some(), "{id}");
    }
}

#[test]
fn uses_only_declared_categories_and_vibes() {
    for p in synth_presets() {
        assert!(
            PRESET_CATEGORIES.iter().any(|c| c.0 == p.meta().category),
            "{}",
            p.id()
        );
        assert!(
            PRESET_VIBES.iter().any(|v| v.0 == p.meta().vibe),
            "{}",
            p.id()
        );
    }
}

#[test]
fn has_sane_parameter_ranges() {
    let fallback = SynthPreset::Fm(fallback_piano().clone());
    for p in synth_presets().iter().chain([&fallback]) {
        let m = p.meta();
        assert!(m.gain >= -15.0 && m.gain <= 12.0, "{}", p.id());
        assert!(m.polyphony >= 1.0, "{}", p.id());
        match p {
            SynthPreset::Subtractive(s) => {
                for (a, su, r) in [
                    (s.amp.attack, s.amp.sustain, s.amp.release),
                    (
                        s.filter_env.attack,
                        s.filter_env.sustain,
                        s.filter_env.release,
                    ),
                ] {
                    assert!(
                        a > 0.0 && (0.0..=1.0).contains(&su) && r > 0.0,
                        "{}",
                        p.id()
                    );
                }
                assert!((1.0..=7.0).contains(&s.osc_a.unison));
                assert!(s.filter.cutoff > 20.0 && s.filter.cutoff <= 18000.0);
            }
            SynthPreset::Fm(f) => {
                assert!(f.ratio > 0.0 && f.index >= 0.0 && f.amp.release > 0.0);
            }
            _ => {}
        }
    }
}

#[test]
fn looks_presets_up_by_id() {
    assert_eq!(preset_index_by_id("grand-piano"), 0);
    assert_eq!(preset_index_by_id("glass-pad"), 40);
    assert_eq!(preset_index_by_id("nope"), -1);
    assert_eq!(fallback_piano().meta.gain, -11.0);
}

/// Renders `secs` seconds (adding into fresh buffers); returns the peak.
fn render(inst: &mut SynthEngine, frame: &mut u64, secs: f64) -> f32 {
    let mut peak = 0f32;
    let end = *frame + (secs * 48000.0) as u64;
    let (mut l, mut r) = ([0f32; 128], [0f32; 128]);
    while *frame < end {
        l.fill(0.0);
        r.fill(0.0);
        inst.process(*frame, &mut l, &mut r);
        for v in l.iter().chain(&r) {
            assert!(v.is_finite());
            peak = peak.max(v.abs());
        }
        *frame += 128;
    }
    peak
}

#[test]
fn every_preset_constructs_and_releases_cleanly() {
    for (i, preset) in synth_presets().iter().enumerate() {
        let mut p = SynthEngine::new(preset, 48000.0, 0.0, 7);
        let mut frame = 0u64;
        p.note_on_midi(60, 0.9, 0.0);
        p.note_on_midi(64, 0.5, 0.01);
        p.note_on_midi(67, 1.0, 0.02);
        p.note_on_midi(60, 0.7, 0.03); // re-strike while held
        assert!(p.active_voice_count(0.05) > 0, "{i}");
        assert!(
            render(&mut p, &mut frame, 0.5) > 0.0,
            "{i} {} is silent",
            preset.id()
        );
        p.note_off_midi(60, 0.5);
        p.note_off_midi(64, 0.5);
        p.note_off_midi(67, 0.5);
        assert!(p.held_notes().is_empty(), "{i}");
        assert_eq!(p.active_voice_count(0.5 + release_tail(i) + 0.1), 0, "{i}");
        // Scheduled one-shots end on their own.
        render(&mut p, &mut frame, 0.5);
        p.trigger_attack_release(48, 0.25, 1.0, 0.8);
        assert_eq!(
            p.active_voice_count(1.0 + 0.25 + release_tail(i) + 0.1),
            0,
            "{i}"
        );
        // Live param change.
        let next = merge_preset_params(
            preset,
            Some(&json!({ "gain": preset.meta().gain - 1.0, "character": { "chorus": 0.2 } })),
        );
        // (a sampler preset's engine here is its FM fallback alone: `SynthPlayer` sets those)
        assert!(p.set_preset(&next) || matches!(preset, SynthPreset::Sampler(_)));
        render(&mut p, &mut frame, 1.0);
        p.note_on_midi(48, 0.8, 2.0);
        render(&mut p, &mut frame, 0.1);
        p.release_all(2.1);
        assert!(p.held_notes().is_empty(), "{i}");
        assert_eq!(p.active_voice_count(2.2), 0, "{i}");
        // Silent once the release-all fade and the longest tail are over.
        render(&mut p, &mut frame, 0.1);
        let tail = if matches!(preset, SynthPreset::Pluck(_)) {
            0.05
        } else {
            0.0
        };
        render(&mut p, &mut frame, 0.2 + tail);
        let peak = render(&mut p, &mut frame, 0.2);
        assert!(peak < 1e-4, "{i} {}: {peak} after releaseAll", preset.id());
    }
}

#[test]
fn resolve_preset_returns_the_base_without_overrides() {
    assert_eq!(&resolve_preset(1.0, None), &synth_presets()[1]);
    assert_eq!(&resolve_preset(1.0, Some(&json!({}))), &synth_presets()[1]);
}

#[test]
fn falls_back_to_preset_0_for_bad_indices() {
    assert_eq!(preset_at(999.0), &synth_presets()[0]);
    assert_eq!(preset_at(-1.0), &synth_presets()[0]);
    assert_eq!(preset_at(f64::NAN), &synth_presets()[0]);
    assert_eq!(&resolve_preset(1.7, None), &synth_presets()[1]);
}

fn lead() -> &'static SubtractivePreset {
    match &synth_presets()[1] {
        SynthPreset::Subtractive(p) => p,
        _ => unreachable!(),
    }
}

#[test]
fn deep_merges_nested_overrides_without_mutating_the_bank() {
    let before = synth_presets()[1].clone();
    let SynthPreset::Subtractive(p) = resolve_preset(
        1.0,
        Some(&json!({ "filter": { "cutoff": 900 }, "amp": { "release": 1.5 } })),
    ) else {
        panic!()
    };
    assert_eq!(p.filter.cutoff, 900.0);
    assert_eq!(p.filter.resonance, lead().filter.resonance);
    assert_eq!(p.amp.release, 1.5);
    assert_eq!(p.amp.attack, lead().amp.attack);
    assert_eq!(synth_presets()[1], before);
}

#[test]
fn ignores_unknown_keys_wrong_types_bad_enums_and_locked_fields() {
    let merged = resolve_preset(
        1.0,
        Some(&json!({
            "bogus": 1,
            "filter": { "cutoff": "loud", "type": "wobbly", "resonance": null, "rolloff": -48, "nope": 3 },
            "oscA": { "wave": "square", "level": "1" },
            "id": "hacked", "engine": "fm", "name": "X",
        })),
    );
    let v = merged.to_json();
    assert!(v.get("bogus").is_none());
    assert!(v["filter"].get("nope").is_none());
    let SynthPreset::Subtractive(p) = merged else {
        panic!()
    };
    assert_eq!(p.filter.cutoff, lead().filter.cutoff);
    assert_eq!(p.filter.kind, lead().filter.kind);
    assert_eq!(p.filter.resonance, lead().filter.resonance);
    assert_eq!(p.filter.rolloff, lead().filter.rolloff);
    assert_eq!(p.osc_a.wave, OscWave::Square);
    assert_eq!(p.osc_a.level, lead().osc_a.level);
    assert_eq!(p.meta.id, "saw-lead");
    assert_eq!(p.meta.name, "Saw Lead");
}

#[test]
fn translates_overrides_saved_by_the_previous_engine() {
    let SynthPreset::Subtractive(sub) = resolve_preset(
        1.0,
        Some(&json!({
            "envelope": { "attack": 0.2, "release": 2 },
            "filterEnvelope": { "attack": 0.3, "baseFrequency": 500, "octaves": 3 },
            "oscillator": { "type": "fatsawtooth" },
        })),
    ) else {
        panic!()
    };
    assert_eq!(sub.amp.attack, 0.2);
    assert_eq!(sub.amp.release, 2.0);
    assert_eq!(sub.filter_env.attack, 0.3);
    assert_eq!(sub.filter.cutoff, 500.0);
    assert_eq!(sub.filter.env_amount, 3.0);

    let fm_index = preset_index_by_id("rhodes") as f64;
    let SynthPreset::Fm(fm) = resolve_preset(
        fm_index,
        Some(&json!({ "modulationIndex": 5, "harmonicity": 2, "envelope": { "decay": 1 } })),
    ) else {
        panic!()
    };
    assert_eq!(fm.index, 5.0);
    assert_eq!(fm.ratio, 2.0);
    assert_eq!(fm.amp.decay, 1.0);

    let pluck_index = preset_index_by_id("nylon-guitar") as f64;
    let SynthPreset::Pluck(pl) = resolve_preset(
        pluck_index,
        Some(&json!({ "dampening": 1500, "resonance": 0.9, "attackNoise": 2 })),
    ) else {
        panic!()
    };
    assert_eq!(
        (pl.dampening, pl.resonance, pl.attack_noise),
        (1500.0, 0.9, 2.0)
    );
}

#[test]
fn every_preset_survives_a_garbage_override_object() {
    let garbage = json!({ "amp": null, "filter": 5, "oscA": [], "character": { "drive": "x" }, "release": {}, "ratio": "y" });
    for (i, p) in synth_presets().iter().enumerate() {
        let merged = resolve_preset(i as f64, Some(&garbage));
        assert_eq!(&merged, p, "{i}");
        let _ = SynthEngine::new(&merged, 48000.0, 0.0, 1);
    }
}

#[test]
fn merge_accepts_partials_arrays_only_with_numbers() {
    let organ = &synth_presets()[9];
    let SynthPreset::Subtractive(ok) =
        merge_preset_params(organ, Some(&json!({ "oscA": { "partials": [1, 0.5] } })))
    else {
        panic!()
    };
    assert_eq!(ok.osc_a.partials, Some(vec![1.0, 0.5]));
    let SynthPreset::Subtractive(bad) =
        merge_preset_params(organ, Some(&json!({ "oscA": { "partials": [1, "x"] } })))
    else {
        panic!()
    };
    let SynthPreset::Subtractive(o) = organ else {
        panic!()
    };
    assert_eq!(bad.osc_a.partials, o.osc_a.partials);
}

#[test]
fn override_object_helpers() {
    assert_eq!(
        path_to_object("filter.cutoff", json!(800)),
        json!({ "filter": { "cutoff": 800 } })
    );
    assert_eq!(path_to_object("gain", json!(1)), json!({ "gain": 1 }));
    let a = json!({ "filter": { "cutoff": 800, "resonance": 2 }, "gain": 1 });
    let b = json!({ "filter": { "cutoff": 900 }, "amp": { "release": 1 } });
    let m = merge_overrides(a.as_object(), b.as_object().unwrap());
    assert_eq!(
        serde_json::Value::Object(m),
        json!({ "filter": { "cutoff": 900, "resonance": 2 }, "gain": 1, "amp": { "release": 1 } })
    );
    let preset = synth_presets()[1].to_json();
    assert_eq!(get_path(&preset, "filter.cutoff"), Some(&json!(2400.0)));
    assert_eq!(get_path(&preset, "filter.nope"), None);
}

#[test]
fn presets_round_trip_through_their_json_form() {
    for p in synth_presets()
        .iter()
        .chain([&SynthPreset::Fm(fallback_piano().clone())])
    {
        let v = p.to_json();
        assert_eq!(SynthPreset::from_json(&v).as_ref(), Some(p));
        assert_eq!(v["engine"], p.engine());
    }
    // Optional keys exist only where the TS sets them.
    let v = synth_presets()[1].to_json();
    assert!(v["oscA"].get("partials").is_none() && v["oscA"].get("width").is_none());
    assert_eq!(synth_presets()[12].to_json()["oscB"]["width"], json!(0.3));
}
