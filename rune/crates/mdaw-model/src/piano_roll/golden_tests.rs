//! Golden parity against `rune/parity/fixtures/piano_roll.json`, generated from the TypeScript
//! helpers by `rune/parity/ts/piano_roll.golden.ts`.

use std::sync::OnceLock;

use serde_json::Value;

use super::*;
use crate::types::RegionNote;

fn fx() -> &'static Value {
    static FX: OnceLock<Value> = OnceLock::new();
    FX.get_or_init(|| serde_json::from_str(include_str!("../../../../parity/fixtures/piano_roll.json")).unwrap())
}

fn rows(key: &str) -> &'static Vec<Value> {
    fx()[key].as_array().unwrap_or_else(|| panic!("fixture key {key}"))
}

fn num(v: &Value) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().unwrap(),
        Value::String(s) if s == "NaN" => f64::NAN,
        Value::String(s) if s == "Infinity" => f64::INFINITY,
        Value::String(s) if s == "-Infinity" => f64::NEG_INFINITY,
        _ => panic!("not a number: {v}"),
    }
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap_or_else(|| panic!("not a string: {v}"))
}

fn usizes(v: &Value) -> Vec<usize> {
    v.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as usize).collect()
}

fn opt_usizes(v: &Value) -> Option<Vec<usize>> {
    if v.is_null() { None } else { Some(usizes(v)) }
}

fn feq(a: f64, b: f64) -> bool {
    if a.is_nan() || b.is_nan() {
        return a.is_nan() && b.is_nan();
    }
    if a.is_infinite() || b.is_infinite() {
        return a == b;
    }
    (a - b).abs() <= 1e-9 * b.abs().max(1.0)
}

#[track_caller]
fn check(actual: f64, expected: &Value, ctx: &dyn std::fmt::Display) {
    let e = num(expected);
    assert!(feq(actual, e), "{ctx}: got {actual}, expected {e}");
}

fn note_of(v: &Value) -> RegionNote {
    let a = v.as_array().unwrap();
    RegionNote {
        note: num(&a[0]),
        velocity: num(&a[1]),
        start_beat: num(&a[2]),
        duration_beats: num(&a[3]),
        is_drum: num(&a[4]) != 0.0,
    }
}

fn notes_of(v: &Value) -> Vec<RegionNote> {
    v.as_array().unwrap().iter().map(note_of).collect()
}

#[track_caller]
fn check_notes(actual: &[RegionNote], expected: &[RegionNote], ctx: &dyn std::fmt::Display) {
    assert_eq!(actual.len(), expected.len(), "{ctx}: length");
    for (i, (a, e)) in actual.iter().zip(expected).enumerate() {
        let ok = feq(a.note, e.note)
            && feq(a.velocity, e.velocity)
            && feq(a.start_beat, e.start_beat)
            && feq(a.duration_beats, e.duration_beats)
            && a.is_drum == e.is_drum;
        assert!(ok, "{ctx}: note {i}: got {a:?}, expected {e:?}");
    }
}

/// Re-apply a `[index, ...note]` diff onto the input notes.
fn apply_diff(base: &[RegionNote], diff: &Value) -> Vec<RegionNote> {
    let mut out = base.to_vec();
    for d in diff.as_array().unwrap() {
        let a = d.as_array().unwrap();
        let i = a[0].as_u64().unwrap() as usize;
        out[i] = note_of(&Value::Array(a[1..].to_vec()));
    }
    out
}

/// Expected result of an appending edit: input + stored tail.
fn with_tail(base: &[RegionNote], tail: &Value) -> Vec<RegionNote> {
    let mut out = base.to_vec();
    out.extend(notes_of(tail));
    out
}

fn note_sets() -> Vec<Vec<RegionNote>> {
    rows("noteSets").iter().map(notes_of).collect()
}

fn index_sets() -> Vec<Vec<usize>> {
    rows("indexSets").iter().map(usizes).collect()
}

fn layout_of(spec: &Value) -> RowLayout {
    let a = spec.as_array().unwrap();
    match s(&a[0]) {
        "piano" => create_piano_layout(num(&a[1]) as i32, num(&a[2]) as i32),
        "drum" => create_drum_layout(&a[1].as_array().unwrap().iter().map(num).collect::<Vec<_>>()),
        k => panic!("layout kind {k}"),
    }
}

fn layouts() -> Vec<RowLayout> {
    rows("layouts").iter().map(|l| layout_of(&l["spec"])).collect()
}

fn ctx(name: &str, row: &Value) -> String {
    let mut r = row.to_string();
    r.truncate(160);
    format!("{name} {r}")
}

// ---------------------------------------------------------------------------

#[test]
fn golden_constants() {
    let c = &fx()["consts"];
    for (key, opts) in [("snapOptions", &SNAP_OPTIONS), ("noteLengthOptions", &NOTE_LENGTH_OPTIONS)] {
        let exp = c[key].as_array().unwrap();
        assert_eq!(exp.len(), opts.len());
        for (o, e) in opts.iter().zip(exp) {
            assert_eq!(o.id, s(&e[0]));
            assert_eq!(o.label, s(&e[1]));
            check(o.beats, &e[2], &key);
        }
    }
    let drums = c["drumRows"].as_array().unwrap();
    assert_eq!(drums.len(), DRUM_ROWS.len());
    for (d, e) in DRUM_ROWS.iter().zip(drums) {
        check(d.midi_note, &e[0], &"drumRows");
        assert_eq!((d.name, d.full_name, d.color), (s(&e[1]), s(&e[2]), s(&e[3])));
    }
    let n = &c["numbers"];
    let ours: [(&str, f64); 18] = [
        ("BEATS_PER_BAR", BEATS_PER_BAR),
        ("PR_NOTE_MIN", f64::from(PR_NOTE_MIN)),
        ("PR_NOTE_MAX", f64::from(PR_NOTE_MAX)),
        ("PR_NOTE_COUNT", f64::from(PR_NOTE_COUNT)),
        ("PR_DRUM_COUNT", PR_DRUM_COUNT as f64),
        ("DEFAULT_NOTE_VELOCITY", DEFAULT_NOTE_VELOCITY),
        ("DEFAULT_NOTE_DURATION", DEFAULT_NOTE_DURATION),
        ("DEFAULT_DRUM_DURATION", DEFAULT_DRUM_DURATION),
        ("MIN_NOTE_LENGTH", MIN_NOTE_LENGTH),
        ("TICKS_PER_BEAT", TICKS_PER_BEAT),
        ("MIN_VELOCITY", MIN_VELOCITY),
        ("PR_MIN_PX_PER_BEAT", PR_MIN_PX_PER_BEAT),
        ("PR_MAX_PX_PER_BEAT", PR_MAX_PX_PER_BEAT),
        ("PR_DEFAULT_PX_PER_BEAT", PR_DEFAULT_PX_PER_BEAT),
        ("PR_MIN_ROW_HEIGHT", PR_MIN_ROW_HEIGHT),
        ("PR_MAX_ROW_HEIGHT", PR_MAX_ROW_HEIGHT),
        ("PR_DEFAULT_ROW_HEIGHT", PR_DEFAULT_ROW_HEIGHT),
        ("PR_DEFAULT_DRUM_ROW_HEIGHT", PR_DEFAULT_DRUM_ROW_HEIGHT),
    ];
    assert_eq!(n.as_object().unwrap().len(), ours.len());
    for (k, v) in ours {
        assert_eq!(v, num(&n[k]), "{k}");
    }
}

#[test]
fn golden_scalar_math() {
    for r in rows("roundBeat") {
        let x = num(&r[0]);
        let c = ctx("roundBeat", r);
        check(round_beat(x), &r[1], &c);
        check(clamp_velocity(x), &r[2], &c);
        check(velocity_to_midi(x), &r[3], &c);
        check(nudge_step(x), &r[4], &c);
    }
    for r in rows("isMultipleOf") {
        assert_eq!(is_multiple_of(num(&r[0]), num(&r[1])), r[2].as_bool().unwrap(), "{}", ctx("isMultipleOf", r));
    }
    for r in rows("clamp") {
        check(clamp(num(&r[0]), num(&r[1]), num(&r[2])), &r[3], &ctx("clamp", r));
    }
    for r in rows("formatBeats") {
        let b = num(&r[0]);
        assert_eq!(format_beats(b), s(&r[1]), "{}", ctx("formatBeats", r));
        assert_eq!(format_position(b), s(&r[2]), "{}", ctx("formatPosition", r));
    }
    for r in rows("findSnapOption") {
        let b = num(&r[0]);
        assert_eq!(find_snap_option(b, &SNAP_OPTIONS).map(|o| o.id), r[1].as_str(), "{}", ctx("findSnapOption", r));
        assert_eq!(
            find_snap_option(b, &NOTE_LENGTH_OPTIONS).map(|o| o.id),
            r[2].as_str(),
            "{}",
            ctx("findSnapOption", r)
        );
    }
    for r in rows("coords") {
        let (v, sc) = (num(&r[0]), num(&r[1]));
        let c = ctx("coords", r);
        check(beat_to_x(v, sc), &r[2], &c);
        check(x_to_beat(v, sc), &r[3], &c);
        check(row_to_y(v, sc), &r[4], &c);
        check(y_to_row(v, sc), &r[5], &c);
        check(beat_to_grid_x(v, sc), &r[6], &c);
        check(grid_x_to_beat(v, sc), &r[7], &c);
    }
    for r in rows("zoomScroll") {
        check(zoom_scroll(num(&r[0]), num(&r[1]), num(&r[2]), num(&r[3])), &r[4], &ctx("zoomScroll", r));
    }
    for r in rows("zoomFromWheel") {
        let out = zoom_from_wheel(num(&r[0]), num(&r[1]), num(&r[2]), num(&r[3]), num(&r[4]));
        check(out, &r[5], &ctx("zoomFromWheel", r));
    }
    for (i, r) in rows("slider").iter().enumerate() {
        let (t, mn, mx) = (num(&r[0]), num(&r[1]), num(&r[2]));
        let c = ctx("slider", r);
        if i < 24 {
            let z = slider_to_zoom(t, mn, mx);
            check(z, &r[3], &c);
            check(zoom_to_slider(z, mn, mx), &r[4], &c);
        } else {
            check(slider_to_zoom(t / 640.0, mn, mx), &r[3], &c);
            check(zoom_to_slider(t, mn, mx), &r[4], &c);
        }
    }
    for r in rows("velocityBucket") {
        check(velocity_bucket(num(&r[0]), num(&r[1])), &r[2], &ctx("velocityBucket", r));
    }
    for r in rows("followScroll") {
        let out = follow_scroll(num(&r[0]), num(&r[1]), num(&r[2]), num(&r[3]));
        match out {
            None => assert!(r[4].is_null(), "{}: got None", ctx("followScroll", r)),
            Some(v) => check(v, &r[4], &ctx("followScroll", r)),
        }
    }
}

#[test]
fn golden_snapping() {
    for r in rows("snap") {
        let (b, sn, o) = (num(&r[0]), num(&r[1]), num(&r[2]));
        let c = ctx("snap", r);
        check(snap_beat(b, sn, o), &r[3], &c);
        check(snap_floor(b, sn, o), &r[4], &c);
        check(snap_ceil(b, sn, o), &r[5], &c);
    }
    for r in rows("snapLegacy") {
        let b = num(&r[0]);
        check(snap_beat_pr(b, 0.25), &r[1], &ctx("snapBeatPR", r));
        check(snap_beat_floor(b, 1.0 / 3.0), &r[2], &ctx("snapBeatFloor", r));
    }
    for r in rows("snappedMoveDelta") {
        let out = snapped_move_delta(num(&r[0]), num(&r[1]), num(&r[2]), num(&r[3]));
        check(out, &r[4], &ctx("snappedMoveDelta", r));
    }
    for r in rows("drawSpan") {
        let d = draw_span(num(&r[0]), num(&r[1]), num(&r[2]), num(&r[3]));
        let c = ctx("drawSpan", r);
        check(d.start_beat, &r[4], &c);
        check(d.duration_beats, &r[5], &c);
    }
}

#[test]
fn golden_grid_and_ruler() {
    for r in rows("gridSubdivision") {
        check(grid_subdivision(num(&r[0]), num(&r[1]), num(&r[2])), &r[3], &ctx("gridSubdivision", r));
    }
    let mut lines_checked = 0;
    for r in rows("gridLines") {
        let lines = compute_grid_lines(num(&r[0]), num(&r[1]), num(&r[2]), num(&r[3]), num(&r[4]), num(&r[5]));
        let exp = r[6].as_array().unwrap();
        let c = ctx("gridLines", r);
        assert_eq!(lines.len() * 2, exp.len(), "{c}: count");
        for (l, e) in lines.iter().zip(exp.chunks(2)) {
            check(l.beat, &e[0], &c);
            assert_eq!(f64::from(l.level), num(&e[1]), "{c}: level at {}", l.beat);
        }
        lines_checked += lines.len();
    }
    assert!(lines_checked > 10_000);
    for r in rows("ruler") {
        let labels = compute_ruler_labels(num(&r[0]), num(&r[1]), num(&r[2]), num(&r[3]), num(&r[4]));
        let exp = r[5].as_array().unwrap();
        let c = ctx("ruler", r);
        assert_eq!(labels.len(), exp.len(), "{c}: count");
        for (l, e) in labels.iter().zip(exp) {
            check(l.beat, &e[0], &c);
            assert_eq!(l.label, s(&e[1]), "{c}");
            assert_eq!(l.major, e[2].as_bool().unwrap(), "{c}");
        }
    }
}

#[test]
fn golden_layouts() {
    for (l, e) in layouts().iter().zip(rows("layouts")) {
        let c = format!("layout {}", e["spec"]);
        assert_eq!(l.kind.as_str(), s(&e["kind"]), "{c}");
        assert_eq!(l.row_count as f64, num(&e["rowCount"]), "{c}");
        if e["pitches"].is_null() {
            assert!(l.pitches().is_empty());
        } else {
            let p: Vec<f64> = e["pitches"].as_array().unwrap().iter().map(num).collect();
            assert_eq!(l.pitches(), &p[..], "{c}");
        }
        for r in e["p2r"].as_array().unwrap() {
            check(l.pitch_to_row(num(&r[0])), &r[1], &format!("{c} p2r {r}"));
        }
        for r in e["r2p"].as_array().unwrap() {
            let row = num(&r[0]);
            check(l.row_to_pitch(row), &r[1], &format!("{c} r2p {r}"));
            assert_eq!(is_shaded_row(l, row), r[2].as_bool().unwrap(), "{c} shaded {r}");
        }
        for r in e["labels"].as_array().unwrap() {
            let row = num(&r[0]) as i64;
            assert_eq!(l.row_label(row), s(&r[1]), "{c} label {r}");
            assert_eq!(l.row_color(row), r[2].as_str(), "{c} color {r}");
        }
    }
}

#[test]
fn golden_hit_testing_and_selection() {
    let (sets, lays) = (note_sets(), layouts());
    let opts_sel: Vec<(f64, f64, bool, Option<Vec<usize>>)> = rows("hitOpts")
        .iter()
        .map(|o| {
            (num(&o["edgePx"]), num(&o["minWidthPx"]), o["resizable"].as_bool().unwrap(), opt_usizes(&o["selected"]))
        })
        .collect();
    let mut hits = 0;
    for r in rows("hit") {
        let (si, li, oi) =
            (r[0].as_u64().unwrap() as usize, r[1].as_u64().unwrap() as usize, r[2].as_u64().unwrap() as usize);
        let (ep, mw, rz, sel) = &opts_sel[oi];
        let opts = HitTestOptions { edge_px: *ep, min_width_px: *mw, resizable: *rz, selected: sel.as_deref() };
        let h = hit_test_note(&sets[si], num(&r[3]), num(&r[4]), &lays[li], num(&r[5]), num(&r[6]), &opts);
        let got = h.map_or((-1.0, ""), |h| (h.index as f64, h.edge.as_str()));
        assert_eq!(got, (num(&r[7]), s(&r[8])), "{}", ctx("hit", r));
        hits += usize::from(h.is_some());
    }
    assert!(hits > 1000);
    for r in rows("noteVisualWidth") {
        let n = &sets[r[0].as_u64().unwrap() as usize][r[1].as_u64().unwrap() as usize];
        check(note_visual_width(n, num(&r[2]), num(&r[3])), &r[4], &ctx("noteVisualWidth", r));
    }
    for r in rows("notesInRect") {
        let (si, li) = (r[0].as_u64().unwrap() as usize, r[1].as_u64().unwrap() as usize);
        let out = notes_in_rect(&sets[si], num(&r[2]), num(&r[3]), num(&r[4]), num(&r[5]), &lays[li]);
        assert_eq!(out, usizes(&r[6]), "{}", ctx("notesInRect", r));
    }
    for r in rows("click") {
        let out = apply_click_selection(&usizes(&r[0]), r[1].as_u64().unwrap() as usize, r[2].as_bool().unwrap());
        assert_eq!(out, usizes(&r[3]), "{}", ctx("click", r));
    }
}

#[test]
fn golden_move_transpose_copies() {
    let (sets, lays, ixs) = (note_sets(), layouts(), index_sets());
    let u = |v: &Value| v.as_u64().unwrap() as usize;
    for r in rows("clampMove") {
        let d = clamp_move_delta(&sets[u(&r[0])], &ixs[u(&r[2])], num(&r[3]), num(&r[4]), &lays[u(&r[1])]);
        let c = ctx("clampMove", r);
        check(d.d_beat, &r[5], &c);
        check(d.d_row, &r[6], &c);
    }
    for r in rows("move") {
        let notes = &sets[u(&r[0])];
        let out = move_notes(notes, &ixs[u(&r[2])], num(&r[3]), num(&r[4]), &lays[u(&r[1])]);
        check_notes(&out, &apply_diff(notes, &r[5]), &ctx("move", r));
    }
    for r in rows("transpose") {
        let notes = &sets[u(&r[0])];
        let out = transpose_notes(notes, &ixs[u(&r[2])], num(&r[3]), &lays[u(&r[1])]);
        check_notes(&out, &apply_diff(notes, &r[4]), &ctx("transpose", r));
    }
    for r in rows("appendCopies") {
        let notes = &sets[u(&r[0])];
        let e = append_copies(notes, &ixs[u(&r[2])], num(&r[3]), num(&r[4]), Some(&lays[u(&r[1])]));
        let c = ctx("appendCopies", r);
        check_notes(&e.notes, &with_tail(notes, &r[5]), &c);
        assert_eq!(e.selection, usizes(&r[6]), "{c}");
    }
}

#[test]
fn golden_edits() {
    let (sets, ixs) = (note_sets(), index_sets());
    let u = |v: &Value| v.as_u64().unwrap() as usize;
    for r in rows("resize") {
        let notes = &sets[u(&r[0])];
        let edge = if s(&r[2]) == "end" { NoteEdge::End } else { NoteEdge::Start };
        let out = resize_notes(notes, &ixs[u(&r[1])], edge, u(&r[3]), num(&r[4]), num(&r[5]), num(&r[6]));
        check_notes(&out, &apply_diff(notes, &r[7]), &ctx("resize", r));
    }
    for r in rows("changeLengths") {
        let notes = &sets[u(&r[0])];
        let out = change_lengths(notes, &ixs[u(&r[1])], num(&r[2]), num(&r[3]));
        check_notes(&out, &apply_diff(notes, &r[4]), &ctx("changeLengths", r));
    }
    for r in rows("del") {
        let out = delete_notes(&sets[u(&r[0])], &ixs[u(&r[1])]);
        check_notes(&out, &notes_of(&r[2]), &ctx("delete", r));
    }
    for r in rows("selectionSpan") {
        let sp = selection_span(&sets[u(&r[0])], &ixs[u(&r[1])]);
        let c = ctx("selectionSpan", r);
        match sp {
            None => assert!(r[2].is_null(), "{c}"),
            Some(sp) => {
                check(sp.start, &r[2][0], &c);
                check(sp.end, &r[2][1], &c);
            }
        }
    }
    for r in rows("duplicate") {
        let notes = &sets[u(&r[0])];
        let e = duplicate_notes(notes, &ixs[u(&r[1])], num(&r[2]));
        let c = ctx("duplicate", r);
        check_notes(&e.notes, &with_tail(notes, &r[3]), &c);
        assert_eq!(e.selection, usizes(&r[4]), "{c}");
    }
    for r in rows("copy") {
        let out = copy_notes(&sets[u(&r[0])], &ixs[u(&r[1])]);
        check_notes(&out, &notes_of(&r[2]), &ctx("copy", r));
    }
    for r in rows("appendNoLayout") {
        let notes = &sets[u(&r[0])];
        let e = append_copies(notes, &ixs[u(&r[1])], -0.7, 5.0, None);
        let c = ctx("appendNoLayout", r);
        check_notes(&e.notes, &with_tail(notes, &r[2]), &c);
        assert_eq!(e.selection, usizes(&r[3]), "{c}");
    }
    for r in rows("paste") {
        let notes = &sets[u(&r[0])];
        let e = paste_notes(notes, &sets[u(&r[1])], num(&r[2]), r[3].as_bool());
        let c = ctx("paste", r);
        check_notes(&e.notes, &with_tail(notes, &r[4]), &c);
        assert_eq!(e.selection, usizes(&r[5]), "{c}");
    }
    let q_opts: Vec<(Option<Vec<usize>>, bool, f64, f64)> = rows("qOpts")
        .iter()
        .map(|q| (opt_usizes(&q["indices"]), q["ends"].as_bool().unwrap(), num(&q["offset"]), num(&q["strength"])))
        .collect();
    for r in rows("quantize") {
        let notes = &sets[u(&r[0])];
        let (ix, ends, offset, strength) = &q_opts[u(&r[2])];
        let opts = QuantizeOptions { indices: ix.as_deref(), ends: *ends, offset: *offset, strength: *strength };
        let out = quantize_notes(notes, num(&r[1]), &opts);
        check_notes(&out, &apply_diff(notes, &r[3]), &ctx("quantize", r));
    }
}

#[test]
fn golden_velocity() {
    let (sets, ixs) = (note_sets(), index_sets());
    let u = |v: &Value| v.as_u64().unwrap() as usize;
    for r in rows("setVel") {
        let notes = &sets[u(&r[0])];
        let out = set_velocities(notes, &ixs[u(&r[1])], num(&r[2]));
        check_notes(&out, &apply_diff(notes, &r[3]), &ctx("setVel", r));
    }
    for r in rows("offsetVel") {
        let notes = &sets[u(&r[0])];
        let out = offset_velocities(notes, &ixs[u(&r[1])], num(&r[2]));
        check_notes(&out, &apply_diff(notes, &r[3]), &ctx("offsetVel", r));
    }
    for r in rows("ramp") {
        let notes = &sets[u(&r[0])];
        let cand = opt_usizes(&r[1]);
        let out = ramp_velocities(notes, cand.as_deref(), num(&r[2]), num(&r[3]), num(&r[4]), num(&r[5]));
        check_notes(&out, &apply_diff(notes, &r[6]), &ctx("ramp", r));
    }
    for r in rows("notesAtX") {
        let cand = opt_usizes(&r[4]);
        let out = notes_at_x(&sets[u(&r[0])], num(&r[1]), num(&r[2]), num(&r[3]), cand.as_deref());
        assert_eq!(out, usizes(&r[5]), "{}", ctx("notesAtX", r));
    }
}

#[test]
fn golden_view_helpers() {
    let (sets, lays) = (note_sets(), layouts());
    let u = |v: &Value| v.as_u64().unwrap() as usize;
    for r in rows("view") {
        let (notes, layout) = (&sets[u(&r[0])], &lays[u(&r[1])]);
        let c = ctx("view", r);
        match pitch_range(notes) {
            None => assert!(r[2].is_null(), "{c}"),
            Some(p) => {
                check(p.lo, &r[2][0], &c);
                check(p.hi, &r[2][1], &c);
            }
        }
        for q in r[3].as_array().unwrap() {
            check(required_region_length(notes, num(&q[0])), &q[1], &format!("{c} required {q}"));
        }
        for q in r[4].as_array().unwrap() {
            check(center_scroll_top(notes, layout, num(&q[0]), num(&q[1])), &q[2], &format!("{c} center {q}"));
        }
        for q in r[5].as_array().unwrap() {
            let f = fit_view(notes, layout, num(&q[0]), num(&q[1]), num(&q[2]));
            let cc = format!("{c} fit {q}");
            check(f.px_per_beat, &q[3][0], &cc);
            check(f.row_height, &q[3][1], &cc);
            check(f.scroll_left, &q[3][2], &cc);
            check(f.scroll_top, &q[3][3], &cc);
        }
    }
}
