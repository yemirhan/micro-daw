//! Open-path tests: every golden migration case through the typed path, legacy (v1.0 / v1.3)
//! project files, a single-node mutation fuzz with an open → save → open self-check, and the
//! serde-level compatibility fixes (review H1, H2, M2).

use serde_json::{Value, json};

use super::*;
use crate::doc::ArrangementDoc;
use crate::migrate::{
    build_project_file, migrate_arrangement, project_file_to_json, remap_template_ids_value,
};
use crate::test_support::{JsonDiff, counter_ids, json_diffs, templates};
use crate::types::{EffectParams, FilterType, LfoWaveform};

fn migrate_fixture() -> Value {
    serde_json::from_str(include_str!("../../../parity/fixtures/migrate.json")).unwrap()
}

const LEGACY_V10: &str = include_str!("../testdata/legacy-v1.0.mdaw");
const LEGACY_V13: &str = include_str!("../testdata/legacy-v1.3.mdaw");

/// The typed open path for one golden case (`template` cases get loadFromTemplate's id remap
/// first, as in the TS).
fn open_case(c: &Value) -> OpenedArrangement {
    let mut ids = counter_ids();
    let mut input = c["input"].clone();
    if c["kind"] == "template" {
        remap_template_ids_value(&mut input, &mut ids).unwrap();
    }
    open_arrangement_value(input, c["packsDir"].as_str(), &mut ids).unwrap()
}

/// The ONLY differences allowed between the typed result and the TS JSON (review M2 / H2):
/// 1. numbers compare as JS numbers (`120` vs `120.0`): built into `json_diffs`;
/// 2. `isDrum: false` is written for notes that had no `isDrum` (the TS reads it as falsy);
/// 3. unknown keys on notes are dropped (`RegionNote` has no `extra`, see its docs);
/// 4. a field absent from the TS JSON is written with its default (module docs of `open`; the
///    TS reads `undefined` there: e.g. the v1.2/v1.3 filter's `lfo*`, a marker without `color`);
/// 5. anything at (or under) a path the coercion reported in a `CoerceWarning`; a list whose
///    length changed is allowed when an element of it was reported (dropped).
fn allowed(d: &JsonDiff, warnings: &[CoerceWarning]) -> Option<&'static str> {
    let in_note = |p: &str| {
        p.rsplit_once('.')
            .is_some_and(|(parent, _)| parent.ends_with(']') && parent.contains(".notes["))
    };
    if d.path.ends_with(".isDrum") && d.want.is_none() && d.got == Some(json!(false)) {
        return Some("isDrum:false added");
    }
    if in_note(&d.path) && d.got.is_none() {
        return Some("unknown note key dropped");
    }
    if d.want.is_none() && d.got.is_some() {
        return Some("absent field defaulted");
    }
    let warned = warnings.iter().any(|w| {
        d.path == w.path
            || d.path.starts_with(&format!("{}.", w.path))
            || d.path.starts_with(&format!("{}[", w.path))
            || w.path.starts_with(&format!("{}[", d.path))
    });
    warned.then_some("coerced (warned)")
}

#[test]
fn every_golden_migration_case_opens_through_the_typed_path() {
    let fx = migrate_fixture();
    let cases = fx["cases"].as_array().unwrap();
    let mut coerced_cases = Vec::new();
    for c in cases {
        let name = format!("{} {}", c["kind"], c["name"]);
        let opened = open_case(c);
        let msgs: Vec<String> = opened
            .dropped_drum_notes
            .iter()
            .map(DroppedDrumNotes::message)
            .collect();
        assert_eq!(json!(msgs), c["warnings"], "{name}");
        let got = serde_json::to_value(&opened.arrangement).unwrap();
        let bad: Vec<String> = json_diffs(&got, &c["output"])
            .into_iter()
            .filter(|d| allowed(d, &opened.warnings).is_none())
            .map(|d| d.to_string())
            .collect();
        assert!(
            bad.is_empty(),
            "{name}: undocumented differences:\n{}\nwarnings: {:?}",
            bad.join("\n"),
            opened.warnings
        );
        if !opened.warnings.is_empty() {
            coerced_cases.push(c["name"].as_str().unwrap().to_owned());
        }
        // Opening is a fixpoint of the (typed) migration the doc runs again on load_project.
        let (again, dropped) = migrate_arrangement(&opened.arrangement, None, &mut || {
            panic!("re-migration must not draw ids")
        });
        assert_eq!(again, opened.arrangement, "{name}: re-migration changed it");
        assert!(dropped.is_empty());
    }
    // The synthetic legacy cases really exercise the coercion (`loopMarkers.startBeat: 'x'` of
    // "two-drum-tracks-markers-groups" is replaced by the migration itself, before coercion).
    for expected in ["garbage-values", "string-length-and-quantize-1-8"] {
        assert!(
            coerced_cases.iter().any(|c| c == expected),
            "{expected} not coerced: {coerced_cases:?}"
        );
    }
    assert!(cases.len() >= 23);
}

#[test]
fn garbage_values_coerce_as_documented() {
    let fx = migrate_fixture();
    let c = fx["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "garbage-values")
        .unwrap();
    let a = open_case(c).arrangement;
    assert_eq!((a.name.as_str(), a.bpm, a.length_beats), ("7", 120.0, 32.0));
    let t = &a.tracks[0];
    assert_eq!((t.volume, t.pan, t.muted, t.solo), (0.0, 0.0, false, true));
    assert_eq!(t.color, "");
    assert_eq!(t.instrument.preset_index, 0);
    assert_eq!(t.instrument.extra["custom"], "kept");
    let r = &t.regions[0];
    assert_eq!(r.start_beat, 2.0);
    assert_eq!(r.extra["meta"], json!({ "kept": 1 }));
    let notes: Vec<(f64, f64, f64)> = r
        .notes
        .iter()
        .map(|n| (n.note, n.velocity, n.duration_beats))
        .collect();
    assert_eq!(notes, vec![(60.0, 0.8, 1.0), (62.0, 0.5, 0.25)]);
    let fx = t.effects.as_ref().unwrap();
    assert_eq!(fx.reverb.wet, 0.4);
    assert!(fx.chorus.enabled);
    assert_eq!(fx.chorus.depth, 0.5);
    assert_eq!(fx.delay.extra["sync"], "1/8");
    assert_eq!(fx.filter.kind, FilterType::Lowpass);
    assert_eq!(fx.filter.cutoff, 500.0);
    assert_eq!(fx.filter.lfo_wave, LfoWaveform::Sine);
    let lanes = t.automation.as_ref().unwrap();
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].points.len(), 1);
    assert!(lanes[0].visible);
    assert_eq!(a.tracks[2].instrument.kind, InstrumentType::Synth);
    let m = &a.markers.as_ref().unwrap()[0];
    assert_eq!((m.color.as_str(), m.beat), ("", 4.0));
    let g = &a.groups.as_ref().unwrap()[0];
    assert_eq!(g.track_ids, vec!["t1", "5"]);
    assert_eq!((g.color.as_str(), g.collapsed), ("", false));
    assert_eq!(g.extra["tag"], "x");
}

/// Review M2: unknown keys below Track level survive serde round trips (the undo snapshots, the
/// autosave and project files all go through serde).
#[test]
fn unknown_keys_survive_below_track_level() {
    let raw = json!({
        "id": "a", "name": "A", "bpm": 120, "lengthBeats": 16,
        "tracks": [{
            "id": "t", "name": "T", "volume": 0, "pan": 0, "muted": false, "solo": false, "color": "",
            "instrument": {
                "type": "drums", "presetIndex": 0, "i": 1,
                "kit": { "id": "k", "name": "K", "k": 2, "pads": [{ "gainDb": 0, "p": 3 }] },
            },
            "regions": [{
                "id": "r", "startBeat": 0, "lengthBeats": 4, "notes": [], "r": 4,
                "audio": { "sampleId": "s", "samplePath": "/s", "a": 5 },
            }],
            "effects": {
                "e": 6, "reverb": { "enabled": false, "wet": 0.2, "x": 7 },
                "filter": { "enabled": false, "cutoff": 100, "resonance": 1, "y": 8 },
            },
        }],
        "markers": [{ "id": "m", "name": "M", "beat": 0, "color": "", "m": 9 }],
        "groups": [{ "id": "g", "name": "G", "color": "", "trackIds": [], "g": 10 }],
    });
    let typed: Arrangement = serde_json::from_value(raw.clone()).unwrap();
    let back = serde_json::to_value(&typed).unwrap();
    for (path, want) in [
        ("/tracks/0/instrument/i", 1),
        ("/tracks/0/instrument/kit/k", 2),
        ("/tracks/0/instrument/kit/pads/0/p", 3),
        ("/tracks/0/regions/0/r", 4),
        ("/tracks/0/regions/0/audio/a", 5),
        ("/tracks/0/effects/e", 6),
        ("/tracks/0/effects/reverb/x", 7),
        ("/tracks/0/effects/filter/y", 8),
        ("/markers/0/m", 9),
        ("/groups/0/g", 10),
    ] {
        assert_eq!(back.pointer(path), Some(&json!(want)), "{path}");
    }
    // The coercion keeps them too.
    let mut w = Vec::new();
    let coerced = coerce_arrangement(&raw, &mut counter_ids(), &mut w);
    assert_eq!(
        serde_json::to_value(&coerced)
            .unwrap()
            .pointer("/tracks/0/effects/filter/y"),
        Some(&json!(8))
    );
}

/// Review H1: the v1.2 / v1.3 per-track `filter` (`{ enabled, cutoff, resonance }`) and partial
/// effect objects decode with the TS fallbacks, through plain serde too.
#[test]
fn old_effect_shapes_decode_with_ts_fallbacks() {
    let fx: crate::types::TrackEffectState = serde_json::from_value(json!({
        "reverb": { "enabled": true },
        "filter": { "enabled": true, "cutoff": 1200, "resonance": 6 },
    }))
    .unwrap();
    assert_eq!(fx.filter.kind, FilterType::Lowpass);
    assert_eq!(
        (fx.filter.lfo_rate, fx.filter.lfo_depth, fx.filter.lfo_wave),
        (1.0, 0.0, LfoWaveform::Sine)
    );
    assert_eq!((fx.filter.cutoff, fx.filter.resonance), (1200.0, 6.0));
    assert_eq!(fx.reverb.wet, 0.25);
    assert_eq!(fx.compressor, crate::types::CompressorFx::default());
}

#[test]
fn legacy_v1_0_project_opens() {
    let p = open_project(LEGACY_V10, None, &mut counter_ids()).unwrap();
    let a = &p.arrangement;
    assert_eq!(p.version_warning, None);
    assert_eq!(a.bpm, 96.0);
    let keys = &a.tracks[0];
    assert_eq!(keys.pan, 0.0);
    assert_eq!(keys.effects, Some(Default::default()));
    let vel: Vec<f64> = keys.regions[0].notes.iter().map(|n| n.velocity).collect();
    assert_eq!(vel, vec![100.0 / 127.0, 1.0, 64.0 / 127.0]);
    // GM drums → factory pads (35 and 36 at beat 2 merge, the louder wins; 81 has no pad).
    let drums = &a.tracks[1];
    assert!(drums.instrument.kit.is_some());
    assert_eq!(drums.regions[0].notes.len(), 6);
    assert_eq!(p.dropped_drum_notes.len(), 1);
    assert_eq!(
        a.loop_markers,
        Some(crate::types::LoopMarkers {
            start_beat: 0.0,
            end_beat: 8.0
        })
    );
    assert_eq!(
        a.beat.as_ref().unwrap().active_drum_track_id.as_deref(),
        Some("t-drums")
    );
    let audio = p.audio.unwrap();
    assert_eq!(
        (audio.master_volume_db, audio.reverb_wet),
        (Some(-6.0), Some(0.2))
    );
    assert!(p.warnings.is_empty(), "{:?}", p.warnings);
    // The whole file also decodes as a typed ProjectFile (old 4-field effectParams).
    let file: crate::types::ProjectFile = serde_json::from_str(LEGACY_V10).unwrap();
    assert_eq!(file.audio_config.effect_params.delay_time, 0.25);
}

#[test]
fn legacy_v1_3_project_opens() {
    let p = open_project(LEGACY_V13, None, &mut counter_ids()).unwrap();
    let a = &p.arrangement;
    assert!(p.warnings.is_empty(), "{:?}", p.warnings);
    let bass = &a.tracks[0];
    let f = &bass.effects.as_ref().unwrap().filter;
    assert!(f.enabled);
    assert_eq!(
        (f.kind, f.cutoff, f.resonance, f.lfo_rate, f.lfo_depth),
        (FilterType::Lowpass, 1200.0, 6.0, 1.0, 0.0)
    );
    assert_eq!(bass.effects.as_ref().unwrap().reverb.wet, 0.3);
    assert_eq!(bass.automation.as_ref().unwrap().len(), 2);
    assert_eq!(
        a.tracks[2].regions[0].audio.as_ref().unwrap().sample_id,
        "sample-7"
    );
    assert_eq!(a.markers.as_ref().unwrap().len(), 2);
    assert_eq!(p.sample_manifest.len(), 1);
    assert_eq!(
        a.loop_markers.map(|m| (m.start_beat, m.end_beat)),
        Some((0.0, 16.0))
    );
    let file: crate::types::ProjectFile = serde_json::from_str(LEGACY_V13).unwrap();
    assert_eq!(
        file.arrangement.tracks[0]
            .effects
            .as_ref()
            .unwrap()
            .filter
            .cutoff,
        1200.0
    );
}

/// Well-formed data: the coercion gives exactly what serde gives (plus the migration tail).
#[test]
fn coerce_matches_serde_on_well_formed_data() {
    let fx = migrate_fixture();
    let mut checked = 0;
    let mut inputs: Vec<Value> = templates()
        .into_iter()
        .map(|(_, t)| t["arrangement"].clone())
        .collect();
    inputs.extend(
        fx["cases"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["output"].clone()),
    );
    inputs.push(serde_json::from_str::<Value>(LEGACY_V13).unwrap()["arrangement"].clone());
    for v in inputs {
        let Ok(mut typed) = serde_json::from_value::<Arrangement>(v.clone()) else {
            continue;
        };
        crate::migrate::migrate_beat_and_loop_markers(&mut typed);
        let mut w = Vec::new();
        let coerced = coerce_arrangement(&v, &mut || panic!("no ids needed"), &mut w);
        assert_eq!(coerced, typed, "{}", v["id"]);
        assert!(w.is_empty(), "{}: {w:?}", v["id"]);
        checked += 1;
    }
    assert!(checked >= 20, "{checked}");
}

// --- fuzz: single-node mutations -------------------------------------------------------------------

/// A rich modern project: the v1.3 file opened and saved by this app, plus a drum kit with a
/// sample, synth overrides, a group / marker set and the beat state.
fn base_project() -> Value {
    let p = open_project(LEGACY_V13, None, &mut counter_ids()).unwrap();
    let mut a = p.arrangement;
    let kit = a.tracks[1].instrument.kit.as_mut().unwrap();
    kit.pads.truncate(3);
    kit.pads[0].sample = Some(crate::types::PadSample {
        name: "Kick".into(),
        source: crate::types::PadSampleSource::File {
            path: "/k.wav".into(),
            sample_id: None,
        },
    });
    a.tracks[0].instrument.synth_overrides = Some(
        json!({ "env": { "attack": 0.1 } })
            .as_object()
            .cloned()
            .unwrap(),
    );
    let file = build_project_file(a, -3.0, EffectParams::default(), "c", "m", &|_| None);
    serde_json::from_str(&project_file_to_json(&file)).unwrap()
}

/// Every JSON pointer in `v` (depth-first).
fn pointers(v: &Value, at: String, out: &mut Vec<String>) {
    out.push(at.clone());
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                pointers(x, format!("{at}/{k}"), out);
            }
        }
        Value::Array(a) => {
            for (i, x) in a.iter().enumerate() {
                pointers(x, format!("{at}/{i}"), out);
            }
        }
        _ => {}
    }
}

enum Mutation {
    Set(Value),
    Remove,
}

fn mutate(root: &Value, ptr: &str, m: &Mutation) -> Value {
    let mut v = root.clone();
    match m {
        Mutation::Set(x) => {
            if ptr.is_empty() {
                return x.clone();
            }
            *v.pointer_mut(ptr).unwrap() = x.clone();
        }
        Mutation::Remove => {
            let (parent, key) = ptr.rsplit_once('/').unwrap();
            match v.pointer_mut(parent).unwrap() {
                Value::Object(o) => {
                    o.shift_remove(key);
                }
                Value::Array(a) => {
                    a.remove(key.parse::<usize>().unwrap());
                }
                _ => unreachable!(),
            }
        }
    }
    v
}

/// open → save → open gives the same arrangement.
fn assert_self_check(opened: &OpenedProject, ctx: &str) {
    let file = build_project_file(
        opened.arrangement.clone(),
        0.0,
        EffectParams::default(),
        "c",
        "m",
        &|_| None,
    );
    let saved = project_file_to_json(&file);
    let again = open_project(&saved, None, &mut || panic!("{ctx}: reopen drew an id"))
        .unwrap_or_else(|e| panic!("{ctx}: saved project does not reopen: {e}\n{saved}"));
    assert_eq!(
        again.arrangement, opened.arrangement,
        "{ctx}: open → save → open changed the arrangement"
    );
    assert!(
        again.warnings.is_empty(),
        "{ctx}: a saved project needs coercion: {:?}",
        again.warnings
    );
}

#[test]
fn single_node_mutations_never_panic_and_reopen_equal() {
    let base = base_project();
    let mut ptrs = Vec::new();
    pointers(&base, String::new(), &mut ptrs);
    let mutations = [
        Mutation::Set(Value::Null), // also what JSON.stringify writes for NaN / ±Infinity
        Mutation::Set(json!("x")),
        Mutation::Set(json!("12")),
        Mutation::Set(json!(true)),
        Mutation::Set(json!(-1)),
        Mutation::Set(json!(2.5)),
        Mutation::Set(json!({})),
        Mutation::Set(json!([])),
        Mutation::Remove,
    ];
    let (mut opened, mut failed) = (0, 0);
    for ptr in &ptrs {
        for m in &mutations {
            if ptr.is_empty() && matches!(m, Mutation::Remove) {
                continue;
            }
            let raw = mutate(&base, ptr, m).to_string();
            let ctx = format!(
                "{ptr} := {}",
                match m {
                    Mutation::Set(x) => x.to_string(),
                    Mutation::Remove => "<removed>".into(),
                }
            );
            match open_project(&raw, None, &mut counter_ids()) {
                Ok(p) => {
                    opened += 1;
                    assert_self_check(&p, &ctx);
                }
                Err(_) => failed += 1,
            }
        }
    }
    // Errors are only the shapes the TS migration throws on (a handful of structural nodes).
    assert!(opened > 10 * failed, "opened {opened}, failed {failed}");
    assert!(opened > 2000, "{opened}");
}

/// The doc writes NaN / ±Infinity unchecked (like the TS); JSON stores them as null; the next
/// open must work and be stable.
#[test]
fn non_finite_values_written_by_the_doc_reopen() {
    let mut doc = ArrangementDoc::with_id_generator(Box::new(counter_ids()));
    doc.reset_to_starter();
    let id = doc.get_tracks()[0].id.clone();
    doc.set_bpm(f64::INFINITY);
    doc.set_track_volume(&id, f64::NAN);
    doc.set_track_pan(&id, f64::NEG_INFINITY);
    let stored = doc.save_to_local_storage();
    assert!(stored.contains("\"bpm\":null"), "{stored}");

    // Autosave path.
    let mut other = ArrangementDoc::with_id_generator(Box::new(counter_ids()));
    let warnings = other
        .load_from_local_storage(Some(&stored))
        .unwrap()
        .unwrap();
    assert_eq!(other.get_bpm(), 120.0);
    assert_eq!(
        (other.get_tracks()[0].volume, other.get_tracks()[0].pan),
        (0.0, 0.0)
    );
    let paths: Vec<&str> = warnings.iter().map(|w| w.path.as_str()).collect();
    assert_eq!(
        paths,
        ["$.tracks[0].volume", "$.tracks[0].volume", "$.bpm", "$.bpm"],
        "null + default each"
    );

    // Project file path.
    let file = build_project_file(
        doc.get_arrangement().clone(),
        f64::NAN,
        EffectParams::default(),
        "c",
        "m",
        &|_| None,
    );
    let p = open_project(&project_file_to_json(&file), None, &mut counter_ids()).unwrap();
    assert_eq!(p.arrangement.bpm, 120.0);
    assert_eq!(p.audio.unwrap().master_volume_db, None);
    assert_self_check(&p, "doc NaN / Infinity");
}

/// Every golden doc-session arrangement JSON (TS) opens and is stable.
#[test]
fn golden_doc_arrangements_open_and_reopen_equal() {
    let fx: Value =
        serde_json::from_str(include_str!("../../../parity/fixtures/doc.json")).unwrap();
    let mut n = 0;
    for session in fx["sessions"].as_array().unwrap() {
        let mut cur = Value::Null;
        for step in session["steps"].as_array().unwrap() {
            for patch in step["arr"].as_array().unwrap() {
                crate::test_support::apply_patch(&mut cur, patch.as_array().unwrap());
            }
            let file = json!({ "version": "1.0.0", "arrangement": cur, "audioConfig": { "masterVolume": 0, "effectParams": {} } });
            let p = open_project(&file.to_string(), None, &mut counter_ids()).unwrap();
            assert!(p.warnings.is_empty(), "{:?}", p.warnings);
            assert_self_check(&p, &format!("{} step {n}", session["name"]));
            n += 1;
        }
    }
    assert!(n > 2400, "{n}");
}
