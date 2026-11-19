//! Ported from src/audio/sampler/factoryKit.test.ts, the pure parts of
//! src/audio/sampler/packSources.test.ts and the kit parts of classic/classic.test.ts.

use super::*;
use serde_json::json;
use std::collections::HashSet;

fn no_id() -> String {
    panic!("unexpected id generation")
}

fn kit_of(raw: Value) -> KitState {
    normalize_kit(&raw, None, &mut no_id)
}

fn factory_voice_of(pad: &PadSettings) -> Option<FactoryVoiceId> {
    match pad.sample.as_ref().map(|s| &s.source) {
        Some(PadSampleSource::Factory { voice }) => Some(*voice),
        _ => None,
    }
}

// --- factoryKit.test.ts: 'factory kit' ---

#[test]
fn has_16_pads_each_with_a_distinct_factory_voice() {
    let kit = create_factory_kit();
    assert_eq!(kit.pads.len(), PAD_COUNT);
    let voices: Vec<_> = kit.pads.iter().map(factory_voice_of).collect();
    assert_eq!(voices.iter().collect::<HashSet<_>>().len(), PAD_COUNT);
    assert_eq!(
        voices[..4],
        [
            Some(FactoryVoiceId::Kick),
            Some(FactoryVoiceId::Snare),
            Some(FactoryVoiceId::Chh),
            Some(FactoryVoiceId::Ohh)
        ]
    );
    assert_eq!(kit.pads[0].sample.as_ref().unwrap().name, "Kick");
}

#[test]
fn every_factory_voice_has_a_label_and_a_gm_note() {
    // (The render-spec half of this vitest case belongs to the DSP crate.)
    assert_eq!(FACTORY_VOICE_IDS.len(), 16);
    for id in FACTORY_VOICE_IDS {
        assert!(!factory_voice_label(id).is_empty());
        assert!(factory_voice_gm_note(id) > 0);
    }
    assert_eq!(PAD_COLORS.len(), PAD_COUNT);
}

#[test]
fn hi_hats_share_a_choke_group() {
    let kit = create_factory_kit();
    assert!(kit.pads[2].choke_group > 0);
    assert_eq!(kit.pads[3].choke_group, kit.pads[2].choke_group);
    assert_eq!(kit.pads[0].choke_group, 0);
}

#[test]
fn returns_fresh_objects_every_call() {
    let mut a = create_factory_kit();
    a.pads[0].gain_db = -12.0;
    assert_eq!(create_factory_kit().pads[0].gain_db, 0.0);
}

#[test]
fn maps_gm_drum_notes_to_factory_pads() {
    let kit = create_factory_kit();
    let voice_at =
        |note: f64| factory_pad_for_gm_note(note).and_then(|p| factory_voice_of(&kit.pads[p]));
    assert_eq!(voice_at(36.0), Some(FactoryVoiceId::Kick));
    assert_eq!(voice_at(38.0), Some(FactoryVoiceId::Snare));
    assert_eq!(voice_at(42.0), Some(FactoryVoiceId::Chh));
    assert_eq!(voice_at(46.0), Some(FactoryVoiceId::Ohh));
    assert_eq!(voice_at(41.0), Some(FactoryVoiceId::TomLo));
    assert_eq!(voice_at(50.0), Some(FactoryVoiceId::TomHi));
    assert_eq!(factory_pad_for_gm_note(60.0), None);
    for id in FACTORY_VOICE_IDS {
        if id == FactoryVoiceId::KickSub {
            continue; // GM 35 is an acoustic kick: maps to 'kick'
        }
        assert_eq!(voice_at(factory_voice_gm_note(id) as f64), Some(id));
    }
}

// --- factoryKit.test.ts: 'normalizeKit / cloneKit' ---

#[test]
fn fills_missing_pads_and_clamps_values() {
    let kit = kit_of(json!({
        "id": "k",
        "name": "Mine",
        "pads": [
            { "gainDb": 99, "pan": -5, "pitch": 40, "decay": 0, "filterCutoff": 1e6, "mode": "gate", "chokeGroup": 2.4,
              "sample": { "name": "x", "source": { "kind": "file", "path": "/a/b.wav" } } },
            { "sample": { "source": { "kind": "factory", "voice": "nope" } } },
        ],
    }));
    assert_eq!(kit.pads.len(), PAD_COUNT);
    let p = &kit.pads[0];
    assert_eq!(
        (
            p.gain_db,
            p.pan,
            p.pitch,
            p.decay,
            p.filter_cutoff,
            p.mode,
            p.choke_group
        ),
        (12.0, -1.0, 24.0, 0.005, 20000.0, PadPlayMode::Gate, 2)
    );
    assert_eq!(
        p.sample.as_ref().unwrap().source,
        PadSampleSource::File {
            path: "/a/b.wav".into(),
            sample_id: None
        }
    );
    assert_eq!(kit.pads[1].sample, None);
    assert_eq!(kit.pads[5], default_pad_settings());
}

#[test]
fn drops_an_end_sec_that_is_not_after_start_sec() {
    let mut ids = 0;
    let mut next_id = || {
        ids += 1;
        format!("id-{ids}")
    };
    let kit = normalize_kit(
        &json!({ "pads": [{ "startSec": 1, "endSec": 0.5 }] }),
        None,
        &mut next_id,
    );
    assert_eq!(kit.pads[0].end_sec, None);
    assert_eq!((kit.id.as_str(), kit.name.as_str()), ("id-1", "Kit"));
}

#[test]
fn clones_deeply() {
    let a = create_factory_kit();
    let mut b = clone_kit(&a);
    b.pads[0].sample.as_mut().unwrap().name = "changed".into();
    assert_eq!(a.pads[0].sample.as_ref().unwrap().name, "Kick");
}

// --- packSources.test.ts: 'normalizePad migration (P1)' ---

const PACKS: &str = "/data/packs";

fn kick() -> PadSampleSource {
    PadSampleSource::Pack {
        pack_id: "tr808-fischer".into(),
        file: "bass-drum/bd0010.wav".into(),
    }
}

#[test]
fn keeps_pack_sources_and_validates_them() {
    let pad = |v: Value| normalize_pad(&v, Some(PACKS));
    assert_eq!(
        pad(json!({ "sample": { "source": { "kind": "pack", "packId": "tr808-fischer", "file": "bass-drum/bd0010.wav" }, "name": "Kick" } }))
            .sample,
        Some(PadSample { source: kick(), name: "Kick".into() })
    );
    assert_eq!(
        pad(
            json!({ "sample": { "source": { "kind": "pack", "packId": "../x", "file": "a.wav" } } })
        )
        .sample,
        None
    );
    assert_eq!(
        pad(json!({ "sample": { "source": { "kind": "pack", "packId": "p", "file": "../../a.wav" } } })).sample,
        None
    );
    assert_eq!(
        pad(json!({ "sample": { "source": { "kind": "pack", "packId": "p", "file": "kicks/bd.wav" } } }))
            .sample
            .unwrap()
            .name,
        "bd"
    );
}

#[test]
fn migrates_absolute_paths_inside_the_packs_folder_to_pack_sources() {
    let kit = normalize_kit(
        &json!({
            "id": "k",
            "name": "K",
            "pads": [
                { "sample": { "source": { "kind": "file", "path": format!("{PACKS}/tr808-fischer/bass-drum/bd0010.wav"), "sampleId": "stale" }, "name": "Kick" } },
                { "sample": { "source": { "kind": "file", "path": "/Users/me/Samples/snare.wav" }, "name": "Snare" } },
            ],
        }),
        Some(PACKS),
        &mut no_id,
    );
    assert_eq!(
        kit.pads[0].sample,
        Some(PadSample {
            source: kick(),
            name: "Kick".into()
        })
    );
    assert_eq!(
        kit.pads[1].sample.as_ref().unwrap().source,
        PadSampleSource::File {
            path: "/Users/me/Samples/snare.wav".into(),
            sample_id: None
        }
    );
}

#[test]
fn does_not_migrate_when_the_packs_folder_is_unknown() {
    let pad = normalize_pad(
        &json!({ "sample": { "source": { "kind": "file", "path": format!("{PACKS}/p/a.wav") }, "name": "a" } }),
        None,
    );
    assert!(matches!(
        pad.sample.unwrap().source,
        PadSampleSource::File { .. }
    ));
}

#[test]
fn pack_id_of_source_and_keys() {
    // packSources.test.ts 'resolves against the packs folder' (pure half)
    assert_eq!(
        pack_id_of_source(&kick(), Some(PACKS)).as_deref(),
        Some("tr808-fischer")
    );
    let legacy = PadSampleSource::File {
        path: format!("{PACKS}/uzu-drumkit/kick/a.wav"),
        sample_id: None,
    };
    assert_eq!(
        pack_id_of_source(&legacy, Some(PACKS)).as_deref(),
        Some("uzu-drumkit")
    );
    let gen0 = |_: &str| 0;
    let gen3 = |id: &str| if id == "tr808-fischer" { 3 } else { 0 };
    assert_eq!(
        pad_source_key(&kick(), Some(PACKS), &gen0),
        "pack:tr808-fischer@0:bass-drum/bd0010.wav"
    );
    assert_ne!(
        pad_source_key(&kick(), Some(PACKS), &gen3),
        pad_source_key(&kick(), Some(PACKS), &gen0)
    );
    assert_eq!(
        pad_source_key(&legacy, Some(PACKS), &gen0),
        format!("file@0:{PACKS}/uzu-drumkit/kick/a.wav")
    );
    assert_eq!(
        pad_source_key(&legacy, None, &gen0),
        format!("file:{PACKS}/uzu-drumkit/kick/a.wav")
    );
    // classic.test.ts 'caches classic pad buffers under a machine+voice key' (key half)
    let guiro = PadSampleSource::Classic {
        machine: ClassicMachineId::Cr78,
        voice: "guiro".into(),
    };
    assert_eq!(pad_source_key(&guiro, None, &gen0), "classic:cr78:guiro");
    let other = PadSampleSource::Classic {
        machine: ClassicMachineId::Tr808,
        voice: "guiro".into(),
    };
    assert_ne!(
        pad_source_key(&other, None, &gen0),
        pad_source_key(&guiro, None, &gen0)
    );
    assert_eq!(
        pad_source_key(
            &PadSampleSource::Factory {
                voice: FactoryVoiceId::KickSub
            },
            None,
            &gen0
        ),
        "factory:kick-sub"
    );
}

// --- classic.test.ts: 'createClassicKit' ---

#[test]
fn create_classic_kit_for_every_machine() {
    for id in ClassicMachineId::ALL {
        let kit = create_classic_kit(id);
        assert_eq!(kit.id, classic_kit_id(id));
        assert!(kit.name.ends_with("-style"));
        assert_eq!(kit.pads.len(), PAD_COUNT);
        let voices: Vec<Option<String>> = kit
            .pads
            .iter()
            .map(|p| match p.sample.as_ref().map(|s| &s.source) {
                Some(PadSampleSource::Classic { voice, .. }) => Some(voice.clone()),
                _ => None,
            })
            .collect();
        assert!(voices.iter().all(|v| {
            v.as_ref()
                .is_some_and(|v| get_classic_voice(id, v).is_some())
        }));
        assert_eq!(voices.iter().collect::<HashSet<_>>().len(), PAD_COUNT);
        let v = |i: usize| voices[i].as_deref().unwrap();
        assert_eq!([v(0), v(1), v(2), v(3)], ["kick", "snare", "chh", "ohh"]);
        assert_eq!(kit.pads[2].choke_group, 1);
        assert_eq!(kit.pads[3].choke_group, 1);
        assert_eq!(kit.pads[0].choke_group, 0);
        let name0 = &kit.pads[0].sample.as_ref().unwrap().name;
        assert_eq!(
            Some(name0.as_str()),
            get_classic_voice(id, "kick").map(|v| v.label)
        );
        assert!(name0.starts_with("Kick"));
        // Valid persisted state: normalizeKit keeps it unchanged.
        let json = serde_json::to_value(&kit).unwrap();
        assert_eq!(normalize_kit(&json, None, &mut no_id), kit);
    }
}

#[test]
fn names_samples_after_the_machine() {
    assert_eq!(
        classic_sample(ClassicMachineId::Tr808, "kick"),
        PadSample {
            source: PadSampleSource::Classic {
                machine: ClassicMachineId::Tr808,
                voice: "kick".into()
            },
            name: "808 Kick".into()
        }
    );
    assert_eq!(
        classic_sample(ClassicMachineId::Linn, "sidestick").name,
        "LinnDrum Sidestick"
    );
}

// --- classic.test.ts: 'normalizeKit with classic sources' ---

#[test]
fn keeps_valid_classic_sources_and_fills_a_missing_name() {
    let kit = kit_of(
        json!({ "id": "k", "name": "K", "pads": [{ "sample": { "source": { "kind": "classic", "machine": "tr909", "voice": "clap" } } }] }),
    );
    assert_eq!(
        kit.pads[0].sample,
        Some(PadSample {
            source: PadSampleSource::Classic {
                machine: ClassicMachineId::Tr909,
                voice: "clap".into()
            },
            name: "909 Clap".into()
        })
    );
}

#[test]
fn drops_unknown_machines_and_voices() {
    let kit = normalize_kit(
        &json!({ "pads": [
            { "sample": { "source": { "kind": "classic", "machine": "tr999", "voice": "kick" }, "name": "x" } },
            { "sample": { "source": { "kind": "classic", "machine": "tr808", "voice": "nope" }, "name": "x" } },
            { "sample": { "source": { "kind": "classic", "machine": "tr808", "voice": 42 }, "name": "x" } },
        ] }),
        None,
        &mut || "k".into(),
    );
    assert!(kit.pads[..3].iter().all(|p| p.sample.is_none()));
}

#[test]
fn source_fallback_names() {
    assert_eq!(
        source_fallback_name(&PadSampleSource::Factory {
            voice: FactoryVoiceId::Perc
        }),
        "Cowbell"
    );
    assert_eq!(
        source_fallback_name(&PadSampleSource::File {
            path: "/".into(),
            sample_id: None
        }),
        "/"
    );
    assert_eq!(
        source_fallback_name(&PadSampleSource::File {
            path: "/x/.wav".into(),
            sample_id: None
        }),
        ".wav"
    );
    assert_eq!(
        source_fallback_name(&PadSampleSource::File {
            path: "C:\\s\\Snare 2.aif".into(),
            sample_id: None
        }),
        "Snare 2"
    );
}

// --- golden: rune/parity/fixtures/kit.json (rune/parity/ts/kit.golden.ts) ---

mod golden {
    use super::super::*;
    use crate::arrangement_helpers::{drum_note_to_gm, migrate_drum_track, migrate_gm_drum_notes};
    use crate::test_support::{assert_json_eq, counter_ids};
    use crate::types::{RegionNote, Track};
    use serde_json::{Value, json};

    fn fixture() -> Value {
        serde_json::from_str(include_str!("../../../parity/fixtures/kit.json")).unwrap()
    }

    fn rows<'a>(fx: &'a Value, key: &str) -> &'a Vec<Value> {
        fx[key].as_array().unwrap()
    }

    #[test]
    fn normalize_pad_and_kit_match_ts() {
        let fx = fixture();
        for r in rows(&fx, "pads") {
            let got = serde_json::to_value(normalize_pad(&r[0], r[1].as_str())).unwrap();
            assert_json_eq(&got, &r[2], &format!("normalizePad({}, {})", r[0], r[1]));
        }
        for r in rows(&fx, "kits") {
            let got = serde_json::to_value(normalize_kit(&r[0], r[1].as_str(), &mut counter_ids()))
                .unwrap();
            assert_json_eq(&got, &r[2], &format!("normalizeKit({}, {})", r[0], r[1]));
        }
        assert!(rows(&fx, "pads").len() > 100 && rows(&fx, "kits").len() > 20);
    }

    #[test]
    fn factory_and_classic_kits_match_ts() {
        let fx = fixture();
        assert_json_eq(
            &serde_json::to_value(create_factory_kit()).unwrap(),
            &fx["factory"],
            "createFactoryKit",
        );
        let classic = rows(&fx, "classic");
        assert_eq!(classic.len(), 6);
        for r in classic {
            let id = ClassicMachineId::from_id(r[0].as_str().unwrap()).unwrap();
            assert_json_eq(
                &serde_json::to_value(create_classic_kit(id)).unwrap(),
                &r[1],
                r[0].as_str().unwrap(),
            );
        }
    }

    #[test]
    fn gm_mapping_matches_ts() {
        let fx = fixture();
        for r in rows(&fx, "gmVoice") {
            let x = r[0].as_f64().unwrap();
            let voice = gm_note_to_factory_voice(x)
                .map(|v| json!(v.id()))
                .unwrap_or(Value::Null);
            let pad = factory_pad_for_gm_note(x)
                .map(|p| json!(p))
                .unwrap_or(Value::Null);
            assert_json_eq(
                &json!([voice, pad]),
                &json!([r[1], r[2]]),
                &format!("gm {x}"),
            );
        }
        for r in rows(&fx, "gmNotes") {
            let notes: Vec<RegionNote> = serde_json::from_value(r[0].clone()).unwrap();
            let res = migrate_gm_drum_notes(&notes);
            let got = json!({ "notes": res.notes, "dropped": res.dropped });
            assert_json_eq(&got, &r[1], &format!("migrateGmDrumNotes({})", r[0]));
        }
        let mut untyped = 0;
        for r in rows(&fx, "tracks") {
            // A kit that is a truthy non-object, or a pad sample without a name, has no typed form
            // (types.rs is strict there); the Value-level
            // migration (migrate.rs golden) covers it.
            let Ok(mut track) = serde_json::from_value::<Track>(r[0].clone()) else {
                untyped += 1;
                continue;
            };
            let dropped = migrate_drum_track(&mut track, r[1].as_str(), &mut counter_ids());
            assert_json_eq(
                &serde_json::to_value(&track).unwrap(),
                &r[2],
                &format!("migrateDrumTrack({})", r[0]),
            );
            assert_eq!(json!(dropped), r[3]);
        }
        assert_eq!(untyped, 2);
        let kits: Vec<(String, Option<KitState>)> = std::iter::once(("none".to_owned(), None))
            .chain(std::iter::once((
                "factory".to_owned(),
                Some(create_factory_kit()),
            )))
            .chain(
                ClassicMachineId::ALL
                    .iter()
                    .map(|m| (m.id().to_owned(), Some(create_classic_kit(*m)))),
            )
            .collect();
        for r in rows(&fx, "gmExport") {
            let kit = kits
                .iter()
                .find(|(n, _)| n == r[0].as_str().unwrap())
                .unwrap()
                .1
                .as_ref();
            let got = drum_note_to_gm(r[1].as_f64().unwrap(), kit);
            assert_json_eq(
                &json!(got),
                &r[2],
                &format!("drumNoteToGm({}, {})", r[1], r[0]),
            );
        }
        for r in rows(&fx, "fallbackNames") {
            let source: PadSampleSource = serde_json::from_value(r[0].clone()).unwrap();
            assert_eq!(json!(source_fallback_name(&source)), r[1], "{}", r[0]);
        }
    }
}
