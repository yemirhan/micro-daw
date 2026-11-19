//! Port of `src/utils/chordDetection.test.ts` + golden parity (`rune/parity/ts/notes.golden.ts`).

use serde_json::Value;

use super::*;

fn display(notes: &[i32]) -> Option<String> {
    detect_chord(notes).map(|c| c.display)
}

#[test]
fn returns_none_for_fewer_than_two_pitch_classes() {
    assert_eq!(detect_chord(&[]), None);
    assert_eq!(detect_chord(&[60]), None);
    assert_eq!(detect_chord(&[60, 72]), None);
}

#[test]
fn detects_triads_regardless_of_voicing() {
    assert_eq!(display(&[60, 64, 67]).as_deref(), Some("C"));
    assert_eq!(display(&[64, 67, 72]).as_deref(), Some("C"));
    assert_eq!(display(&[57, 60, 64]).as_deref(), Some("Am"));
    assert_eq!(display(&[59, 62, 65]).as_deref(), Some("Bdim"));
}

#[test]
fn detects_seventh_chords() {
    assert_eq!(display(&[67, 71, 74, 77]).as_deref(), Some("G7"));
    assert_eq!(display(&[60, 64, 67, 71]).as_deref(), Some("Cmaj7"));
}

const FIXTURE: &str = include_str!("../../../parity/fixtures/notes.json");

#[test]
fn golden_detect_chord() {
    let f: Value = serde_json::from_str(FIXTURE).unwrap();
    let cases = f["detectChord"].as_array().unwrap();
    assert!(cases.len() > 1800);
    for c in cases {
        let notes: Vec<i32> = c[0]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_i64().unwrap() as i32)
            .collect();
        let mut e = c[1].clone();
        // JSON.stringify drops `root: undefined` (NOTE_NAMES[negative pitch class]).
        if let Some(o) = e.as_object_mut() {
            o.entry("root").or_insert(Value::String("undefined".into()));
        }
        let expected: Option<ChordInfo> = serde_json::from_value(e).unwrap();
        assert_eq!(detect_chord(&notes), expected, "detectChord({})", c[0]);
    }
}
