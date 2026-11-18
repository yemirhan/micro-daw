//! The routing core against the real `useMidiNotes` (`rune/parity/ts/midi_routing.golden.ts`).
//!
//! The router's actions are expanded into the TS call vocabulary by what the core does for each
//! (verified in phase 3): `PadDown` = `drum.hit` (unless pad-muted), capture (recording), flash,
//! `drum` bus event, `repeat.press`; `PadUp` = `repeat.release`, `drum.release`, capture
//! note-off (recording). Legacy CCs go through a real `ArrangementEngine::handle_legacy_cc`
//! (its pickup and live effect state). Every call of every step, and the hook state after it
//! (held notes, sustain, sounding notes, mode, learn state, profile, last activity, effect
//! params), must be identical.

mod common;

use std::sync::Arc;

use mdaw_engine::buffers::PadBufferCache;
use mdaw_engine::engine::{ArrangementEngine, NoSamples};
use mdaw_engine::fx::cc::CcAction;
use mdaw_engine::fx::live::LiveOp;
use mdaw_midi::router::{Action, BusEvent, Router, RoutingMode};
use mdaw_model::controller::{
    ControllerMapping, MemoryProfileStore, PadLearn, knob_target_from_key, knob_target_key,
    validate_profile,
};
use mdaw_model::types::pad_to_note;
use serde_json::{Value, json};

use common::{fixture, same};

struct Harness {
    router: Router<MemoryProfileStore>,
    engine: ArrangementEngine,
    rec: bool,
    pad_muted: bool,
}

impl Harness {
    /// `mapping`: the previous scenario's (the TS singleton keeps its last activity).
    fn new(
        mapping: Option<ControllerMapping<MemoryProfileStore>>,
        profile: &Value,
        volume_handler: bool,
    ) -> Self {
        let mut mapping = mapping.unwrap_or_else(|| {
            ControllerMapping::new(MemoryProfileStore::default(), Box::new(|| 0.0))
        });
        mapping.cancel_learn();
        mapping.set_profile(&validate_profile(profile).expect("profile"));
        let mut router = Router::new(mapping);
        router.set_master_volume_handler(volume_handler);
        let (engine, _core, _live) = ArrangementEngine::new(
            48_000,
            PadBufferCache::with_fs(48_000, None),
            Arc::new(NoSamples),
        );
        Self {
            router,
            engine,
            rec: false,
            pad_muted: false,
        }
    }

    /// The TS calls this action stands for.
    fn expand(&mut self, a: Action, calls: &mut Vec<Value>) {
        match a {
            Action::PadDown { pad, velocity, .. } | Action::PadHit { pad, velocity, .. } => {
                if !self.pad_muted {
                    calls.push(json!(["drum.hit", pad, velocity]));
                }
                let note = pad_to_note(pad);
                if self.rec {
                    calls.push(json!(["captureOn", note, velocity, true]));
                }
                calls.push(json!(["flash", pad]));
                calls.push(json!(["bus", {"type": "drum", "padId": pad, "note": note, "velocity": velocity}]));
                if matches!(a, Action::PadDown { .. }) {
                    calls.push(json!(["repeat.press", pad, velocity]));
                }
            }
            Action::PadUp { pad, .. } => {
                calls.push(json!(["repeat.release", pad]));
                calls.push(json!(["drum.release", pad]));
                if self.rec {
                    calls.push(json!(["captureOff", pad_to_note(pad)]));
                }
            }
            Action::SynthOn { note, velocity, .. } => {
                calls.push(json!(["audio.noteOn", note, velocity]))
            }
            Action::SynthOff { note, .. } => calls.push(json!(["audio.noteOff", note])),
            Action::SynthReleaseAll { .. } => calls.push(json!(["audio.releaseAll"])),
            Action::RepeatReleaseAll { .. } => calls.push(json!(["repeat.releaseAll"])),
            Action::PadMute { muted, .. } => self.pad_muted = muted,
            Action::CaptureNoteOn { note, velocity, .. } => {
                if self.rec {
                    calls.push(json!(["captureOn", note, velocity, false]));
                }
            }
            Action::CaptureNoteOff { note, .. } => {
                if self.rec {
                    calls.push(json!(["captureOff", note]));
                }
            }
            Action::Bus(BusEvent::NoteOn {
                note,
                velocity,
                channel,
            }) => calls.push(json!(["bus", {"type": "noteon", "note": note, "velocity": velocity, "channel": channel}])),
            Action::Bus(BusEvent::NoteOff { note, channel }) => {
                calls.push(json!(["bus", {"type": "noteoff", "note": note, "channel": channel}]))
            }
            Action::Bus(BusEvent::AllNotesOff) => {
                calls.push(json!(["bus", {"type": "allnotesoff"}]))
            }
            Action::Knob(k) => calls.push(json!(["knob", knob_target_key(&k.target), k.value01])),
            Action::LegacyCc { cc, value, master } => {
                match self.engine.handle_legacy_cc(cc, value, master) {
                    None => {}
                    Some(CcAction::Volume(db)) => {
                        let name = if master {
                            "onVolumeChange"
                        } else {
                            "audio.setVolume"
                        };
                        calls.push(json!([name, db]));
                    }
                    Some(CcAction::Effect(_, op)) => {
                        let (name, v) = match op {
                            LiveOp::ReverbWet(v) => ("audio.setReverbWet", v),
                            LiveOp::ChorusDepth(v) => ("audio.setChorusDepth", v),
                            LiveOp::FilterCutoff(v) => ("audio.setFilterCutoff", v),
                            LiveOp::FilterResonance(v) => ("audio.setFilterResonance", v),
                            other => panic!("unexpected legacy op {other:?}"),
                        };
                        calls.push(json!([name, v]));
                        calls.push(json!(["effects", self.engine.live_effect_params()]));
                    }
                }
            }
            Action::Sustain(_) | Action::ActiveNotes => {}
        }
    }

    fn step(&mut self, step: &Value) -> Vec<Action> {
        let s = step.as_array().unwrap();
        let op = s[0].as_str().unwrap();
        let n = |i: usize| s[i].as_f64().unwrap();
        let i = |i: usize| s[i].as_i64().unwrap();
        let mut out = Vec::new();
        let r = &mut self.router;
        match op {
            "on" => r.midi_note_on(i(1), n(2), i(3), None, &mut out),
            "off" => r.midi_note_off(i(1), i(2), None, &mut out),
            "cc" => r.handle_cc(i(1) as u8, i(2) as u8, i(3), None, &mut out),
            "kon" => r.note_on(i(1), n(2), &mut out),
            "koff" => r.note_off(i(1), &mut out),
            "press" => r.drum_press(n(1), n(2), &mut out),
            "release" => r.drum_release(n(1), &mut out),
            "hit" => r.drum_hit(n(1), n(2), &mut out),
            "panic" => r.all_notes_off(&mut out),
            "sustain" => r.set_sustain(s[1].as_bool().unwrap(), &mut out),
            "mode" => r.set_routing_mode(RoutingMode::parse(s[1].as_str().unwrap()).unwrap()),
            "mute" => r.set_muted(s[1].as_bool().unwrap(), &mut out),
            "rec" => self.rec = s[1].as_bool().unwrap(),
            "learnPad" => r.mapping.start_pad_learn(match &s[1] {
                Value::String(_) => PadLearn::Sequence,
                v => PadLearn::Pad(v.as_i64().unwrap()),
            }),
            "learnKnob" => r.mapping.start_knob_learn(
                knob_target_from_key(s[1].as_str().unwrap()).unwrap(),
                s[2].as_i64(),
            ),
            "cancelLearn" => r.mapping.cancel_learn(),
            "skipPad" => r.mapping.skip_pad(),
            "prevPad" => r.mapping.previous_pad(),
            other => panic!("unknown step {other}"),
        }
        out
    }

    fn state(&self) -> Value {
        let r = &self.router;
        let mut activity = serde_json::to_value(r.mapping.get_last_activity()).unwrap();
        if let Some(o) = activity.as_object_mut() {
            o.remove("at");
        }
        json!({
            "active": r.active_notes().iter().map(|a| json!([a.note, a.velocity])).collect::<Vec<_>>(),
            "sustain": r.sustain_on(),
            "sounding": r.sounding_notes(),
            "mode": r.routing_mode().as_str(),
            "learn": r.mapping.get_learn_state(),
            "profile": r.mapping.get_profile(),
            "activity": activity,
            "effects": self.engine.live_effect_params(),
        })
    }
}

#[test]
fn routing_core_matches_use_midi_notes() {
    let fx = fixture("midi_routing");
    let scenarios = fx["scenarios"].as_array().unwrap();
    assert!(scenarios.len() >= 35);
    let mut steps_checked = 0;
    let mut calls_checked = 0;
    let mut carried = None;
    for sc in scenarios {
        let name = sc["name"].as_str().unwrap();
        let mut h = Harness::new(
            carried.take(),
            &sc["profile"],
            sc["volumeHandler"].as_bool().unwrap(),
        );
        for (k, (step, want)) in sc["steps"]
            .as_array()
            .unwrap()
            .iter()
            .zip(sc["results"].as_array().unwrap())
            .enumerate()
        {
            let mut calls = Vec::new();
            for a in h.step(step) {
                h.expand(a, &mut calls);
            }
            let calls = Value::Array(calls);
            assert!(
                same(&calls, &want["calls"]),
                "{name} step {k} {step}:\n got  {calls}\n want {}",
                want["calls"]
            );
            let state = h.state();
            for key in [
                "active", "sustain", "sounding", "mode", "learn", "profile", "activity", "effects",
            ] {
                assert!(
                    same(&state[key], &want[key]),
                    "{name} step {k} {step} {key}:\n got  {}\n want {}",
                    state[key],
                    want[key]
                );
            }
            steps_checked += 1;
            calls_checked += want["calls"].as_array().unwrap().len();
        }
        let mut calls = Vec::new();
        let mut out = Vec::new();
        h.router.all_notes_off(&mut out);
        for a in out {
            h.expand(a, &mut calls);
        }
        assert!(
            same(&Value::Array(calls.clone()), &sc["unmount"]),
            "{name} unmount: {calls:?} vs {}",
            sc["unmount"]
        );
        carried = Some(h.router.mapping);
    }
    eprintln!("{steps_checked} steps, {calls_checked} calls identical");
}
