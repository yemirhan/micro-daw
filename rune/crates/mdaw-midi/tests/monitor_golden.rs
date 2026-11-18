//! The MIDI monitor against the real `useMidiMonitor` (`rune/parity/ts/midi_monitor.golden.ts`):
//! formatting of every message kind (malformed ones included), the clock / active-sensing
//! filter, the type filters, the 500-event cap and pause.

mod common;

use mdaw_midi::monitor::{MidiMonitor, MonitorBuffer, MonitorFilter};
use serde_json::{Value, json};

use common::{fixture, same};

#[test]
fn monitor_matches_use_midi_monitor() {
    let fx = fixture("midi_monitor");
    let mut buf = MonitorBuffer::default();
    let mut mon = MidiMonitor::default();
    let bytes = |m: &Value| -> Vec<u8> {
        m.as_array()
            .unwrap()
            .iter()
            .map(|b| b.as_u64().unwrap() as u8)
            .collect()
    };
    for m in fx["messages"].as_array().unwrap() {
        buf.on_raw(&bytes(m), 0.0);
    }
    mon.flush(buf.take());
    let all: Vec<Value> = mon
        .events()
        .into_iter()
        .map(|e| {
            let mut v = serde_json::to_value(e).unwrap();
            v.as_object_mut().unwrap().remove("timestamp");
            v
        })
        .collect();
    assert_eq!(all.len(), fx["events"].as_array().unwrap().len());
    for (g, w) in all.iter().zip(fx["events"].as_array().unwrap()) {
        assert!(same(g, w), "\n got  {g}\n want {w}");
    }
    for (key, f) in [
        ("noteon", MonitorFilter::NoteOn),
        ("noteoff", MonitorFilter::NoteOff),
        ("controlchange", MonitorFilter::ControlChange),
        ("pitchbend", MonitorFilter::PitchBend),
        ("aftertouch", MonitorFilter::Aftertouch),
        ("programchange", MonitorFilter::ProgramChange),
    ] {
        mon.filter = f;
        let ids: Vec<u64> = mon.events().iter().map(|e| e.id).collect();
        assert!(same(&json!(ids), &fx["filtered"][key]), "{key}");
    }
    mon.filter = MonitorFilter::All;
    for i in 0..300u32 {
        buf.on_raw(&[0x90, (i % 128) as u8, (1 + i % 127) as u8], 0.0);
    }
    mon.flush(buf.take());
    for i in 0..300u32 {
        buf.on_raw(&[0x91, (i % 128) as u8, (1 + i % 127) as u8], 0.0);
    }
    mon.flush(buf.take());
    let ids: Vec<u64> = mon.events().iter().map(|e| e.id).collect();
    assert!(same(&json!(ids), &fx["capped"]));
    buf.set_paused(true);
    buf.on_raw(&[0x90, 1, 1], 0.0);
    mon.flush(buf.take());
    assert_eq!(mon.events().len() as u64, fx["pausedLen"].as_u64().unwrap());
}
