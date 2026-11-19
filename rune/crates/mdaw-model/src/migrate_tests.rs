//! Migration / template / project-file tests (unit + golden parity against
//! rune/parity/fixtures/migrate.json from rune/parity/ts/migrate.golden.ts).

use super::*;
use crate::test_support::{assert_json_eq, counter_ids, templates};
use crate::types::{AudioRegionData, InstrumentType, Region, Track, TrackInstrument};

fn no_id() -> String {
    panic!("unexpected id")
}

// --- golden ---

#[test]
fn golden_migration_matches_ts() {
    let fx: Value =
        serde_json::from_str(include_str!("../../../parity/fixtures/migrate.json")).unwrap();
    let cases = fx["cases"].as_array().unwrap();
    let mut templates_seen = 0;
    for c in cases {
        let name = format!("{} {}", c["kind"], c["name"]);
        let packs_dir = c["packsDir"].as_str();
        let mut ids = counter_ids();
        let (out, warnings) = if c["kind"] == "template" {
            load_from_template_value(&c["input"], packs_dir, &mut ids).unwrap()
        } else {
            let mut v = c["input"].clone();
            let w = migrate_arrangement_value(&mut v, packs_dir, &mut ids).unwrap();
            (v, w)
        };
        assert_json_eq(&out, &c["output"], &name);
        let msgs: Vec<String> = warnings.iter().map(DroppedDrumNotes::message).collect();
        assert_eq!(json!(msgs), c["warnings"], "{name}");
        if templates().iter().any(|(id, _)| c["name"] == *id) {
            templates_seen += 1;
            // The typed path gives the same result for real data.
            let typed: Arrangement = serde_json::from_value(c["input"].clone()).unwrap();
            let mut ids = counter_ids();
            let (arr, _) = if c["kind"] == "template" {
                load_from_template(&typed, packs_dir, &mut ids).unwrap()
            } else {
                migrate_arrangement(&typed, packs_dir, &mut ids)
            };
            let back: Arrangement = serde_json::from_value(c["output"].clone()).unwrap();
            assert_eq!(arr, back, "{name} (typed)");
        }
    }
    assert_eq!(templates_seen, 12, "6 templates x (template, project)");
    assert!(cases.len() >= 20);
}

#[test]
fn golden_inputs_are_the_shipped_templates() {
    let fx: Value =
        serde_json::from_str(include_str!("../../../parity/fixtures/migrate.json")).unwrap();
    for (id, t) in templates() {
        let case = fx["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == id && c["kind"] == "project")
            .unwrap();
        assert_json_eq(&case["input"], &t["arrangement"], id);
    }
}

// --- unit: migration edge cases ---

#[test]
fn rejects_shapes_the_ts_throws_on() {
    let mut ids = counter_ids();
    for bad in [
        json!(null),
        json!(5),
        json!({}),
        json!({ "tracks": "abc" }),
        json!({ "tracks": [null] }),
        json!({ "tracks": [{ "instrument": { "type": "synth" } }] }),
        json!({ "tracks": [{ "regions": [] }] }),
        json!({ "tracks": [{ "instrument": { "type": "synth" }, "regions": [{ "notes": [null] }] }] }),
        json!({ "tracks": [{ "instrument": { "type": "synth" }, "regions": [{}] }] }),
    ] {
        assert!(
            migrate_arrangement_value(&mut bad.clone(), None, &mut ids).is_err(),
            "{bad}"
        );
    }
    let mut ok = json!({ "tracks": [{ "instrument": 5, "regions": [{ "notes": [7, "x", { "velocity": "254" }] }] }] });
    migrate_arrangement_value(&mut ok, None, &mut ids).unwrap();
    assert_eq!(
        ok["tracks"][0]["regions"][0]["notes"][2]["velocity"],
        json!(1.0)
    );
    assert_eq!(ok["lengthBeats"], json!(8.0));
    assert_eq!(ok["beat"]["activeDrumTrackId"], Value::Null);
}

#[test]
fn template_remap_errors_and_group_filtering() {
    let mut ids = counter_ids();
    assert!(
        remap_template_ids_value(
            &mut json!({ "tracks": [], "groups": [{ "trackIds": "t1" }] }),
            &mut ids
        )
        .is_err()
    );
    assert!(
        remap_template_ids_value(&mut json!({ "tracks": [], "markers": {} }), &mut ids).is_err()
    );
    let mut v = json!({
        "id": "a", "tracks": [{ "id": 1, "regions": [] }, { "id": 1.0, "regions": [{ "id": "r" }] }],
        "beat": { "activeDrumTrackId": "" }, "groups": [{ "trackIds": [1, 2] }], "markers": 0,
    });
    let mut ids = counter_ids();
    remap_template_ids_value(&mut v, &mut ids).unwrap();
    // Map.set with an equal key (1 === 1.0) keeps the later id; "" is falsy: left alone.
    assert_eq!(v["groups"][0]["trackIds"], json!(["uuid-3"]));
    assert_eq!(v["beat"]["activeDrumTrackId"], json!(""));
    assert_eq!(v["tracks"][1]["regions"][0]["id"], json!("uuid-4"));
    assert_eq!(v["groups"][0]["id"], json!("uuid-5"));
}

#[test]
fn js_number_coercion() {
    let n = |v: Value| js_num(Some(&v));
    assert_eq!(n(json!(" 12 ")), 12.0);
    assert_eq!(n(json!("0x10")), 16.0);
    assert_eq!(n(json!("")), 0.0);
    assert_eq!(n(json!(null)), 0.0);
    assert_eq!(n(json!(true)), 1.0);
    assert_eq!(n(json!([])), 0.0);
    assert_eq!(n(json!(["7"])), 7.0);
    assert!(n(json!("inf")).is_nan());
    assert!(n(json!("1,5")).is_nan());
    assert!(n(json!({})).is_nan());
    assert!(js_num(None).is_nan());
    assert_eq!(n(json!("-Infinity")), f64::NEG_INFINITY);
}

// --- project file (ProjectManager pure parts) ---

fn audio_track(regions: Vec<(Option<&str>, &str)>) -> Track {
    Track {
        id: "t".into(),
        name: "Audio".into(),
        instrument: TrackInstrument {
            kind: InstrumentType::Audio,
            preset_index: 0,
            synth_overrides: None,
            kit: None,
            extra: Default::default(),
        },
        regions: regions
            .into_iter()
            .enumerate()
            .map(|(i, (name, sample))| Region {
                id: format!("r{i}"),
                start_beat: 0.0,
                length_beats: 4.0,
                notes: vec![],
                color: None,
                name: name.map(str::to_owned),
                audio: Some(AudioRegionData {
                    sample_id: sample.into(),
                    sample_path: format!("/s/{sample}.wav"),
                    offset_seconds: 0.0,
                    gain_db: 0.0,
                    original_duration: 1.0,
                    extra: Default::default(),
                }),
                extra: Default::default(),
            })
            .collect(),
        volume: 0.0,
        pan: 0.0,
        muted: false,
        solo: false,
        color: "#fff".into(),
        effects: None,
        automation: None,
        extra: Default::default(),
    }
}

fn arrangement(tracks: Vec<Track>) -> Arrangement {
    serde_json::from_value(
        json!({ "id": "p", "name": "P", "bpm": 120, "lengthBeats": 16, "tracks": tracks }),
    )
    .unwrap()
}

#[test]
fn builds_the_sample_manifest_and_project_file() {
    let arr = arrangement(vec![audio_track(vec![
        (Some("Vox"), "a"),
        (None, "b"),
        (Some("dup"), "a"),
        (Some(""), "c"),
    ])]);
    let names = |id: &str| (id == "b").then(|| "Loaded B".to_owned());
    let manifest = build_sample_manifest(&arr, &names);
    let got: Vec<(&str, &str, &str)> = manifest
        .iter()
        .map(|e| (e.sample_id.as_str(), e.path.as_str(), e.name.as_str()))
        .collect();
    assert_eq!(
        got,
        [
            ("a", "/s/a.wav", "Vox"),
            ("b", "/s/b.wav", "Loaded B"),
            ("c", "/s/c.wav", "")
        ]
    );

    let file = build_project_file(
        arr.clone(),
        -12.0,
        Default::default(),
        "2026-01-01T00:00:00.000Z",
        "now",
        &names,
    );
    assert_eq!(file.version, "1.0.0");
    assert_eq!(file.sample_manifest.as_ref().map(Vec::len), Some(3));
    let text = project_file_to_json(&file);
    assert!(text.starts_with("{\n  \"version\": \"1.0.0\",\n  \"arrangement\": {"));
    let back: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        back["audioConfig"]["effectParams"]["reverbWet"],
        json!(0.12)
    );
    assert_eq!(back["createdAt"], "2026-01-01T00:00:00.000Z");

    let empty = build_project_file(
        arrangement(vec![]),
        0.0,
        Default::default(),
        "c",
        "m",
        &|_| None,
    );
    assert!(empty.sample_manifest.is_none());
    assert!(!project_file_to_json(&empty).contains("sampleManifest"));
}

/// ProjectManager.test.ts 'opens through loadProject (new session) every time, even for the same
/// project' (pure half: the file parses and yields the arrangement to install, every time).
#[test]
fn loads_the_same_project_twice() {
    let raw = r#"{"version":"1.0.0","arrangement":{"id":"same-project","tracks":[]}}"#;
    for _ in 0..2 {
        let p = load_project_json(raw, None, &mut no_id).unwrap();
        assert_eq!(p.arrangement["id"], "same-project");
        assert_eq!(p.version_warning, None);
        assert!(p.sample_manifest.is_empty() && p.kits_to_preload.is_empty() && p.audio.is_none());
        assert_eq!((p.created_at, p.modified_at), (None, None));
    }
}

#[test]
fn load_project_json_mirrors_load_from_file() {
    let raw = json!({
        "version": "0.9",
        "arrangement": {
            "id": "p", "name": "P", "bpm": 120, "lengthBeats": 16,
            "tracks": [
                { "id": "d", "name": "D", "instrument": { "type": "drums", "presetIndex": 0, "kit": { "name": "K", "pads": [] } },
                  "regions": [], "volume": 0, "muted": false, "solo": false, "color": "#fff" },
            ],
        },
        "audioConfig": { "masterVolume": -9, "effectParams": { "reverbWet": 0.3, "chorusDepth": "x", "filterCutoff": 900, "filterResonance": 2 } },
        "createdAt": "", "modifiedAt": "2026-02-02",
        "sampleManifest": [{ "sampleId": "a", "path": "/a.wav", "name": "A" }, { "sampleId": 3 }],
    })
    .to_string();
    let p = load_project_json(&raw, None, &mut counter_ids()).unwrap();
    assert_eq!(
        p.version_warning.as_deref(),
        Some("Unknown project version: 0.9")
    );
    assert_eq!(p.sample_manifest.len(), 2);
    assert_eq!(
        p.sample_manifest[1],
        ManifestLoad {
            sample_id: None,
            path: None,
            name: None
        }
    );
    // preload draws uuid-1, the migration's normalizeKit draws uuid-2.
    assert_eq!(p.kits_to_preload[0].id, "uuid-1");
    assert_eq!(
        p.arrangement["tracks"][0]["instrument"]["kit"]["id"],
        "uuid-2"
    );
    let a = p.audio.unwrap();
    assert_eq!(
        (
            a.master_volume_db,
            a.reverb_wet,
            a.chorus_depth,
            a.filter_cutoff
        ),
        (Some(-9.0), Some(0.3), None, Some(900.0))
    );
    assert_eq!(p.created_at, None); // "" is falsy: the caller uses now
    assert_eq!(p.modified_at.as_deref(), Some("2026-02-02"));

    assert!(matches!(
        load_project_json("{", None, &mut no_id),
        Err(ProjectLoadError::Json(_))
    ));
    assert!(matches!(
        load_project_json("null", None, &mut no_id),
        Err(ProjectLoadError::Shape(_))
    ));
    assert!(matches!(
        load_project_json(r#"{"version":"1.0.0"}"#, None, &mut no_id),
        Err(ProjectLoadError::Shape(_))
    ));
    assert!(
        load_project_json(
            r#"{"arrangement":{"tracks":[]},"audioConfig":{"masterVolume":0}}"#,
            None,
            &mut no_id
        )
        .is_err()
    );
    assert!(
        load_project_json(
            r#"{"arrangement":{"tracks":[]},"sampleManifest":5}"#,
            None,
            &mut no_id
        )
        .is_err()
    );
    let p = load_project_json(
        r#"{"version":1,"arrangement":{"tracks":[]},"sampleManifest":"ab"}"#,
        None,
        &mut no_id,
    )
    .unwrap();
    assert_eq!(p.sample_manifest.len(), 2);
    assert_eq!(
        p.version_warning.as_deref(),
        Some("Unknown project version: 1")
    );
}

#[test]
fn names_titles_and_recent_projects() {
    assert_eq!(project_name(None), "Untitled");
    assert_eq!(project_name(Some("/x/song.mdaw")), "song");
    assert_eq!(project_name(Some("/x/.mdaw")), "");
    // Electron bug fixed in both apps (phase 10): Windows paths name the file, not the path.
    assert_eq!(project_name(Some("C:\\x\\song.mdaw")), "song");
    assert_eq!(window_title(Some("/a/b.mdaw"), true), "b * - Micro DAW");
    assert_eq!(window_title(None, false), "Untitled - Micro DAW");

    let mut recent = Vec::new();
    for i in 0..12 {
        recent = add_to_recent_projects(&recent, &format!("/p/{i}.mdaw"), "t");
    }
    assert_eq!(recent.len(), MAX_RECENT);
    assert_eq!(recent[0].name, "11");
    recent = add_to_recent_projects(&recent, "/p/5.mdaw", "t2");
    assert_eq!(
        (recent[0].name.as_str(), recent[0].modified_at.as_str()),
        ("5", "t2")
    );
    assert_eq!(
        recent.iter().filter(|p| p.file_path == "/p/5.mdaw").count(),
        1
    );
    assert_eq!(
        add_to_recent_projects(&[], "/p/.mdaw", "t")[0].name,
        "Untitled"
    );

    let stored = serde_json::to_string(&recent).unwrap();
    assert_eq!(parse_recent_projects(Some(&stored)), recent);
    assert!(parse_recent_projects(Some("nope")).is_empty());
    assert!(parse_recent_projects(None).is_empty());
    assert_eq!(RECENT_PROJECTS_KEY, "micro-daw-recent-projects");
}

// --- review L5: the Value-level and typed migrations agree ---

/// Tiny deterministic PRNG (xorshift64*).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// Legacy-looking arrangements: GM drum notes (with near-duplicate hits for the merge),
/// MIDI-scale velocities, kitless and kitted drum tracks, audio regions on drum tracks.
fn generated_legacy(seed: u64) -> Value {
    let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut tracks = Vec::new();
    for t in 0..(1 + r.below(4)) {
        let kind = ["drums", "drums", "synth"][r.below(3) as usize];
        let mut regions = Vec::new();
        for ri in 0..(1 + r.below(3)) {
            let mut notes = Vec::new();
            for _ in 0..r.below(14) {
                let start = r.below(16) as f64 * 0.25 + [0.0, 0.00004, 0.5][r.below(3) as usize];
                let velocity = if r.below(2) == 0 {
                    r.below(128) as f64
                } else {
                    r.below(101) as f64 / 100.0
                };
                notes.push(json!({
                    "note": 30 + r.below(60), "velocity": velocity, "startBeat": start,
                    "durationBeats": 0.25, "isDrum": r.below(2) == 0,
                }));
            }
            let mut region = json!({
                "id": format!("r{t}-{ri}"), "startBeat": ri as f64 * 4.0, "lengthBeats": 4, "notes": notes,
            });
            if r.below(6) == 0 {
                region["audio"] = json!({ "sampleId": "s", "samplePath": "/s.wav", "offsetSeconds": 0, "gainDb": 0, "originalDuration": 1 });
            }
            regions.push(region);
        }
        let mut instrument = json!({ "type": kind, "presetIndex": r.below(5) });
        if kind == "drums" && r.below(3) == 0 {
            instrument["kit"] = json!({ "id": format!("k{t}"), "name": "K", "pads": [{ "gainDb": -3, "mode": "gate" }] });
        }
        tracks.push(json!({
            "id": format!("t{t}"), "name": format!("T{t}"), "instrument": instrument, "regions": regions,
            "volume": 0, "muted": false, "solo": false, "color": "#000",
        }));
    }
    json!({ "id": format!("g{seed}"), "name": "G", "bpm": 120, "lengthBeats": 4 * r.below(12), "tracks": tracks })
}

#[test]
fn typed_and_value_migrations_agree() {
    let fx: Value =
        serde_json::from_str(include_str!("../../../parity/fixtures/migrate.json")).unwrap();
    let mut inputs: Vec<(String, Value, Option<String>)> = fx["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            let mut v = c["input"].clone();
            if c["kind"] == "template" {
                remap_template_ids_value(&mut v, &mut counter_ids()).unwrap();
            }
            (
                format!("{} {}", c["kind"], c["name"]),
                v,
                c["packsDir"].as_str().map(str::to_owned),
            )
        })
        .collect();
    inputs.extend((0..300).map(|s| (format!("generated {s}"), generated_legacy(s), None)));
    let mut compared = 0;
    let mut dropped_seen = 0;
    for (name, input, packs_dir) in inputs {
        let Ok(typed_in) = serde_json::from_value::<Arrangement>(input.clone()) else {
            continue; // ill-typed inputs only exist on the Value path
        };
        let mut v = input;
        let value_warnings =
            migrate_arrangement_value(&mut v, packs_dir.as_deref(), &mut counter_ids()).unwrap();
        let from_value: Arrangement = serde_json::from_value(v).unwrap();
        let (typed, typed_warnings) =
            migrate_arrangement(&typed_in, packs_dir.as_deref(), &mut counter_ids());
        assert_eq!(typed, from_value, "{name}");
        assert_eq!(typed_warnings, value_warnings, "{name}");
        dropped_seen += typed_warnings.len();
        compared += 1;
    }
    assert!(compared >= 300, "{compared}");
    assert!(dropped_seen > 50, "{dropped_seen}");
}
