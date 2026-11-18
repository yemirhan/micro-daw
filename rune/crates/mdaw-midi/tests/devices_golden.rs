//! Device selection, hot-plug and held-note release against the real `MidiService`
//! (`rune/parity/ts/midi_devices.golden.ts`). The harness is what `MidiHost` does around
//! [`DeviceSelector`] (no midir here: the port list is scripted): raw bytes from the active input
//! are parsed with [`parse`] and held notes are tracked by [`HeldNotes`]; a detach releases them.

mod common;

use mdaw_midi::devices::{DeviceInfo, DeviceOp, DeviceSelector, HeldNotes};
use mdaw_midi::message::{MidiEvent, parse};
use serde_json::{Value, json};

use common::{fixture, same};

#[derive(Default)]
struct Service {
    enabled: bool,
    inputs: Vec<DeviceInfo>,
    sel: DeviceSelector,
    held: HeldNotes,
    calls: Vec<Value>,
    notified: u32,
}

impl Service {
    fn apply(&mut self, ops: Vec<DeviceOp>) {
        for op in ops {
            if op == DeviceOp::Detach {
                for (note, ch) in self.held.take() {
                    self.calls.push(json!(["off", note, ch]));
                }
            }
        }
    }

    fn change(&mut self) {
        if !self.enabled {
            return;
        }
        let ops = self.sel.device_change(&self.inputs);
        self.apply(ops);
        self.notified += 1;
    }

    fn message(&mut self, dev: &str, bytes: &[u8]) {
        if !self.enabled || self.sel.active() != Some(dev) {
            return;
        }
        match parse(bytes) {
            Some(MidiEvent::NoteOn {
                note,
                velocity,
                channel,
            }) => {
                self.held.note_on(note, channel);
                self.calls.push(json!(["on", note, velocity, channel]));
            }
            Some(MidiEvent::NoteOff { note, channel }) => {
                self.held.note_off(note, channel);
                self.calls.push(json!(["off", note, channel]));
            }
            Some(MidiEvent::ControlChange { cc, value, channel }) => {
                self.calls.push(json!(["cc", cc, value, channel]))
            }
            None => {}
        }
    }

    fn step(&mut self, s: &[Value]) {
        let i = |k: usize| s[k].as_u64().unwrap() as u8;
        match s[0].as_str().unwrap() {
            "enable" => {
                if !self.enabled {
                    self.enabled = true;
                    let ops = self.sel.auto_select(&self.inputs);
                    self.apply(ops);
                }
            }
            "set" => {
                let next: Vec<DeviceInfo> = s[1]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|d| DeviceInfo::new(d[0].as_str().unwrap(), d[1].as_str().unwrap()))
                    .collect();
                let removed = self
                    .inputs
                    .iter()
                    .any(|d| !next.iter().any(|n| n.id == d.id));
                let added = next
                    .iter()
                    .any(|n| !self.inputs.iter().any(|d| d.id == n.id));
                self.inputs = next;
                // webmidi fires one event per change; the host's poll sees each list change
                if removed {
                    self.change();
                }
                if added {
                    self.change();
                }
            }
            "select" => {
                let ops = self.sel.select(s[1].as_str().unwrap(), &self.inputs);
                self.apply(ops);
            }
            "noteOn" => {
                let st = 0x90 | (i(4) - 1);
                self.message(s[1].as_str().unwrap(), &[st, i(2), i(3)]);
            }
            "noteOff" => {
                let st = 0x80 | (i(3) - 1);
                self.message(s[1].as_str().unwrap(), &[st, i(2), 64]);
            }
            "cc" => {
                let st = 0xB0 | (i(4) - 1);
                self.message(s[1].as_str().unwrap(), &[st, i(2), i(3)]);
            }
            "dispose" => {
                let ops = self.sel.dispose();
                self.apply(ops);
                self.enabled = false;
            }
            other => panic!("{other}"),
        }
    }
}

#[test]
fn device_selection_matches_midi_service() {
    let fx = fixture("midi_devices");
    let mut n = 0;
    for sc in fx["scenarios"].as_array().unwrap() {
        let name = sc["name"].as_str().unwrap();
        let mut svc = Service::default();
        for (k, (step, want)) in sc["steps"]
            .as_array()
            .unwrap()
            .iter()
            .zip(sc["results"].as_array().unwrap())
            .enumerate()
        {
            svc.calls.clear();
            svc.notified = 0;
            svc.step(step.as_array().unwrap());
            let devices: Vec<&str> = if svc.enabled {
                svc.inputs.iter().map(|d| d.id.as_str()).collect()
            } else {
                Vec::new()
            };
            let got = json!({
                "calls": svc.calls,
                "notified": svc.notified,
                "active": svc.sel.active(),
                "preferred": svc.sel.preferred(),
                "devices": devices,
            });
            assert!(
                same(&got, want),
                "{name} step {k} {step}:\n got  {got}\n want {want}"
            );
            n += 1;
        }
    }
    assert!(n > 800, "{n} steps");
}
