//! Port of `src/utils/noteHelpers.test.ts` + golden parity (`rune/parity/ts/notes.golden.ts`).

use serde_json::Value;

use super::*;
use crate::beat_helpers::BufferLike;
use crate::beat_helpers::golden_tests::{assert_num, gen_buffer, num};

#[test]
fn converts_midi_numbers_to_note_names() {
    assert_eq!(midi_to_note_name(60.0), "C4");
    assert_eq!(midi_to_note_name(69.0), "A4");
    assert_eq!(midi_to_note_name(61.0), "C#4");
    assert_eq!(midi_to_note_name(21.0), "A0");
}

#[test]
fn round_trips_note_names() {
    for midi in 12..=119 {
        assert_eq!(note_name_to_midi(&midi_to_note_name(midi as f64)), midi);
    }
    assert_eq!(note_name_to_midi("H4"), -1);
}

#[test]
fn detects_black_keys() {
    let got: Vec<bool> = [60, 61, 62, 63, 64, 65, 66].into_iter().map(is_black_key).collect();
    assert_eq!(got, vec![false, true, false, true, false, false, true]);
}

#[test]
fn computes_equal_tempered_frequencies() {
    assert!((midi_to_frequency(69.0) - 440.0).abs() < 0.005);
    assert!((midi_to_frequency(81.0) - 880.0).abs() < 0.005);
    assert!((midi_to_frequency(60.0) - 261.626).abs() < 0.005);
}

#[test]
fn normalizes_and_clamps_velocity() {
    assert_eq!(velocity_to_normalized(127.0), 1.0);
    assert_eq!(velocity_to_normalized(0.0), 0.0);
    assert_eq!(velocity_to_normalized(200.0), 1.0);
}

const FIXTURE: &str = include_str!("../../../parity/fixtures/notes.json");

fn rows(f: &Value, k: &str) -> Vec<Vec<Value>> {
    f[k].as_array().unwrap().iter().map(|r| r.as_array().unwrap().clone()).collect()
}

#[test]
fn golden_notes() {
    let f: Value = serde_json::from_str(FIXTURE).unwrap();
    for c in rows(&f, "midiToNoteName") {
        assert_eq!(midi_to_note_name(num(&c[0])), c[1].as_str().unwrap(), "midiToNoteName({})", c[0]);
    }
    for c in rows(&f, "noteNameToMidi") {
        assert_eq!(
            note_name_to_midi(c[0].as_str().unwrap()) as i64,
            c[1].as_i64().unwrap(),
            "noteNameToMidi({})",
            c[0]
        );
    }
    for c in rows(&f, "isBlackKey") {
        let m = num(&c[0]);
        if m.abs() > i32::MAX as f64 {
            continue; // only ±1.2e22 (JS: % 12 → 0 / -0 → not black)
        }
        assert_eq!(is_black_key(m as i32), c[1].as_bool().unwrap(), "isBlackKey({})", c[0]);
    }
    for c in rows(&f, "midiToFrequency") {
        let e = num(&c[1]);
        let got = midi_to_frequency(num(&c[0]));
        assert!((got - e).abs() <= 1e-9 * e.abs().max(1.0), "midiToFrequency({}): {got} != {e}", c[0]);
    }
    for c in rows(&f, "velocityToNormalized") {
        assert_num(velocity_to_normalized(num(&c[0])), num(&c[1]), "velocityToNormalized");
    }
    let buffers: Vec<_> = f["bufferSpecs"].as_array().unwrap().iter().map(gen_buffer).collect();
    for c in rows(&f, "computePeaksFromBuffer") {
        let b = &buffers[c[0].as_u64().unwrap() as usize];
        let chans: Vec<&[f32]> = (0..b.number_of_channels()).map(|i| b.channel_data(i)).collect();
        let got: Vec<f64> = compute_peaks_from_buffer(&chans, b.length(), c[1].as_u64().unwrap() as usize)
            .into_iter()
            .map(f64::from)
            .collect();
        let expected: Vec<f64> = c[2].as_array().unwrap().iter().map(num).collect();
        assert_eq!(got, expected, "computePeaksFromBuffer({}, {})", c[0], c[1]);
    }
}
