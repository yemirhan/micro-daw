//! Golden parity: `rune/parity/ts/doc.golden.ts` drives the real TS `arrangementEngine` through
//! scripted edit sessions and 8 seeded random sessions (300 steps each, every doc op); every
//! step is replayed here on one `ArrangementDoc` and compared: the arrangement JSON, the op's
//! return value, the side effects (TS private-method spies vs the drained `DocEvent`s), undo/redo
//! labels, canUndo/canRedo, session id, loop flags and the clipboard flag.
//!
//! The arrangement and the return values are compared RAW (Rust JSON vs TS JSON, review M2), so
//! a dropped or defaulted field shows up. The only normalizations allowed ([`allowed`]):
//! 1. numbers compare as JS numbers within 1e-9 (`120` vs `120.0`);
//! 2. `isDrum: false` written for a note the TS stored without `isDrum` (template notes);
//! 3. `visible: false` written for an automation lane without `visible` (none in practice).
//!
//! Side effects ([`event_log`]) map each `DocEvent` onto the TS spy names; intentional
//! differences: `TrackVolumeChanged` / `TrackMuteChanged` / `TrackPanChanged` are direct
//! Tone-node writes in the TS (no method to spy) and `SwingChanged` / `MigrationWarning` have no
//! TS method either, so they map to nothing; `startLoopRecording` replays only the data half
//! (`begin_loop_take`), so the engine half that follows it in the TS (`applyLoopToTransport`
//! after the transport reset, then `scheduleAllTracks`) is appended to the Rust log.

use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::test_support::{JsonDiff, apply_patch, json_diff, json_diffs};

use super::DocEvent;

use super::{
    ArrangementDoc, EffectName, KitSelectionSource, LoopSettingsPatch, MarkerUpdate,
    SYNTH_PRESET_NAMES,
};
use crate::types::{
    Arrangement, AutomationParameter, InstrumentType, KitState, PadSample, Region, RegionNote,
    TrackInstrument,
};

fn fixture() -> Value {
    serde_json::from_str(include_str!("../../../../parity/fixtures/doc.json")).expect("doc.json")
}

fn counter_ids() -> super::IdGenerator {
    Box::new(crate::test_support::counter_ids())
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
        "loadFromLocalStorage" => {
            return Value::Bool(matches!(
                doc.load_from_local_storage(arg(0).as_str()),
                Ok(Some(_))
            ));
        }
        "startLoopRecording" => {
            if doc.begin_loop_take().is_some() {
                // Engine half: `applyLoopToTransport()` after the transport reset.
                return Value::String("$applyLoop".into());
            }
        }
        "stopLoopRecording" => doc.stop_loop_take(),
        other => panic!("unknown op {other}"),
    }
    Value::Null
}

/// `DocEvent` → the TS spy log entries it stands for (see the module docs).
fn event_log(ev: &DocEvent, out: &mut Vec<String>) {
    match ev {
        DocEvent::TrackAdded(id) => out.push(format!("create:{id}")),
        DocEvent::TrackRemoved(id) => out.push(format!("dispose:{id}")),
        DocEvent::TrackAudioRebuilt(id) => {
            out.push(format!("dispose:{id}"));
            out.push(format!("create:{id}"));
        }
        DocEvent::TrackVolumeChanged { .. }
        | DocEvent::TrackMuteChanged { .. }
        | DocEvent::TrackPanChanged { .. }
        | DocEvent::SwingChanged
        // Rust-only features (arming several tracks, live input, hosted plugins): no TS spy.
        | DocEvent::ArmChanged
        | DocEvent::TrackInputChanged(_)
        | DocEvent::TrackPluginsChanged(_)
        | DocEvent::TrackChainChanged(_)
        | DocEvent::MigrationWarning(_) => {}
        DocEvent::SoloChanged => out.push("solo".into()),
        DocEvent::TrackEffectChanged { effect, .. } => out.push(format!("effect:{}", effect.key())),
        DocEvent::TrackEffectsReset(id) => out.push(format!("resetEffects:{id}")),
        DocEvent::RescheduleTrack(id) => out.push(format!("reschedule:{id}")),
        DocEvent::KitApplied(id) => out.push(format!("applyKit:{id}")),
        DocEvent::KitSelected(e) => {
            let source = serde_json::to_value(e.source).unwrap();
            out.push(format!(
                "kitSelected:{}:{}:{}",
                e.track_id,
                e.kit_id,
                source.as_str().unwrap()
            ));
        }
        DocEvent::SyncDrumSink => out.push("syncDrumSink".into()),
        DocEvent::LoopRangeChanged | DocEvent::BpmChanged(_) => out.push("applyLoop".into()),
        DocEvent::MetronomeUpdate => out.push("metronome".into()),
        DocEvent::RebuildAll => out.push("rebuildAll".into()),
        DocEvent::LoopTakeEnded { .. } => out.push("stopTransport".into()),
    }
}

/// The documented raw-JSON normalizations (module docs); None = a real difference.
fn allowed(d: &JsonDiff) -> Option<&'static str> {
    let key = d.path.rsplit_once('.').map_or("", |(_, k)| k);
    match (key, &d.got, &d.want) {
        ("isDrum", Some(Value::Bool(false)), None) if d.path.contains(".notes[") => {
            Some("isDrum:false added")
        }
        ("visible", Some(Value::Bool(false)), None) if d.path.contains(".automation[") => {
            Some("visible:false added")
        }
        _ => None,
    }
}

#[track_caller]
fn assert_raw_eq(got: &Value, want: &Value, ctx: &str) {
    let bad: Vec<String> = json_diffs(got, want)
        .into_iter()
        .filter(|d| allowed(d).is_none())
        .map(|d| d.to_string())
        .collect();
    assert!(
        bad.is_empty(),
        "{ctx}: differs from the TS:\n{}\n got: {got}\nwant: {want}",
        bad.join("\n")
    );
}

/// Resolve `{ "$ref": ... }` args (doc.golden.ts `encodeArgs`).
fn resolve_args(args: &[Value], prev: &Value) -> Vec<Value> {
    args.iter()
        .map(|a| match a.get("$ref").and_then(Value::as_str) {
            Some("prev") => prev.clone(),
            Some("prevJson") => Value::String(prev.to_string()),
            Some(r) if r.starts_with("template:") => {
                let id = &r["template:".len()..];
                crate::test_support::templates()
                    .into_iter()
                    .find(|(t, _)| *t == id)
                    .map(|(_, t)| t["arrangement"].clone())
                    .unwrap()
            }
            Some(r) => panic!("unknown ref {r}"),
            None => a.clone(),
        })
        .collect()
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
    let (mut compared, mut random_steps, mut effects_seen) = (0, 0, 0);
    for session in fx["sessions"].as_array().unwrap() {
        let name = s(&session["name"]);
        doc.set_id_generator(counter_ids());
        let mut want = Value::Null;
        for (i, step) in session["steps"].as_array().unwrap().iter().enumerate() {
            let op = s(&step["op"]);
            let args = resolve_args(step["args"].as_array().unwrap(), &want);
            let shown: Vec<String> = step["args"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| {
                    let t = a.to_string();
                    if t.len() > 200 {
                        format!("{}…", &t[..200])
                    } else {
                        t
                    }
                })
                .collect();
            let ctx = format!("{name} step {i} ({op} [{}])", shown.join(", "));
            let mut ret = run(&mut doc, op, &args);
            let mut log = Vec::new();
            for ev in doc.drain_events() {
                event_log(&ev, &mut log);
            }
            if ret == "$applyLoop" {
                log.extend(["applyLoop".into(), "scheduleAll".into()]);
                ret = Value::Null;
            }

            for patch in step["arr"].as_array().unwrap() {
                apply_patch(&mut want, patch.as_array().unwrap());
            }
            let got = serde_json::to_value(doc.get_arrangement()).unwrap();
            assert_raw_eq(&got, &want, &format!("{ctx}: arrangement"));

            let want_ret = match step["ret"].get("$at").and_then(Value::as_str) {
                Some(ptr) => want.pointer(ptr).cloned().unwrap(),
                None => step["ret"].clone(),
            };
            assert_raw_eq(&ret, &want_ret, &format!("{ctx}: return value"));

            let want_fx: Vec<String> = serde_json::from_value(step["fx"].clone()).unwrap();
            assert_eq!(log, want_fx, "{ctx}: side effects");
            effects_seen += log.len();

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
            if name.starts_with("random-") {
                random_steps += 1;
            }
        }
    }
    assert!(compared > 2400, "only {compared} steps compared");
    assert!(random_steps >= 2000, "only {random_steps} random steps");
    assert!(
        effects_seen > 1000,
        "only {effects_seen} side effects compared"
    );
}

#[test]
fn raw_comparison_is_not_vacuous() {
    use serde_json::json;
    let ok = json!({ "notes": [{ "note": 1, "isDrum": false }] });
    let want = json!({ "notes": [{ "note": 1.0 }] });
    assert!(json_diffs(&ok, &want).iter().all(|d| allowed(d).is_some()));
    // A dropped key, a defaulted value, an added non-default key: all reported.
    for (got, want) in [
        (json!({ "a": 1 }), json!({ "a": 1, "b": 2 })),
        (
            json!({ "notes": [{ "isDrum": true }] }),
            json!({ "notes": [{}] }),
        ),
        (json!({ "color": "" }), json!({})),
    ] {
        assert!(
            json_diffs(&got, &want).iter().any(|d| allowed(d).is_none()),
            "{got} vs {want}"
        );
    }
}
