use std::cell::Cell;
use std::rc::Rc;

use serde_json::{Value, json};

use super::*;

fn fresh(store: MemoryProfileStore) -> ControllerMapping<MemoryProfileStore> {
    ControllerMapping::new(store, Box::new(|| 0.0))
}

const SWING: KnobTarget = KnobTarget::Swing;

// --- ControllerMapping.test.ts ------------------------------------------------------------------

#[test]
fn default_profile_maps_pads_and_knobs() {
    let p = create_default_profile();
    assert_eq!(p.pad_map.notes, (0..16).map(|i| Some(36 + i)).collect::<Vec<_>>());
    assert_eq!(p.pad_map.channel, None);
    assert_eq!(
        p.cc_mappings.iter().map(|m| m.cc).collect::<Vec<_>>(),
        vec![70, 71, 72, 73, 74, 75, 76, 77]
    );
    assert_eq!(p.cc_mappings[0].target, KnobTarget::Pad { param: PadParam::Pitch });
    assert_eq!(p.cc_mappings[7].target, KnobTarget::MasterVolume);
    assert_eq!(find_pad_for_note(&p, 36, 10), Some(0));
    assert_eq!(find_pad_for_note(&p, 51, 1), Some(15));
    assert_eq!(find_pad_for_note(&p, 60, 10), None);
}

#[test]
fn honours_pad_map_channel_filter() {
    let mut p = create_default_profile();
    p.pad_map.channel = Some(10);
    assert_eq!(find_pad_for_note(&p, 36, 10), Some(0));
    assert_eq!(find_pad_for_note(&p, 36, 1), None);
}

#[test]
fn matches_cc_mappings_by_cc_and_channel() {
    let mut p = create_default_profile();
    p.cc_mappings[0].channel = Some(2);
    assert_eq!(find_cc_mappings(&p, 70, 1), vec![]);
    assert_eq!(find_cc_mappings(&p, 70, 2).len(), 1);
    assert_eq!(find_cc_mappings(&p, 71, 5).len(), 1);
}

#[test]
fn assigning_a_used_note_moves_it() {
    let p = assign_pad_note(&create_default_profile(), 0, 40);
    assert_eq!(p.pad_map.notes[0], Some(40));
    assert_eq!(p.pad_map.notes[4], None);
}

#[test]
fn knob_learn_rebinds_target_and_removes_cc_from_others() {
    let p = assign_knob_cc(&create_default_profile(), &SWING, 70, None);
    let on70 = find_cc_mappings(&p, 70, 1);
    assert_eq!(on70.len(), 1);
    assert_eq!(on70[0].target, SWING);
    assert_eq!(p.cc_mappings.iter().filter(|m| m.target == SWING).count(), 1);
    assert!(
        !p.cc_mappings
            .iter()
            .any(|m| m.target == KnobTarget::Pad { param: PadParam::Pitch })
    );
}

#[test]
fn knob_learn_with_out_of_range_index_adds_a_row() {
    let base = create_default_profile();
    let pan = KnobTarget::Pad { param: PadParam::Pan };
    let p = assign_knob_cc(&base, &pan, 20, Some(base.cc_mappings.len() as i64));
    assert_eq!(p.cc_mappings.len(), base.cc_mappings.len() + 1);
    assert_eq!(
        p.cc_mappings.last(),
        Some(&CCMapping {
            cc: 20,
            channel: None,
            target: pan
        })
    );
}

#[test]
fn round_trips_through_serialize_parse() {
    let mut p = assign_pad_note(&create_default_profile(), 3, 60);
    p.pad_map.channel = Some(10);
    assert_eq!(parse_stored_profile(Some(&serialize_profile(&p))), p);
}

#[test]
fn falls_back_to_default_on_missing_corrupt_or_wrong_version() {
    let def = create_default_profile();
    assert_eq!(parse_stored_profile(None), def);
    assert_eq!(parse_stored_profile(Some("{not json")), def);
    let dv = serde_json::to_value(&def).unwrap();
    assert_eq!(
        parse_stored_profile(Some(&json!({"version": 99, "profile": dv}).to_string())),
        def
    );
    let mut bad_notes = dv.clone();
    bad_notes["padMap"] = json!({"notes": [1, 2, 3], "channel": null});
    assert_eq!(
        parse_stored_profile(Some(
            &json!({"version": CONTROLLER_STORAGE_VERSION, "profile": bad_notes}).to_string()
        )),
        def
    );
    let mut bad_channel = dv.clone();
    bad_channel["padMap"]["channel"] = json!(17);
    assert_eq!(validate_profile(&bad_channel), None);
}

#[test]
fn drops_individual_malformed_cc_mappings() {
    let def = create_default_profile();
    let mut raw = serde_json::to_value(&def).unwrap();
    let arr = raw["ccMappings"].as_array_mut().unwrap();
    arr.push(json!({"cc": 300, "channel": null, "target": {"kind": "swing"}}));
    arr.push(json!({"cc": 5, "channel": null, "target": {"kind": "pad", "param": "nope"}}));
    arr.push(json!({"cc": 6, "channel": null, "target": {"kind": "unknown"}}));
    assert_eq!(validate_profile(&raw).map(|p| p.cc_mappings), Some(def.cc_mappings));
}

#[test]
fn loads_stored_profile_and_persists_set_profile_with_version() {
    let stored = assign_pad_note(&create_default_profile(), 0, 60);
    let cm_store = MemoryProfileStore {
        value: Some(serialize_profile(&stored)),
    };
    let mut cm = fresh(cm_store);
    assert_eq!(cm.pad_for_note(60, 1), Some(0));

    cm.set_profile(&create_default_profile());
    let saved: Value = serde_json::from_str(cm.store().value.as_deref().unwrap()).unwrap();
    assert_eq!(saved["version"], json!(CONTROLLER_STORAGE_VERSION));
    assert_eq!(saved["profile"]["padMap"]["notes"][0], json!(36));
}

#[test]
fn corrupt_storage_falls_back_to_default() {
    let cm = fresh(MemoryProfileStore {
        value: Some(r#"{"version":1,"profile":{"id":3}}"#.into()),
    });
    assert_eq!(cm.get_profile(), &create_default_profile());
}

#[test]
fn sequence_wizard_hits_backspace_duplicates() {
    let mut cm = fresh(MemoryProfileStore::default());
    let v0 = cm.version();
    cm.start_pad_learn(PadLearn::Sequence);
    assert_eq!(cm.get_learn_state(), LearnState::Pad { pad: 0, sequence: true });

    assert!(cm.handle_note_on(60, 0.8, 10));
    assert!(cm.handle_note_on(61, 0.8, 10));
    assert!(matches!(cm.get_learn_state(), LearnState::Pad { pad: 2, .. }));
    cm.previous_pad();
    assert!(matches!(cm.get_learn_state(), LearnState::Pad { pad: 1, .. }));
    cm.handle_note_on(60, 0.8, 10); // pad 1 takes note 60 from pad 0
    assert_eq!(cm.get_profile().pad_map.notes[0..2], [None, Some(60)]);
    cm.skip_pad();
    assert!(matches!(cm.get_learn_state(), LearnState::Pad { pad: 3, .. }));
    for n in 0..13 {
        cm.handle_note_on(70 + n, 0.8, 10);
    }
    assert_eq!(cm.get_learn_state(), LearnState::Idle);
    assert_eq!(cm.get_profile().pad_map.notes[15], Some(82));
    // All wizard hits came from channel 10 → pads narrowed to channel 10.
    assert_eq!(cm.get_profile().pad_map.channel, Some(10));
    assert!(cm.version() > v0);
}

#[test]
fn per_pad_learn_consumes_one_note_cancel_leaves_profile() {
    let mut cm = fresh(MemoryProfileStore::default());
    cm.start_pad_learn(PadLearn::Pad(5));
    assert!(cm.handle_note_on(72, 1.0, 1));
    assert_eq!(cm.pad_for_note(72, 1), Some(5));
    assert!(!cm.is_learning());
    assert!(!cm.handle_note_on(72, 1.0, 1));

    cm.start_pad_learn(PadLearn::Pad(6));
    cm.cancel_learn();
    assert!(!cm.handle_note_on(90, 1.0, 1));
    assert_eq!(cm.pad_for_note(90, 1), None);
}

#[test]
fn knob_learn_binds_next_cc_and_mapped_ccs_dispatch() {
    let mut cm = fresh(MemoryProfileStore::default());
    let mut knob: Vec<KnobEvent> = Vec::new();

    let out = cm.handle_cc(70, 127.0, 1);
    assert!(out.consumed);
    knob.extend(out.knobs);
    assert_eq!(
        knob.last(),
        Some(&KnobEvent {
            target: KnobTarget::Pad { param: PadParam::Pitch },
            value01: 1.0
        })
    );
    assert!(!cm.handle_cc(20, 64.0, 1).consumed);

    cm.start_knob_learn(KnobTarget::Bpm, None);
    let out = cm.handle_cc(20, 10.0, 1);
    assert!(out.consumed); // consumed by the learner, not dispatched
    knob.extend(out.knobs);
    assert_eq!(knob.len(), 1);
    assert!(!cm.is_learning());
    knob.extend(cm.handle_cc(20, 0.0, 1).knobs);
    assert_eq!(
        knob.last(),
        Some(&KnobEvent {
            target: KnobTarget::Bpm,
            value01: 0.0
        })
    );
    assert_eq!(find_cc_mappings(cm.get_profile(), 75, 1), vec![]); // bpm moved off CC 75
    // (`off()` = the caller stops delivering the returned events.)
}

#[test]
fn records_the_last_activity() {
    let t = Rc::new(Cell::new(5.0));
    let c = t.clone();
    let mut cm = ControllerMapping::new(MemoryProfileStore::default(), Box::new(move || c.get()));
    let a0 = cm.activity_version();
    cm.handle_note_on(37, 0.5, 10);
    assert_eq!(
        cm.get_last_activity(),
        Some(&MidiActivity::Note {
            note: 37,
            velocity: 0.5,
            channel: 10,
            pad: Some(1),
            at: 5.0
        })
    );
    t.set(9.0);
    cm.handle_cc(71, 30.0, 1);
    assert_eq!(
        cm.get_last_activity(),
        Some(&MidiActivity::Cc {
            cc: 71,
            value: 30.0,
            channel: 1,
            targets: vec![KnobTarget::Pad { param: PadParam::Decay }],
            at: 9.0
        })
    );
    assert_eq!(cm.activity_version() - a0, 2);
}

// --- golden fixtures ----------------------------------------------------------------------------

fn fixture() -> Value {
    serde_json::from_str(include_str!("../../../parity/fixtures/controller.json")).unwrap()
}

fn profile(v: &Value) -> ControllerProfile {
    serde_json::from_value(v.clone()).unwrap()
}

fn target(v: &Value) -> KnobTarget {
    serde_json::from_value(v.clone()).unwrap()
}

/// Serialize, writing integral floats as integers (JSON.stringify has a single number type).
fn to_json<T: serde::Serialize>(t: &T) -> Value {
    norm(serde_json::to_value(t).unwrap())
}

fn norm(v: Value) -> Value {
    match v {
        Value::Number(n) => match n.as_f64() {
            Some(f) if n.is_f64() && f.fract() == 0.0 && f.abs() < 1e15 => json!(f as i64),
            _ => Value::Number(n),
        },
        Value::Array(a) => Value::Array(a.into_iter().map(norm).collect()),
        Value::Object(m) => Value::Object(m.into_iter().map(|(k, v)| (k, norm(v))).collect()),
        other => other,
    }
}

#[test]
fn golden_serialize_is_byte_identical() {
    let data = fixture();
    for row in data["serialize"].as_array().unwrap() {
        let p = profile(&row[0]);
        assert_eq!(serialize_profile(&p), row[1].as_str().unwrap());
    }
}

#[test]
fn golden_lookups_and_edits_match() {
    let data = fixture();
    let profiles: Vec<ControllerProfile> = data["serialize"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| profile(&r[0]))
        .collect();
    let mut n = 0;
    for row in data["lookups"].as_array().unwrap() {
        let p = &profiles[row[1].as_u64().unwrap() as usize];
        let (a, ch) = (row[2].as_i64().unwrap(), row[3].as_i64().unwrap());
        match row[0].as_str().unwrap() {
            "pad" => assert_eq!(to_json(&find_pad_for_note(p, a, ch)), row[4], "{row}"),
            _ => assert_eq!(to_json(&find_cc_mappings(p, a, ch)), row[4], "{row}"),
        }
        n += 1;
    }
    for row in data["edits"].as_array().unwrap() {
        let p = &profiles[row[1].as_u64().unwrap() as usize];
        match row[0].as_str().unwrap() {
            "pad" => {
                let got = assign_pad_note(p, row[2].as_u64().unwrap() as usize, row[3].as_i64().unwrap());
                assert_eq!(to_json(&got), row[4], "{row}");
            }
            _ => {
                let got = assign_knob_cc(p, &target(&row[2]), row[3].as_i64().unwrap(), row[4].as_i64());
                assert_eq!(to_json(&got), row[5], "{row}");
            }
        }
        n += 1;
    }
    for row in data["targetKeys"].as_array().unwrap() {
        assert_eq!(
            to_json(&knob_target_from_key(row[0].as_str().unwrap())),
            row[1],
            "{row}"
        );
    }
    for row in data["targetLabels"].as_array().unwrap() {
        let t = target(&row[0]);
        assert_eq!(knob_target_key(&t), row[1].as_str().unwrap());
        assert_eq!(knob_target_label(&t), row[2].as_str().unwrap());
    }
    assert_eq!(
        KNOB_TARGET_OPTIONS
            .iter()
            .map(|o| to_json(&o.target))
            .collect::<Vec<_>>(),
        data["targetLabels"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r[0].clone())
            .collect::<Vec<_>>()
    );
    assert!(n > 9000, "{n}");
}

#[test]
fn golden_validate_and_parse_match() {
    let data = fixture();
    for row in data["validate"].as_array().unwrap() {
        assert_eq!(to_json(&validate_profile(&row[0])), row[1], "validate {}", row[0]);
    }
    for row in data["parse"].as_array().unwrap() {
        let got = parse_stored_profile(row[0].as_str());
        assert_eq!(to_json(&got), row[1], "parse {}", row[0]);
    }
}

#[test]
fn golden_learn_sessions_match() {
    let data = fixture();
    let profiles: Vec<ControllerProfile> = data["serialize"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| profile(&r[0]))
        .collect();
    let mut n = 0;
    for (si, s) in data["sessions"].as_array().unwrap().iter().enumerate() {
        let mut cm = fresh(MemoryProfileStore {
            value: s["initial"].as_str().map(str::to_string),
        });
        let v0 = cm.version();
        let a0 = cm.activity_version();
        let mut exp_profile = Value::Null;
        let mut exp_stored = Value::Null;
        for (i, step) in s["steps"].as_array().unwrap().iter().enumerate() {
            let op = step[0].as_array().unwrap();
            let mut ret = Value::Null;
            let mut knobs: Vec<Value> = Vec::new();
            match op[0].as_str().unwrap() {
                "init" => {}
                "startPadLearn" => cm.start_pad_learn(match &op[1] {
                    Value::String(_) => PadLearn::Sequence,
                    v => PadLearn::Pad(v.as_i64().unwrap()),
                }),
                "startKnobLearn" => cm.start_knob_learn(target(&op[1]), op[2].as_i64()),
                "cancelLearn" => cm.cancel_learn(),
                "skipPad" => cm.skip_pad(),
                "previousPad" => cm.previous_pad(),
                "noteOn" => {
                    ret = json!(cm.handle_note_on(
                        op[1].as_i64().unwrap(),
                        op[2].as_f64().unwrap(),
                        op[3].as_i64().unwrap()
                    ));
                }
                "cc" => {
                    let out = cm.handle_cc(
                        op[1].as_i64().unwrap(),
                        op[2].as_f64().unwrap(),
                        op[3].as_i64().unwrap(),
                    );
                    ret = json!(out.consumed);
                    knobs = out
                        .knobs
                        .iter()
                        .map(|k| norm(json!([to_json(&k.target), k.value01])))
                        .collect();
                }
                "clearPad" => cm.clear_pad(op[1].as_i64().unwrap()),
                "setPadChannel" => cm.set_pad_channel(op[1].as_i64()),
                "setCCMapping" => {
                    let p = &op[2];
                    let patch = CCMappingPatch {
                        cc: p.get("cc").map(|v| v.as_i64().unwrap()),
                        channel: p.get("channel").map(Value::as_i64),
                        target: p.get("target").map(target),
                    };
                    cm.set_cc_mapping(op[1].as_i64().unwrap(), patch);
                }
                "removeCCMapping" => cm.remove_cc_mapping(op[1].as_i64().unwrap()),
                "resetToDefault" => cm.reset_to_default(),
                "setProfileIndex" => cm.set_profile(&profiles[op[1].as_u64().unwrap() as usize]),
                "setInvalidProfile" => {
                    let mut p = create_default_profile();
                    p.pad_map.notes[0] = Some(200);
                    cm.set_profile(&p);
                }
                "reload" => {
                    // The TS dump writes localStorage directly, then calls reload().
                    let raw = match &op[1] {
                        Value::Null => None,
                        Value::String(s) => Some(s.clone()),
                        v => Some(serialize_profile(&profiles[v.as_u64().unwrap() as usize])),
                    };
                    cm.store.value = raw;
                    cm.reload();
                }
                other => panic!("{other}"),
            }
            let ctx = format!("session {si} step {i} op {op:?}");
            assert_eq!(ret, step[1], "{ctx}: return");
            assert_eq!(Value::Array(knobs), step[2], "{ctx}: knobs");
            let exp = &step[3];
            assert_eq!(to_json(&cm.get_learn_state()), exp["learn"], "{ctx}: learn");
            let mut activity = to_json(&cm.get_last_activity());
            if let Value::Object(m) = &mut activity {
                m.remove("at");
            }
            assert_eq!(activity, exp["activity"], "{ctx}: activity");
            assert_eq!(json!(cm.version() - v0), exp["changes"], "{ctx}: changes");
            assert_eq!(
                json!(cm.activity_version() - a0),
                exp["activityCount"],
                "{ctx}: activity count"
            );
            if let Some(p) = exp.get("profile") {
                exp_profile = p.clone();
            }
            assert_eq!(to_json(cm.get_profile()), exp_profile, "{ctx}: profile");
            if let Some(st) = exp.get("stored") {
                exp_stored = match st {
                    Value::Bool(true) => json!(serialize_profile(&profile(&exp_profile))),
                    other => other.clone(),
                };
            }
            assert_eq!(json!(cm.store().value), exp_stored, "{ctx}: stored");
            n += 1;
        }
    }
    assert!(n > 2000, "{n}");
}
