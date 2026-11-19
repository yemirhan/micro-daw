//! Golden parity (rune/parity/fixtures/midi_export.json from rune/parity/ts/midi_export.golden.ts)
//! and the ExportService.test.ts cases.

use mdaw_model::kit::create_factory_kit;
use mdaw_model::types::{
    Arrangement, BeatState, InstrumentType, LoopRecordSettings, PadPlayMode, PadSettings,
    QuantizeGrid, Region, RegionNote, SwingGrid, Track, TrackEffectState, TrackInstrument,
    pad_to_note,
};
use serde_json::Value;

use super::midi::{number_to_bytes, vlq};
use super::*;

fn base64_decode(s: &str) -> Vec<u8> {
    let val = |c: u8| -> u32 {
        match c {
            b'A'..=b'Z' => (c - b'A') as u32,
            b'a'..=b'z' => (c - b'a' + 26) as u32,
            b'0'..=b'9' => (c - b'0' + 52) as u32,
            b'+' => 62,
            b'/' => 63,
            _ => panic!("bad base64"),
        }
    };
    let bytes: Vec<u8> = s.bytes().filter(|c| *c != b'=').collect();
    let mut out = Vec::new();
    for chunk in bytes.chunks(4) {
        let mut acc = 0u32;
        for (i, c) in chunk.iter().enumerate() {
            acc |= val(*c) << (18 - 6 * i);
        }
        let n = chunk.len() * 6 / 8;
        for i in 0..n {
            out.push((acc >> (16 - 8 * i)) as u8);
        }
    }
    out
}

/// Injected pad buffer durations, as in the golden (`DURATIONS`).
fn duration_fn(kind: &str) -> impl FnMut(&PadSettings) -> Option<f64> + '_ {
    move |p: &PadSettings| match kind {
        "null" => None,
        "six" => Some(6.0),
        "zero" => Some(0.0),
        "by-name" => {
            let name_len = mdaw_model::js::len(&p.sample.as_ref()?.name);
            if name_len.is_multiple_of(5) {
                None
            } else {
                Some((name_len % 7) as f64 * 0.37 + 0.05)
            }
        }
        _ => unreachable!(),
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[test]
fn golden_midi_export_and_end_seconds() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../../../parity/fixtures/midi_export.json")).unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert!(cases.len() >= 17);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let arrangement: Arrangement = serde_json::from_value(case["arrangement"].clone())
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        match (&case["midi"], &case["error"]) {
            (Value::String(b64), _) => {
                let expected = base64_decode(b64);
                let got = midi_export(&arrangement).unwrap_or_else(|e| panic!("{name}: {e}"));
                assert_eq!(hex(&got), hex(&expected), "{name}: MIDI bytes");
            }
            (_, Value::String(msg)) => {
                let err = midi_export(&arrangement).unwrap_err();
                assert_eq!(err.to_string(), *msg, "{name}");
            }
            _ => panic!("{name}: no midi / error"),
        }
        for (kind, expected) in case["endSeconds"].as_object().unwrap() {
            let got = arrangement_end_seconds(&arrangement, duration_fn(kind));
            let expected = expected.as_f64().unwrap();
            assert_eq!(got, expected, "{name} / {kind}: end seconds");
        }
    }
}

#[test]
fn vlq_and_number_bytes_match_midi_writer() {
    let enc = |v: u64| {
        let mut out = Vec::new();
        vlq(v, &mut out);
        out
    };
    assert_eq!(enc(0), [0x00]);
    assert_eq!(enc(0x7F), [0x7F]);
    assert_eq!(enc(0x80), [0x81, 0x00]);
    assert_eq!(enc(0x2000), [0xC0, 0x00]);
    assert_eq!(enc(0x3FFF), [0xFF, 0x7F]);
    assert_eq!(enc(0x1F400), [0x87, 0xE8, 0x00]);
    assert_eq!(enc(0x0FFF_FFFF), [0xFF, 0xFF, 0xFF, 0x7F]);
    assert_eq!(number_to_bytes(500_000, 3), [0x07, 0xA1, 0x20]);
    assert_eq!(number_to_bytes(0, 2), [0, 0]);
    assert_eq!(number_to_bytes(0x1234_5678, 3), [0x12, 0x34, 0x56, 0x78]);
}

// --- ExportService.test.ts ---------------------------------------------------------------

const BPM: f64 = 120.0; // 0.5 s per beat

fn drum_track(notes: Vec<RegionNote>, patch: impl FnOnce(&mut Vec<PadSettings>)) -> Track {
    let mut kit = create_factory_kit();
    patch(&mut kit.pads);
    Track {
        id: "d".into(),
        name: "Drums".into(),
        instrument: TrackInstrument {
            kind: InstrumentType::Drums,
            preset_index: 0,
            synth_overrides: None,
            kit: Some(kit),
            extra: Default::default(),
            plugin: None,
        },
        regions: vec![Region {
            id: "r".into(),
            start_beat: 0.0,
            length_beats: 4.0,
            notes,
            color: None,
            name: None,
            audio: None,
            extra: Default::default(),
        }],
        volume: 0.0,
        pan: 0.0,
        muted: false,
        solo: false,
        color: "#f00".into(),
        effects: Some(TrackEffectState::default()),
        automation: None,
        extra: Default::default(),
        input: None,
        plugins: Vec::new(),
    }
}

fn hit(pad: usize, start_beat: f64, duration_beats: f64) -> RegionNote {
    RegionNote {
        note: pad_to_note(pad) as f64,
        velocity: 0.8,
        start_beat,
        duration_beats,
        is_drum: true,
    }
}

fn arrangement_of(tracks: Vec<Track>) -> Arrangement {
    Arrangement {
        id: "a".into(),
        name: "x".into(),
        bpm: BPM,
        length_beats: 16.0,
        tracks,
        loop_markers: None,
        markers: None,
        groups: None,
        beat: None,
        sounds: Vec::new(),
        extra: Default::default(),
    }
}

fn with_loop(arr: &mut Arrangement, swing: f64, quantize: QuantizeGrid, swing_grid: SwingGrid) {
    arr.beat = Some(BeatState {
        active_drum_track_id: Some("d".into()),
        r#loop: LoopRecordSettings {
            swing,
            quantize,
            swing_grid,
            ..Default::default()
        },
        selected_pad: 0,
    });
}

fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 5e-7, "{a} vs {b}");
}

#[test]
fn long_one_shot_outlives_its_note() {
    let arr = arrangement_of(vec![drum_track(vec![hit(0, 2.0, 0.25)], |_| ())]);
    close(arrangement_end_seconds(&arr, |_| Some(6.0)), 1.0 + 6.0);
    close(arrangement_end_seconds(&arr, |_| None), 1.125);
    close(
        export_duration_seconds(&arr, |_| None),
        1.125 + EXPORT_TAIL_SECONDS,
    );
}

#[test]
fn honours_trim_and_pitch() {
    let arr = arrangement_of(vec![drum_track(vec![hit(0, 0.0, 0.25)], |pads| {
        pads[0].start_sec = 1.0;
        pads[0].end_sec = Some(3.0);
        pads[0].pitch = 12.0;
    })]);
    close(arrangement_end_seconds(&arr, |_| Some(10.0)), 1.0);
}

#[test]
fn gate_pads_stop_at_note_end_plus_decay() {
    let arr = arrangement_of(vec![drum_track(vec![hit(0, 0.0, 2.0)], |pads| {
        pads[0].mode = PadPlayMode::Gate;
        pads[0].decay = 0.5;
    })]);
    close(arrangement_end_seconds(&arr, |_| Some(10.0)), 1.5);
    close(arrangement_end_seconds(&arr, |_| Some(1.2)), 1.2);
}

#[test]
fn swing_applies_to_drum_starts() {
    let mut arr = arrangement_of(vec![drum_track(vec![hit(0, 3.75, 0.25)], |_| ())]);
    with_loop(&mut arr, 0.5, QuantizeGrid::Q16, SwingGrid::S16);
    close(
        arrangement_end_seconds(&arr, |_| Some(1.0)),
        (3.75 + 0.125) * 0.5 + 1.0,
    );
}

#[test]
fn delay_tail_and_muted_tracks() {
    let mut t = drum_track(vec![hit(0, 0.0, 0.25)], |_| ());
    {
        let d = &mut t.effects.as_mut().unwrap().delay;
        d.enabled = true;
        d.wet = 0.5;
        d.time = 0.5;
        d.feedback = 0.5;
    }
    close(effects_tail_seconds(&t), 0.5 * (1e-3f64.ln() / 0.5f64.ln()));
    let tail = effects_tail_seconds(&t);
    close(
        arrangement_end_seconds(&arrangement_of(vec![t.clone()]), |_| Some(1.0)),
        1.0 + tail,
    );
    t.muted = true;
    assert_eq!(
        arrangement_end_seconds(&arrangement_of(vec![t]), |_| Some(1.0)),
        0.0
    );
}

#[test]
fn midi_drums_are_written_swung() {
    let mut arr = arrangement_of(vec![drum_track(
        vec![hit(0, 0.0, 0.25), hit(0, 0.25, 0.25), hit(0, 0.5, 0.25)],
        |_| (),
    )]);
    with_loop(&mut arr, 0.5, QuantizeGrid::Q4, SwingGrid::S16);
    let bytes = midi_export(&arr).unwrap();
    // Single track: format 0. Start ticks 0, 48, 64 (the odd 1/16 is 0.125 beat late).
    // Tick-0 note: deferred (on 0, off 32); then on 48 / off 80 and on 64 / off 96 merged.
    let track = &bytes[14 + 8..];
    let notes: Vec<u8> = track
        .windows(3)
        .filter(|w| w[0] == 0x99 || w[0] == 0x89)
        .map(|w| w[0])
        .collect();
    assert_eq!(notes, [0x99, 0x89, 0x99, 0x99, 0x89, 0x89]);
    assert_eq!(&bytes[8..10], &[0, 0]);
}

#[test]
fn nothing_to_export() {
    let mut t = drum_track(vec![hit(0, 0.0, 0.25)], |_| ());
    t.muted = true;
    assert_eq!(
        midi_export(&arrangement_of(vec![t])),
        Err(ExportError::NothingToExport)
    );
    assert_eq!(
        midi_export(&arrangement_of(vec![])),
        Err(ExportError::NothingToExport)
    );
}
