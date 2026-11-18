//! Golden parity: `rune/parity/ts/doc.golden.ts` drives the real TS `arrangementEngine` through
//! scripted edit sessions; every step is replayed here on one `ArrangementDoc` and compared:
//! arrangement JSON, undo/redo labels, canUndo/canRedo, session id, loop flags, clipboard flag and
//! the op's return value (numbers within 1e-9, key order ignored).
//!
//! The TS arrangement is canonicalized through the typed model (`Arrangement` deserialize +
//! serialize) before comparing: the typed model cannot express "key absent" for defaulted fields
//! (e.g. a template note without `isDrum` reads back as `isDrum: false`).

use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use super::{
    ArrangementDoc, EffectName, KitSelectionSource, LoopSettingsPatch, MarkerUpdate,
    SYNTH_PRESET_NAMES,
};
use crate::types::{
    Arrangement, AutomationParameter, InstrumentType, KitState, Marker, PadSample, Region,
    RegionNote, Track, TrackGroup, TrackInstrument,
};

fn fixture() -> Value {
    serde_json::from_str(include_str!("../../../../parity/fixtures/doc.json")).expect("doc.json")
}

fn counter_ids() -> super::IdGenerator {
    let mut n = 0;
    Box::new(move || {
        n += 1;
        format!("uuid-{n}")
    })
}

/// Structural JSON diff (numbers within 1e-9, object key order ignored). None when equal.
fn json_diff(got: &Value, want: &Value, path: &str) -> Option<String> {
    match (got, want) {
        (Value::Number(a), Value::Number(b)) => {
            let (a, b) = (a.as_f64().unwrap(), b.as_f64().unwrap());
            ((a - b).abs() > 1e-9).then(|| format!("{path}: {a} != {b}"))
        }
        (Value::Array(a), Value::Array(b)) => {
            if a.len() != b.len() {
                return Some(format!("{path}: length {} != {}", a.len(), b.len()));
            }
            a.iter()
                .zip(b)
                .enumerate()
                .find_map(|(i, (x, y))| json_diff(x, y, &format!("{path}[{i}]")))
        }
        (Value::Object(a), Value::Object(b)) => {
            let mut keys: Vec<&String> = a.keys().chain(b.keys()).collect();
            keys.sort();
            keys.dedup();
            keys.into_iter().find_map(|k| match (a.get(k), b.get(k)) {
                (Some(x), Some(y)) => json_diff(x, y, &format!("{path}.{k}")),
                (x, y) => Some(format!("{path}.{k}: {x:?} != {y:?}")),
            })
        }
        _ => (got != want).then(|| format!("{path}: {got} != {want}")),
    }
}

/// Round-trip a TS value through a typed model value.
fn canon<T: DeserializeOwned + serde::Serialize>(v: &Value) -> Value {
    if v.is_null() {
        return Value::Null;
    }
    let typed: T = serde_json::from_value(v.clone()).unwrap_or_else(|e| panic!("canon: {e}: {v}"));
    serde_json::to_value(typed).unwrap()
}

fn de<T: DeserializeOwned>(v: &Value) -> T {
    serde_json::from_value(v.clone()).unwrap_or_else(|e| panic!("arg {v}: {e}"))
}

fn s(v: &Value) -> &str {
    v.as_str()
        .unwrap_or_else(|| panic!("expected a string, got {v}"))
}

fn f(v: &Value) -> f64 {
    v.as_f64()
        .unwrap_or_else(|| panic!("expected a number, got {v}"))
}

fn opt_f(v: Option<&Value>) -> Option<f64> {
    v.and_then(Value::as_f64)
}

fn obj(v: &Value) -> Map<String, Value> {
    v.as_object().cloned().unwrap_or_default()
}

fn kind(v: Option<&Value>) -> InstrumentType {
    v.map_or(InstrumentType::Synth, de)
}

/// Pad / step arguments: a non-index number maps to an out-of-range pad (TS `isPadIndex` false).
fn pad(v: &Value) -> usize {
    let n = f(v);
    if n >= 0.0 && n.fract() == 0.0 {
        n as usize
    } else {
        usize::MAX
    }
}

fn step(v: &Value) -> i64 {
    let n = f(v);
    if n.fract() == 0.0 { n as i64 } else { -1 }
}

/// Run one op; returns its canonical return value.
fn run(doc: &mut ArrangementDoc, op: &str, a: &[Value]) -> Value {
    let arg = |i: usize| a.get(i).unwrap_or(&Value::Null);
    match op {
        "resetToStarter" => doc.reset_to_starter(),
        "addTrack" => {
            let t = doc.add_track(
                kind(a.first()),
                a.get(1).and_then(Value::as_i64).unwrap_or(0),
            );
            return serde_json::to_value(t).unwrap();
        }
        "removeTrack" => doc.remove_track(s(arg(0))),
        "setTrackVolume" => doc.set_track_volume(s(arg(0)), f(arg(1))),
        "setTrackPan" => doc.set_track_pan(s(arg(0)), f(arg(1))),
        "setTrackMute" => doc.set_track_mute(s(arg(0)), arg(1).as_bool().unwrap()),
        "setTrackSolo" => doc.set_track_solo(s(arg(0)), arg(1).as_bool().unwrap()),
        "setTrackEffect" => doc.set_track_effect(s(arg(0)), de::<EffectName>(arg(1)), &obj(arg(2))),
        "setTrackSynthOverrides" => doc.set_track_synth_overrides(s(arg(0)), &obj(arg(1))),
        "setTrackInstrument" => doc.set_track_instrument(s(arg(0)), &de::<TrackInstrument>(arg(1))),
        "addRegion" => doc.add_region(s(arg(0)), de::<Region>(arg(1))),
        "removeRegion" => doc.remove_region(s(arg(0)), s(arg(1))),
        "moveRegion" => doc.move_region(s(arg(0)), s(arg(1)), f(arg(2))),
        "moveRegionToTrack" => doc.move_region_to_track(s(arg(0)), s(arg(1)), s(arg(2))),
        "splitRegion" => doc.split_region(s(arg(0)), s(arg(1)), f(arg(2))),
        "duplicateRegion" => doc.duplicate_region(s(arg(0)), s(arg(1))),
        "resizeRegion" => doc.resize_region(s(arg(0)), s(arg(1)), opt_f(a.get(2)), opt_f(a.get(3))),
        "quantizeRegionNotes" => doc.quantize_region_notes(s(arg(0)), s(arg(1)), f(arg(2))),
        "quantizeSelectedNotes" => {
            let ix: Vec<usize> = de(arg(2));
            doc.quantize_selected_notes(s(arg(0)), s(arg(1)), &ix, f(arg(3)));
        }
        "updateRegionNotes" => {
            doc.update_region_notes(s(arg(0)), s(arg(1)), de::<Vec<RegionNote>>(arg(2)))
        }
        "editRegionNotes" => doc.edit_region_notes(
            s(arg(0)),
            s(arg(1)),
            de::<Vec<RegionNote>>(arg(2)),
            arg(3).as_str(),
            opt_f(a.get(4)),
        ),
        "copyRegion" => doc.copy_region(s(arg(0)), s(arg(1))),
        "pasteRegion" => doc.paste_region(s(arg(0)), f(arg(1))),
        "setBpm" => doc.set_bpm(f(arg(0))),
        "addMarker" => {
            return serde_json::to_value(doc.add_marker(s(arg(0)), f(arg(1)), arg(2).as_str()))
                .unwrap();
        }
        "updateMarker" => doc.update_marker(s(arg(0)), &de::<MarkerUpdate>(arg(1))),
        "removeMarker" => doc.remove_marker(s(arg(0))),
        "createGroup" => {
            let ids: Vec<String> = de(arg(1));
            return serde_json::to_value(doc.create_group(s(arg(0)), &ids)).unwrap();
        }
        "addTrackToGroup" => doc.add_track_to_group(s(arg(0)), s(arg(1))),
        "removeTrackFromGroup" => doc.remove_track_from_group(s(arg(0)), s(arg(1))),
        "renameGroup" => doc.rename_group(s(arg(0)), s(arg(1))),
        "toggleGroupCollapsed" => doc.toggle_group_collapsed(s(arg(0))),
        "setGroupMute" => doc.set_group_mute(s(arg(0)), arg(1).as_bool().unwrap()),
        "setGroupSolo" => doc.set_group_solo(s(arg(0)), arg(1).as_bool().unwrap()),
        "removeGroup" => doc.remove_group(s(arg(0))),
        "addAutomationLane" => {
            doc.add_automation_lane(s(arg(0)), de::<AutomationParameter>(arg(1)))
        }
        "removeAutomationLane" => {
            doc.remove_automation_lane(s(arg(0)), de::<AutomationParameter>(arg(1)))
        }
        "setAutomationPoint" => doc.set_automation_point(
            s(arg(0)),
            de::<AutomationParameter>(arg(1)),
            f(arg(2)),
            f(arg(3)),
            f(arg(4)),
        ),
        "deleteAutomationPoint" => doc.delete_automation_point(
            s(arg(0)),
            de::<AutomationParameter>(arg(1)),
            f(arg(2)) as usize,
        ),
        "toggleAutomationLaneVisibility" => {
            doc.toggle_automation_lane_visibility(s(arg(0)), de::<AutomationParameter>(arg(1)))
        }
        "setLoopEnabled" => doc.set_loop_enabled(arg(0).as_bool().unwrap()),
        "setLoopMarkers" => doc.set_loop_markers(f(arg(0)), f(arg(1))),
        "beginUndoGroup" => doc.begin_undo_group(s(arg(0))),
        "endUndoGroup" => doc.end_undo_group(),
        "performUndo" => doc.perform_undo(),
        "performRedo" => doc.perform_redo(),
        "getActiveDrumTrackId" => {
            return doc
                .get_active_drum_track_id()
                .map_or(Value::Null, Value::String);
        }
        "ensureDrumTrack" => return Value::String(doc.ensure_drum_track()),
        "setActiveDrumTrack" => doc.set_active_drum_track(s(arg(0))),
        "armTrackForRecording" => doc.arm_track_for_recording(arg(0).as_str()),
        "getKit" => return serde_json::to_value(doc.get_kit(s(arg(0)))).unwrap(),
        "setKit" => {
            let source: KitSelectionSource = a.get(2).map_or(KitSelectionSource::User, de);
            doc.set_kit(s(arg(0)), &de::<KitState>(arg(1)), source);
        }
        "updatePad" => doc.update_pad(s(arg(0)), pad(arg(1)), &obj(arg(2))),
        "setPadSample" => {
            let sample: Option<PadSample> = de(arg(2));
            doc.set_pad_sample(s(arg(0)), pad(arg(1)), sample.as_ref());
        }
        "setSelectedPad" => doc.set_selected_pad(pad(arg(0))),
        "getSelectedPad" => return Value::from(doc.get_selected_pad()),
        "getLoopSettings" => return serde_json::to_value(doc.get_loop_settings()).unwrap(),
        "setLoopSettings" => doc.set_loop_settings(&de::<LoopSettingsPatch>(arg(0))),
        "getPatternRegion" => {
            return serde_json::to_value(doc.get_pattern_region(s(arg(0)))).unwrap();
        }
        "ensurePatternRegion" => {
            return serde_json::to_value(doc.ensure_pattern_region(s(arg(0)))).unwrap();
        }
        "toggleStep" => doc.toggle_step(
            s(arg(0)),
            pad(arg(1)),
            step(arg(2)),
            f(arg(3)),
            opt_f(a.get(4)),
        ),
        "setStepVelocity" => {
            doc.set_step_velocity(s(arg(0)), pad(arg(1)), step(arg(2)), f(arg(3)), f(arg(4)))
        }
        "clearPattern" => doc.clear_pattern(s(arg(0)), a.get(1).filter(|v| !v.is_null()).map(pad)),
        "loadProject" => doc.load_project(&de::<Arrangement>(arg(0))),
        "loadFromTemplate" => doc.load_from_template(&de::<Arrangement>(arg(0))).unwrap(),
        "restoreFromSnapshot" => doc.restore_from_snapshot(&de::<Arrangement>(arg(0))),
        "loadFromLocalStorage" => return Value::Bool(doc.load_from_local_storage(arg(0).as_str())),
        "startLoopRecording" => {
            doc.begin_loop_take();
        }
        "stopLoopRecording" => doc.stop_loop_take(),
        other => panic!("unknown op {other}"),
    }
    Value::Null
}

/// Canonical form of an op's TS return value.
fn canon_ret(op: &str, v: &Value) -> Value {
    match op {
        "addTrack" => canon::<Track>(v),
        "addMarker" => canon::<Marker>(v),
        "createGroup" => canon::<TrackGroup>(v),
        "getKit" => canon::<KitState>(v),
        "getPatternRegion" | "ensurePatternRegion" => canon::<Region>(v),
        _ => v.clone(),
    }
}

/// Apply a delta-encoded arrangement step (see doc.golden.ts) to the previous expected value.
fn apply_delta(prev: &Value, delta: &Map<String, Value>) -> Value {
    let mut out = prev.as_object().cloned().unwrap_or_default();
    for (k, v) in delta {
        if v.is_null() {
            out.remove(k);
        } else if k == "tracks"
            && let (Some(new), Some(old)) = (v.as_array(), out.get(k).and_then(Value::as_array))
            && new.len() == old.len()
            && new.iter().any(|t| t == "=")
        {
            let merged: Vec<Value> = new
                .iter()
                .zip(old)
                .map(|(n, o)| if n == "=" { o.clone() } else { n.clone() })
                .collect();
            out.insert(k.clone(), Value::Array(merged));
        } else {
            out.insert(k.clone(), v.clone());
        }
    }
    Value::Object(out)
}

#[test]
fn synth_preset_names_match_ts() {
    let fx = fixture();
    let names: Vec<String> = de(&fx["presetNames"]);
    assert_eq!(names, SYNTH_PRESET_NAMES.to_vec());
}

#[test]
fn golden_sessions_match_the_ts_engine() {
    let fx = fixture();
    let mut doc = ArrangementDoc::with_id_generator(counter_ids());
    let mut compared = 0;
    for session in fx["sessions"].as_array().unwrap() {
        let name = s(&session["name"]);
        doc.set_id_generator(counter_ids());
        let mut want_raw = Value::Null;
        for (i, step) in session["steps"].as_array().unwrap().iter().enumerate() {
            let op = s(&step["op"]);
            let args = step["args"].as_array().unwrap();
            let ctx = format!("{name} step {i} ({op} {})", Value::Array(args.clone()));
            let ret = run(&mut doc, op, args);
            doc.drain_events();

            want_raw = apply_delta(&want_raw, step["arr"].as_object().unwrap());
            let want = canon::<Arrangement>(&want_raw);
            let got = serde_json::to_value(doc.get_arrangement()).unwrap();
            if let Some(d) = json_diff(&got, &want, "$") {
                panic!("{ctx}: arrangement differs: {d}\n got: {got}\nwant: {want}");
            }
            let want_ret = canon_ret(op, &step["ret"]);
            if let Some(d) = json_diff(&ret, &want_ret, "ret") {
                panic!("{ctx}: return value differs: {d}\n got: {ret}\nwant: {want_ret}");
            }
            let u = doc.undo_manager();
            let flags = serde_json::json!({
                "undoLabel": u.undo_label(),
                "redoLabel": u.redo_label(),
                "canUndo": u.can_undo(),
                "canRedo": u.can_redo(),
                "sessionId": doc.get_session_id(),
                "loopEnabled": doc.is_loop_enabled(),
                "hasClipboard": doc.has_clipboard(),
                "loopRecording": doc.is_loop_recording(),
            });
            let want_flags = serde_json::json!({
                "undoLabel": step["undoLabel"],
                "redoLabel": step["redoLabel"],
                "canUndo": step["canUndo"],
                "canRedo": step["canRedo"],
                "sessionId": step["sessionId"],
                "loopEnabled": step["loopEnabled"],
                "hasClipboard": step["hasClipboard"],
                "loopRecording": step["loopRecording"],
            });
            if let Some(d) = json_diff(&flags, &want_flags, "flags") {
                panic!("{ctx}: {d}");
            }
            compared += 1;
        }
    }
    assert!(compared > 250, "only {compared} steps compared");
}
