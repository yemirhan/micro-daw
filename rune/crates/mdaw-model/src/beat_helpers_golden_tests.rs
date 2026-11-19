//! Golden parity: `rune/parity/ts/beat_helpers.golden.ts` → `rune/parity/fixtures/beat_helpers.json`.

use serde_json::Value;

use super::*;
use crate::types::{LoopMarkers, PadSettings, RegionNote};

const FIXTURE: &str = include_str!("../../../parity/fixtures/beat_helpers.json");

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).expect("fixture parses")
}

/// Number from the fixture (`N()` encodes NaN / ±Infinity / -0 as strings).
pub(crate) fn num(v: &Value) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().unwrap(),
        Value::String(s) => match s.as_str() {
            "NaN" => f64::NAN,
            "Infinity" => f64::INFINITY,
            "-Infinity" => f64::NEG_INFINITY,
            "-0" => -0.0,
            other => panic!("not a number: {other}"),
        },
        other => panic!("not a number: {other}"),
    }
}

pub(crate) fn opt_num(v: &Value) -> Option<f64> {
    if v.is_null() { None } else { Some(num(v)) }
}

fn us(v: &Value) -> usize {
    v.as_u64().unwrap_or_else(|| panic!("not an index: {v}")) as usize
}

fn st(v: &Value) -> &str {
    v.as_str().unwrap_or_else(|| panic!("not a string: {v}"))
}

fn arr(v: &Value) -> &Vec<Value> {
    v.as_array().unwrap_or_else(|| panic!("not an array: {v}"))
}

/// Exact for integers / special values, 1e-9 otherwise.
pub(crate) fn assert_num(actual: f64, expected: f64, ctx: &str) {
    if expected.is_nan() {
        assert!(actual.is_nan(), "{ctx}: expected NaN, got {actual}");
    } else if expected.is_infinite() || expected.fract() == 0.0 {
        assert_eq!(actual, expected, "{ctx}");
    } else {
        assert!(
            (actual - expected).abs() <= 1e-9,
            "{ctx}: {actual} != {expected}"
        );
    }
}

fn rows(v: &Value) -> Vec<Vec<usize>> {
    arr(v)
        .iter()
        .map(|r| arr(r).iter().map(us).collect())
        .collect()
}

// ---------------------------------------------------------------------------
// Test buffers (rune/parity/ts/bufgen.ts)
// ---------------------------------------------------------------------------

pub(crate) fn mulberry32(seed: u32) -> impl FnMut() -> f64 {
    let mut a = seed;
    move || {
        a = a.wrapping_add(0x6d2b79f5);
        let mut t = (a ^ (a >> 15)).wrapping_mul(1 | a);
        t = t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t)) ^ t;
        (t ^ (t >> 14)) as f64 / 4294967296.0
    }
}

pub(crate) fn gen_buffer(spec: &Value) -> SampleBuffer {
    let sr = num(&spec["sr"]);
    let len = us(&spec["len"]);
    let channels = arr(&spec["channels"])
        .iter()
        .map(|ch| {
            let mut data = vec![0f32; len];
            let mut rnd = mulberry32(ch["seed"].as_u64().unwrap() as u32);
            let noise = num(&ch["noise"]);
            if noise > 0.0 {
                for d in data.iter_mut() {
                    *d = ((rnd() * 2.0 - 1.0) * noise) as f32;
                }
            }
            for c in arr(&ch["clicks"]) {
                let i0 = crate::js::round(num(&c["t"]) * sr) as usize;
                let decay = num(&c["decay"]);
                let amp = num(&c["amp"]);
                let mut i = 0usize;
                while (i as f64) < decay && i0 + i < len {
                    data[i0 + i] = (amp * (1.0 - i as f64 / decay)) as f32;
                    i += 1;
                }
            }
            data
        })
        .collect();
    SampleBuffer {
        channels,
        sample_rate: sr,
    }
}

fn pad_settings_from(v: &Value) -> Option<PadSettings> {
    if v.is_null() {
        return None;
    }
    let mut obj = v.as_object().unwrap().clone();
    // The TS cases are partial settings; fill the rest from the defaults.
    let defaults = serde_json::to_value(PadSettings::default()).unwrap();
    for (k, d) in defaults.as_object().unwrap() {
        obj.entry(k.clone()).or_insert(d.clone());
    }
    Some(serde_json::from_value(Value::Object(obj)).expect("pad settings"))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn golden_pad_layout() {
    let f = fixture();
    assert_eq!(pad_rows(PadLayout::FourByFour), rows(&f["padRows"]["4x4"]));
    assert_eq!(pad_rows(PadLayout::TwoByEight), rows(&f["padRows"]["2x8"]));
    let mut n = 2;
    for p in arr(&f["pads"]) {
        let p = arr(p);
        let pad = us(&p[0]);
        assert_eq!(pad_bank(pad).to_string(), st(&p[1]));
        assert_eq!(pad_label(pad), st(&p[2]));
        assert_eq!(pad_color(None, pad), st(&p[3]));
        let red = PadSettings {
            color: Some("red".into()),
            ..PadSettings::default()
        };
        assert_eq!(pad_color(Some(&red), pad), st(&p[4]));
        let empty = PadSettings {
            color: Some(String::new()),
            ..PadSettings::default()
        };
        assert_eq!(pad_color(Some(&empty), pad), st(&p[5]));
        assert_eq!(pad_color(Some(&PadSettings::default()), pad), st(&p[6]));
        assert_eq!(row_for_pad(pad), us(&p[7]));
        assert_eq!(PAD_KEY_HINTS[pad], st(&p[8]));
        n += 6;
    }
    for c in arr(&f["padName"]) {
        let c = arr(c);
        let settings = pad_settings_from(&c[0]);
        assert_eq!(pad_name(settings.as_ref()), st(&c[1]), "padName({})", c[0]);
        n += 1;
    }
    for c in arr(&f["hardwareNoteLabel"]) {
        let c = arr(c);
        assert_eq!(
            hardware_note_label(opt_num(&c[0])),
            st(&c[1]),
            "hardwareNoteLabel({})",
            c[0]
        );
        n += 1;
    }
    for c in arr(&f["velocityFromPointer"]) {
        let c = arr(c);
        let got = velocity_from_pointer(num(&c[0]), num(&c[1]), num(&c[2]));
        assert_num(got, num(&c[3]), &format!("velocityFromPointer{c:?}"));
        n += 1;
    }
    assert!(n > 300);
}

#[test]
fn golden_pad_keys_and_targets() {
    let f = fixture();
    for c in arr(&f["padForKeyEvent"]) {
        let c = arr(c);
        let m = c[2].as_u64().unwrap();
        let e = KeyEventLike {
            key: st(&c[0]).into(),
            code: st(&c[1]).into(),
            meta: m & 1 != 0,
            ctrl: m & 2 != 0,
            alt: m & 4 != 0,
            shift: false,
        };
        let expected = if c[3].is_null() {
            None
        } else {
            Some(us(&c[3]))
        };
        assert_eq!(pad_for_key_event(&e), expected, "padForKeyEvent{c:?}");
    }
    for t in arr(&f["targets"]) {
        let t = arr(t);
        let chain: Option<Vec<DomNode>> = match &t[0] {
            Value::Array(nodes) => Some(
                nodes
                    .iter()
                    .map(|n| {
                        let n = arr(n);
                        DomNode {
                            tag: st(&n[0]).into(),
                            role: n[1].as_str().map(Into::into),
                            contenteditable: n[2].as_str().map(Into::into),
                            keys_scope: n[3].as_str().map(Into::into),
                        }
                    })
                    .collect(),
            ),
            _ => None, // null / window
        };
        let chain = chain.as_deref();
        assert_eq!(
            is_typing_target(chain),
            t[1].as_bool().unwrap(),
            "isTypingTarget({})",
            t[0]
        );
        assert_eq!(
            is_scoped_key_target(chain),
            t[2].as_bool().unwrap(),
            "isScopedKeyTarget({})",
            t[0]
        );
    }
}

#[test]
fn golden_step_grid() {
    let f = fixture();
    for g in arr(&f["stepGrids"]) {
        let g = arr(g);
        let grid = StepGrid::from_label(st(&g[0])).unwrap();
        assert_eq!(step_beats_for(grid), num(&g[1]));
    }
    assert_eq!(arr(&f["stepGrids"]).len(), STEP_GRIDS.len());
    for c in arr(&f["stepCount"]) {
        let c = arr(c);
        assert_eq!(
            step_count(num(&c[0]), num(&c[1])),
            us(&c[2]),
            "stepCount{c:?}"
        );
    }
    for c in arr(&f["stepsPerBeat"]) {
        let c = arr(c);
        assert_eq!(steps_per_beat(num(&c[0])), us(&c[1]), "stepsPerBeat{c:?}");
    }
    for c in arr(&f["patternStepAt"]) {
        let c = arr(c);
        assert_eq!(
            pattern_step_at_index(num(&c[0]), num(&c[1]), us(&c[2])),
            us(&c[3]),
            "patternStepAt{c:?}"
        );
    }
    for c in arr(&f["normVelocity"]) {
        let c = arr(c);
        assert_num(norm_velocity(num(&c[0])), num(&c[1]), "normVelocity");
    }
    for case in arr(&f["buildStepMatrix"]) {
        let notes: Option<Vec<RegionNote>> = if case["region"].is_null() {
            None
        } else {
            Some(serde_json::from_value(case["region"]["notes"].clone()).unwrap())
        };
        let region = notes.as_deref().map(|notes| StepRegion {
            start_beat: num(&case["region"]["startBeat"]),
            notes,
        });
        let m = build_step_matrix(
            region,
            num(&case["patternStart"]),
            num(&case["sb"]),
            us(&case["steps"]),
        );
        assert_eq!(m.len(), us(&case["len"]));
        let cells: Vec<(usize, f64)> = m
            .iter()
            .enumerate()
            .filter(|(_, v)| **v != 0.0)
            .map(|(i, v)| (i, *v as f64))
            .collect();
        let expected: Vec<(usize, f64)> = arr(&case["cells"])
            .iter()
            .map(|c| (us(&c[0]), num(&c[1])))
            .collect();
        assert_eq!(cells, expected, "buildStepMatrix {}", case["patternStart"]);
    }
    for c in arr(&f["playheadStep"]) {
        let c = arr(c);
        let got = playhead_step(num(&c[0]), num(&c[1]), num(&c[2]), num(&c[3]));
        match opt_num(&c[4]) {
            None => assert_eq!(got, None, "playheadStep{c:?}"),
            Some(e) => assert_num(got.expect("some"), e, &format!("playheadStep{c:?}")),
        }
    }
    for c in arr(&f["cellAt"]) {
        let c = arr(c);
        let got = cell_at(num(&c[0]), num(&c[1]), num(&c[2]), num(&c[3]), us(&c[4]));
        let expected = if c[5].is_null() {
            None
        } else {
            Some(StepCell {
                pad: us(&c[5]["pad"]),
                step: us(&c[5]["step"]),
            })
        };
        assert_eq!(got, expected, "cellAt{c:?}");
    }
    for c in arr(&f["paint"]) {
        let c = arr(c);
        let v = num(&c[0]);
        assert_eq!(paint_mode_for(v).label(), st(&c[1]));
        assert_eq!(needs_toggle(PaintMode::Draw, v), c[2].as_bool().unwrap());
        assert_eq!(needs_toggle(PaintMode::Erase, v), c[3].as_bool().unwrap());
    }
    for c in arr(&f["adjustVelocity"]) {
        let c = arr(c);
        let px = opt_num(&c[2]).unwrap_or(ADJUST_VELOCITY_PX_PER_FULL);
        assert_num(
            adjust_velocity(num(&c[0]), num(&c[1]), px),
            num(&c[3]),
            &format!("adjustVelocity{c:?}"),
        );
    }
}

#[test]
fn golden_sample_drops() {
    let f = fixture();
    for c in arr(&f["drops"]) {
        let c = arr(c);
        let payload = st(&c[0]);
        let files: Vec<String> = arr(&c[1]).iter().map(|v| st(v).to_string()).collect();
        let mut types = Vec::new();
        if !payload.is_empty() {
            types.push(SAMPLE_DRAG_MIME.to_string());
        }
        if !files.is_empty() {
            types.push("Files".to_string());
        }
        let dt = DragDataLike {
            types,
            sample_payload: payload.into(),
            file_names: files,
        };
        let empty = |_: &str| String::new();
        let prefix = |f: &str| format!("/drop/{f}");
        let only_wav = |f: &str| {
            if f.ends_with(".wav") {
                format!("/w/{f}")
            } else {
                String::new()
            }
        };
        let get: Option<&dyn Fn(&str) -> String> = match st(&c[2]) {
            "none" => None,
            "empty" => Some(&empty),
            "prefix" => Some(&prefix),
            _ => Some(&only_wav),
        };
        assert_eq!(drag_has_sample(Some(&dt.types)), c[3].as_bool().unwrap());
        let got = read_dropped_sample(Some(&dt), get);
        let expected = if c[4].is_null() {
            None
        } else {
            Some(DroppedSample {
                path: st(&c[4]["path"]).into(),
                name: st(&c[4]["name"]).into(),
            })
        };
        assert_eq!(got, expected, "readDroppedSample{c:?}");
        if let Some(s) = got {
            let expected_sample: crate::types::PadSample =
                serde_json::from_value(c[5].clone()).unwrap();
            assert_eq!(file_sample(&s), expected_sample);
        }
    }
    assert!(read_dropped_sample(None, None).is_none());
    for c in arr(&f["dragHasSample"]) {
        let c = arr(c);
        let types: Option<Vec<String>> = c[0]
            .as_array()
            .map(|a| a.iter().map(|v| st(v).to_string()).collect());
        assert_eq!(drag_has_sample(types.as_deref()), c[1].as_bool().unwrap());
    }
}

fn ranges(v: &Value) -> Vec<SliceRange> {
    arr(v)
        .iter()
        .map(|r| SliceRange {
            start_sec: num(&r["startSec"]),
            end_sec: num(&r["endSec"]),
        })
        .collect()
}

#[test]
fn golden_chop() {
    let f = fixture();
    let input = ranges(&f["chopRangesInput"]);
    for c in arr(&f["planChop"]) {
        let c = arr(c);
        let got = plan_chop(us(&c[0]), &input[..us(&c[1])]);
        let expected: Vec<ChopAssignment> = arr(&c[2])
            .iter()
            .map(|a| ChopAssignment {
                pad: us(&a["pad"]),
                start_sec: num(&a["startSec"]),
                end_sec: num(&a["endSec"]),
            })
            .collect();
        assert_eq!(got, expected);
    }
    let buffers: Vec<SampleBuffer> = arr(&f["bufferSpecs"]).iter().map(gen_buffer).collect();
    let mut n = 0;
    for c in arr(&f["chopRanges"]) {
        let c = arr(c);
        let buf = &buffers[us(&c[0])];
        let trim = ChopTrim {
            start_sec: num(&c[1]["startSec"]),
            end_sec: opt_num(&c[1]["endSec"]),
        };
        let mode = match st(&c[2]["kind"]) {
            "equal" => ChopMode::Equal {
                count: us(&c[2]["count"]),
            },
            _ => ChopMode::Transients {
                sensitivity: num(&c[2]["sensitivity"]),
            },
        };
        let got = chop_ranges(buf, trim, mode, us(&c[3]));
        let expected = ranges(&c[4]);
        assert_eq!(got.len(), expected.len(), "chopRanges{c:?}");
        for (g, e) in got.iter().zip(&expected) {
            assert_num(
                g.start_sec,
                e.start_sec,
                &format!("chopRanges start {:?}", &c[..4]),
            );
            assert_num(
                g.end_sec,
                e.end_sec,
                &format!("chopRanges end {:?}", &c[..4]),
            );
        }
        n += 1;
    }
    assert!(n > 3000);
    for c in arr(&f["missingFix"]) {
        let c = arr(c);
        let pads: Vec<usize> = arr(&c[0]).iter().map(us).collect();
        let fix = if st(&c[1]) == "clear" {
            MissingPadFix::Clear
        } else {
            MissingPadFix::Builtin
        };
        assert_eq!(fix.label(), st(&c[1]));
        let got = plan_missing_pad_fix(&pads, fix);
        let expected = arr(&c[2]);
        assert_eq!(got.len(), expected.len());
        for (g, e) in got.iter().zip(expected) {
            assert_eq!(g.pad, us(&e["pad"]));
            let es: Option<crate::types::PadSample> =
                serde_json::from_value(e["sample"].clone()).unwrap();
            assert_eq!(g.sample, es);
            let ep = if e["patch"].is_null() {
                None
            } else {
                Some(PadTrimReset {
                    start_sec: num(&e["patch"]["startSec"]),
                    end_sec: opt_num(&e["patch"]["endSec"]),
                    reverse: e["patch"]["reverse"].as_bool().unwrap(),
                })
            };
            assert_eq!(g.patch, ep);
        }
    }
}

#[test]
fn golden_log_and_format() {
    let f = fixture();
    for c in arr(&f["log"]) {
        let c = arr(c);
        let (min, max, x) = (num(&c[1]), num(&c[2]), num(&c[3]));
        let got = if st(&c[0]) == "from" {
            from_log01(x, min, max)
        } else {
            to_log01(x, min, max)
        };
        let e = num(&c[4]);
        if e.is_finite() && e.fract() != 0.0 {
            assert!(
                (got - e).abs() <= 1e-9 * e.abs().max(1.0),
                "log{c:?}: {got}"
            );
        } else {
            assert_num(got, e, &format!("log{c:?}"));
        }
    }
    for c in arr(&f["formatSeconds"]) {
        let c = arr(c);
        assert_eq!(
            format_seconds(num(&c[0])),
            st(&c[1]),
            "formatSeconds({})",
            c[0]
        );
    }
    for c in arr(&f["formatHz"]) {
        let c = arr(c);
        assert_eq!(format_hz(num(&c[0])), st(&c[1]), "formatHz({})", c[0]);
    }
}

#[test]
fn golden_loop_mismatch() {
    let f = fixture();
    for c in arr(&f["describeLoopMismatch"]) {
        let c = arr(c);
        let lp = if c[0].is_null() {
            None
        } else {
            Some(LoopMarkers {
                start_beat: num(&c[0]["startBeat"]),
                end_beat: num(&c[0]["endBeat"]),
            })
        };
        let input = LoopMismatchInput {
            r#loop: lp,
            loop_enabled: c[1].as_bool().unwrap(),
            playing: c[2].as_bool().unwrap(),
            pattern_start_beat: num(&c[3]),
            bars: num(&c[4]),
        };
        assert_eq!(
            describe_loop_mismatch(&input).as_deref(),
            c[5].as_str(),
            "describeLoopMismatch{c:?}"
        );
    }
}

#[test]
fn golden_step_keys() {
    let f = fixture();
    assert_eq!(num(&f["velocityKeyStep"]), VELOCITY_KEY_STEP);
    for c in arr(&f["stepKeyAction"]) {
        let c = arr(c);
        let m = c[1].as_u64().unwrap();
        let e = KeyEventLike {
            key: st(&c[0]).into(),
            shift: m & 1 != 0,
            alt: m & 2 != 0,
            meta: m & 4 != 0,
            ctrl: m & 8 != 0,
            ..KeyEventLike::default()
        };
        let cell = StepCell {
            pad: us(&c[2]["pad"]),
            step: us(&c[2]["step"]),
        };
        let got = step_key_action(&e, cell, us(&c[3]), us(&c[4]));
        let r = &c[5];
        let expected = if r.is_null() {
            None
        } else {
            Some(match st(&r["type"]) {
                "move" => StepKeyAction::Move(StepCell {
                    pad: us(&r["cell"]["pad"]),
                    step: us(&r["cell"]["step"]),
                }),
                "toggle" => StepKeyAction::Toggle,
                _ => StepKeyAction::Velocity {
                    delta: num(&r["delta"]),
                },
            })
        };
        assert_eq!(got, expected, "stepKeyAction{c:?}");
    }
    for c in arr(&f["nudgeVelocity"]) {
        let c = arr(c);
        assert_num(
            nudge_velocity(num(&c[0]), num(&c[1])),
            num(&c[2]),
            &format!("nudgeVelocity{c:?}"),
        );
    }
    for c in arr(&f["describeStep"]) {
        let c = arr(c);
        assert_eq!(
            describe_step(st(&c[0]), us(&c[1]), num(&c[2])),
            st(&c[3]),
            "describeStep{c:?}"
        );
    }
}

fn rect_of(call: &Value) -> Rect {
    let a = arr(call);
    Rect {
        x: num(&a[0]),
        y: num(&a[1]),
        w: num(&a[2]),
        h: num(&a[3]),
    }
}

fn assert_rects(got: &[Rect], expected: &[Value], ctx: &str) {
    assert_eq!(got.len(), expected.len(), "{ctx}");
    for (g, e) in got.iter().zip(expected) {
        let e = rect_of(e);
        assert_num(g.x, e.x, ctx);
        assert_num(g.y, e.y, ctx);
        assert_num(g.w, e.w, ctx);
        assert_num(g.h, e.h, ctx);
    }
}

#[test]
fn golden_waveform() {
    let f = fixture();
    let buffers: Vec<SampleBuffer> = arr(&f["bufferSpecs"]).iter().map(gen_buffer).collect();
    for c in arr(&f["peaks"]) {
        let c = arr(c);
        let p = compute_peaks(&buffers[us(&c[0])], num(&c[1]));
        let min: Vec<f64> = arr(&c[2]).iter().map(num).collect();
        let max: Vec<f64> = arr(&c[3]).iter().map(num).collect();
        assert_eq!(
            p.min.iter().map(|v| *v as f64).collect::<Vec<_>>(),
            min,
            "peaks min {:?}",
            &c[..2]
        );
        assert_eq!(
            p.max.iter().map(|v| *v as f64).collect::<Vec<_>>(),
            max,
            "peaks max {:?}",
            &c[..2]
        );
        let bx = Rect {
            x: 3.0,
            y: 2.0,
            w: 120.0,
            h: 40.0,
        };
        assert_rects(
            &peak_rects(&p, bx),
            arr(&c[4]),
            &format!("drawPeaks {:?}", &c[..2]),
        );
    }
    let flat = &f["peakRectsFlat"];
    let peaks = Peaks {
        min: arr(&flat["peaks"]["min"])
            .iter()
            .map(|v| num(v) as f32)
            .collect(),
        max: arr(&flat["peaks"]["max"])
            .iter()
            .map(|v| num(v) as f32)
            .collect(),
    };
    for r in arr(&flat["rects"]) {
        let r = arr(r);
        let bx = Rect {
            x: num(&r[0]["x"]),
            y: num(&r[0]["y"]),
            w: num(&r[0]["w"]),
            h: num(&r[0]["h"]),
        };
        assert_rects(&peak_rects(&peaks, bx), arr(&r[1]), "drawPeaks flat");
    }
    for c in arr(&f["prepareCanvas"]) {
        let c = arr(c);
        let b = canvas_backing(num(&c[1]), num(&c[2]), Some(num(&c[0])));
        assert_eq!(
            (b.width, b.height, b.dpr),
            (num(&c[3]), num(&c[4]), num(&c[5])),
            "prepareCanvas{c:?}"
        );
    }
}

#[test]
fn golden_tsx_helpers() {
    let f = fixture();
    for c in arr(&f["swingGridLabel"]) {
        let c = arr(c);
        let g = if st(&c[0]) == "1/8" {
            crate::types::SwingGrid::S8
        } else {
            crate::types::SwingGrid::S16
        };
        assert_eq!(swing_grid_label(g), st(&c[1]));
    }
    for c in arr(&f["pendingLoadText"]) {
        let c = arr(c);
        let t = pending_load_text(st(&c[0]), c[1].as_bool().unwrap());
        assert_eq!(t.prefix, st(&c[2]["prefix"]));
        assert_eq!(t.name, st(&c[2]["name"]));
        assert_eq!(t.text, st(&c[2]["text"]));
        assert_eq!(t.title, st(&c[2]["title"]));
    }
}
