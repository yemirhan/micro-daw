use std::cell::Cell;
use std::rc::Rc;

use serde_json::{Value, json};

use super::*;
use crate::types::{KitState, LoopRecordSettings, PadSettings};

use crate::test_support::close;

// --- KnobPickup.test.ts -------------------------------------------------------------------------

#[test]
fn pickup_passes_on_first_message_when_near() {
    assert!(pickup_step(initial_pickup_state(), 0.52, 0.5, PICKUP_THRESHOLD).0);
    assert!(
        !pickup_step(
            initial_pickup_state(),
            0.5 + PICKUP_THRESHOLD * 2.0,
            0.5,
            PICKUP_THRESHOLD
        )
        .0
    );
}

#[test]
fn pickup_on_cross_either_direction_then_follows() {
    let s = pickup_step(initial_pickup_state(), 0.9, 0.5, PICKUP_THRESHOLD).1;
    let r = pickup_step(s, 0.7, 0.5, PICKUP_THRESHOLD);
    assert!(!r.0);
    let r = pickup_step(r.1, 0.3, 0.5, PICKUP_THRESHOLD); // jumped across 0.5
    assert!(r.0);
    let s = r.1;
    assert!(pickup_step(s, 0.1, 0.3, PICKUP_THRESHOLD).0); // follows, even far from the value
    let up = pickup_step(
        pickup_step(initial_pickup_state(), 0.1, 0.5, PICKUP_THRESHOLD).1,
        0.6,
        0.5,
        PICKUP_THRESHOLD,
    );
    assert!(up.0);
}

#[test]
fn pickup_rearms_after_external_change() {
    let mut p = KnobPickup::new();
    assert!(p.check("k", 0.5, 0.5));
    p.applied("k", 0.6);
    assert!(p.check("k", 0.62, 0.6));
    p.applied("k", 0.62);
    // UI/undo moved it to 0.1; the knob at 0.63 must not jump it back.
    assert!(!p.check("k", 0.63, 0.1));
    assert!(p.check("k", 0.08, 0.1));
}

#[test]
fn pickup_reset_forgets() {
    let mut p = KnobPickup::new();
    p.check("a", 0.5, 0.5);
    assert!(p.is_picked_up("a"));
    p.reset(None);
    assert!(!p.is_picked_up("a"));
    assert!(!p.check("a", 0.9, 0.5));
}

// --- KnobRouter.test.ts: knob value ranges ----------------------------------------------------

#[test]
fn maps_pad_params_to_plan_ranges() {
    let pad = PadSettings::default();
    assert_eq!(
        knob_pad_value(PadParam::Pitch, 0.0, &pad, None),
        Some(-24.0)
    );
    assert_eq!(knob_pad_value(PadParam::Pitch, 0.5, &pad, None), Some(0.0));
    assert_eq!(knob_pad_value(PadParam::Pitch, 1.0, &pad, None), Some(24.0));
    assert_eq!(
        knob_pad_value(PadParam::GainDb, 0.0, &pad, None),
        Some(-40.0)
    );
    assert_eq!(knob_pad_value(PadParam::GainDb, 1.0, &pad, None), Some(6.0));
    assert_eq!(knob_pad_value(PadParam::Pan, 0.5, &pad, None), Some(0.0));
}

#[test]
fn maps_decay_and_cutoff_logarithmically() {
    let pad = PadSettings::default();
    assert!(close(
        knob_pad_value(PadParam::Decay, 0.0, &pad, None).unwrap(),
        0.005,
        3
    ));
    assert!(close(
        knob_pad_value(PadParam::Decay, 1.0, &pad, None).unwrap(),
        4.0,
        3
    ));
    assert!(close(
        knob_pad_value(PadParam::Decay, 0.5, &pad, None).unwrap(),
        (0.005f64 * 4.0).sqrt(),
        2
    ));
    assert_eq!(
        knob_pad_value(PadParam::FilterCutoff, 0.0, &pad, None),
        Some(20.0)
    );
    assert_eq!(
        knob_pad_value(PadParam::FilterCutoff, 1.0, &pad, None),
        Some(20000.0)
    );
    assert_eq!(
        knob_pad_value(PadParam::FilterCutoff, 0.5, &pad, None),
        Some(632.0)
    );
}

#[test]
fn maps_trim_start_within_slice_and_needs_buffer_length() {
    let pad = PadSettings::default();
    assert_eq!(knob_pad_value(PadParam::StartSec, 0.5, &pad, None), None);
    assert_eq!(
        knob_pad_value(PadParam::StartSec, 0.0, &pad, Some(2.0)),
        Some(0.0)
    );
    assert!(close(
        knob_pad_value(PadParam::StartSec, 1.0, &pad, Some(2.0)).unwrap(),
        1.99,
        3
    ));
    let trimmed = PadSettings {
        end_sec: Some(1.0),
        ..PadSettings::default()
    };
    assert!(close(
        knob_pad_value(PadParam::StartSec, 1.0, &trimmed, Some(2.0)).unwrap(),
        0.99,
        3
    ));
}

#[test]
fn maps_swing_tempo_and_volume() {
    assert_eq!(knob_swing(0.0), 0.0);
    assert_eq!(knob_swing(1.0), 0.75);
    assert_eq!(knob_bpm(0.0), 60.0);
    assert_eq!(knob_bpm(0.5), 120.0);
    assert_eq!(knob_bpm(1.0), 180.0);
    assert_eq!(knob_volume_db(0.0), -40.0);
    assert_eq!(knob_volume_db(1.0), 0.0);
}

#[test]
fn clamps_out_of_range_input() {
    assert_eq!(knob_bpm(2.0), 180.0);
    assert_eq!(knob_bpm(-1.0), 60.0);
    assert_eq!(knob_bpm(f64::NAN), 60.0);
}

// --- KnobRouter.test.ts: createKnobRouter -------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Call {
    Ensure,
    UpdatePad(String, usize, PadParam, f64),
    Begin(String),
    End,
    SetSwing(f64),
    SetBpm(f64),
    SetTrackVolume(String, f64),
}

struct FakeEngine {
    kit: KitState,
    bpm: f64,
    swing: f64,
    selected_pad: usize,
    track_volume: f64,
    track_id: Option<String>,
    calls: Vec<Call>,
}

fn fake_engine(track_id: Option<&str>) -> FakeEngine {
    FakeEngine {
        kit: KitState {
            id: "k".into(),
            name: "K".into(),
            pads: vec![PadSettings::default(); 16],
            extra: Default::default(),
        },
        bpm: 120.0,
        swing: 0.0,
        selected_pad: 3,
        track_volume: -6.0,
        track_id: track_id.map(str::to_string),
        calls: Vec::new(),
    }
}

impl FakeEngine {
    fn count(&self, f: impl Fn(&Call) -> bool) -> usize {
        self.calls.iter().filter(|c| f(c)).count()
    }
    fn begins(&self) -> usize {
        self.count(|c| matches!(c, Call::Begin(_)))
    }
    fn ends(&self) -> usize {
        self.count(|c| matches!(c, Call::End))
    }
    fn last_update(&self) -> Option<&Call> {
        self.calls
            .iter()
            .rev()
            .find(|c| matches!(c, Call::UpdatePad(..)))
    }
}

fn set_pad_param(pad: &mut PadSettings, param: PadParam, value: f64) {
    match param {
        PadParam::GainDb => pad.gain_db = value,
        PadParam::Pan => pad.pan = value,
        PadParam::Pitch => pad.pitch = value,
        PadParam::Decay => pad.decay = value,
        PadParam::FilterCutoff => pad.filter_cutoff = value,
        PadParam::StartSec => pad.start_sec = value,
    }
}

impl KnobEngine for FakeEngine {
    fn get_active_drum_track_id(&self) -> Option<String> {
        self.track_id.clone()
    }
    fn ensure_drum_track(&mut self) -> String {
        self.calls.push(Call::Ensure);
        "new".into()
    }
    fn get_selected_pad(&self) -> PadIndex {
        self.selected_pad
    }
    fn get_kit(&self, _track_id: &str) -> KitState {
        self.kit.clone()
    }
    fn update_pad(&mut self, track_id: &str, pad: PadIndex, param: PadParam, value: f64) {
        self.calls
            .push(Call::UpdatePad(track_id.into(), pad, param, value));
        set_pad_param(&mut self.kit.pads[pad], param, value);
    }
    fn begin_undo_group(&mut self, label: &str) {
        self.calls.push(Call::Begin(label.into()));
    }
    fn end_undo_group(&mut self) {
        self.calls.push(Call::End);
    }
    fn set_loop_swing(&mut self, swing: f64) {
        self.calls.push(Call::SetSwing(swing));
        self.swing = swing;
    }
    fn get_loop_settings(&self) -> LoopRecordSettings {
        LoopRecordSettings {
            swing: self.swing,
            ..LoopRecordSettings::default()
        }
    }
    fn set_bpm(&mut self, bpm: f64) {
        self.calls.push(Call::SetBpm(bpm));
        self.bpm = bpm;
    }
    fn get_bpm(&self) -> f64 {
        self.bpm
    }
    fn set_track_volume(&mut self, track_id: &str, db: f64) {
        self.calls.push(Call::SetTrackVolume(track_id.into(), db));
        self.track_volume = db;
    }
    fn track_volume(&self, track_id: &str) -> Option<f64> {
        (self.track_id.as_deref() == Some(track_id)).then_some(self.track_volume)
    }
}

type DurationFn = Box<dyn Fn(&PadSettings) -> Option<f64>>;

/// Master volume held in a variable; buffer duration from a closure.
struct FakeDeps {
    master: f64,
    master_sets: Vec<f64>,
    duration: DurationFn,
}

impl KnobDeps for FakeDeps {
    fn set_master_volume(&mut self, db: f64) {
        self.master_sets.push(db);
        self.master = db;
    }
    fn get_master_volume(&self) -> f64 {
        self.master
    }
    fn pad_buffer_duration(&self, pad: &PadSettings) -> Option<f64> {
        (self.duration)(pad)
    }
}

fn deps(duration: Option<f64>) -> FakeDeps {
    FakeDeps {
        master: -12.0,
        master_sets: Vec::new(),
        duration: Box::new(move |_| duration),
    }
}

/// Router on a fake clock (vi.useFakeTimers()).
fn router(idle_ms: Option<f64>) -> (KnobRouter, Rc<Cell<f64>>) {
    let now = Rc::new(Cell::new(0.0));
    let n = now.clone();
    (KnobRouter::new(Box::new(move || n.get()), idle_ms), now)
}

/// `vi.advanceTimersByTime(ms)`: advance the clock and let a due timer fire.
fn advance(r: &mut KnobRouter, e: &mut FakeEngine, now: &Rc<Cell<f64>>, ms: f64) {
    now.set(now.get() + ms);
    r.poll(e);
}

const PITCH: KnobTarget = KnobTarget::Pad {
    param: PadParam::Pitch,
};
const GAIN: KnobTarget = KnobTarget::Pad {
    param: PadParam::GainDb,
};
const DECAY: KnobTarget = KnobTarget::Pad {
    param: PadParam::Decay,
};

#[test]
fn edits_selected_pad_one_twist_one_undo_group() {
    let mut e = fake_engine(Some("t1"));
    let mut d = deps(Some(1.0));
    let (mut r, now) = router(Some(300.0));
    r.handle(&mut e, &mut d, PITCH, 0.5); // picks up at the current value (pitch 0)
    r.handle(&mut e, &mut d, PITCH, 0.6);
    r.handle(&mut e, &mut d, PITCH, 0.7);
    advance(&mut r, &mut e, &now, 200.0);
    r.handle(&mut e, &mut d, PITCH, 0.8);
    assert_eq!(e.begins(), 1);
    assert_eq!(e.ends(), 0);
    assert_eq!(
        e.last_update(),
        Some(&Call::UpdatePad("t1".into(), 3, PadParam::Pitch, 14.0))
    );

    advance(&mut r, &mut e, &now, 300.0);
    assert_eq!(e.ends(), 1);

    r.handle(&mut e, &mut d, PITCH, 0.75);
    assert_eq!(e.begins(), 2);
}

#[test]
fn soft_takeover_far_knob_does_nothing_until_crossing() {
    let mut e = fake_engine(Some("t1"));
    let mut d = deps(Some(1.0));
    let (mut r, _) = router(None);
    r.handle(&mut e, &mut d, PITCH, 0.9);
    r.handle(&mut e, &mut d, PITCH, 0.8);
    assert_eq!(e.last_update(), None);
    r.handle(&mut e, &mut d, PITCH, 0.45); // crossed 0.5
    assert_eq!(
        e.last_update(),
        Some(&Call::UpdatePad("t1".into(), 3, PadParam::Pitch, -2.0))
    );
    r.handle(&mut e, &mut d, PITCH, 0.3);
    assert_eq!(e.kit.pads[3].pitch, (-24.0f64 + 0.3 * 48.0).round());
}

#[test]
fn pickup_resets_when_selected_pad_or_track_changes() {
    let mut e = fake_engine(Some("t1"));
    let mut d = deps(Some(1.0));
    let (mut r, _) = router(None);
    r.handle(&mut e, &mut d, PITCH, 0.5);
    r.handle(&mut e, &mut d, PITCH, 0.75); // pad 3 → +12
    assert_eq!(e.kit.pads[3].pitch, 12.0);
    e.selected_pad = 4;
    r.handle(&mut e, &mut d, PITCH, 0.76);
    assert_eq!(e.kit.pads[4].pitch, 0.0);
    r.handle(&mut e, &mut d, PITCH, 0.4); // crossed 0.5
    assert_eq!(e.kit.pads[4].pitch, -5.0);
}

#[test]
fn value_changed_elsewhere_must_be_picked_up_again() {
    let mut e = fake_engine(Some("t1"));
    let mut d = deps(Some(1.0));
    let (mut r, _) = router(None);
    let v = pad_param_to01(PadParam::GainDb, &e.kit.pads[3], Some(1.0)).unwrap();
    r.handle(&mut e, &mut d, GAIN, v);
    r.handle(&mut e, &mut d, GAIN, 0.9);
    let set = e.kit.pads[3].gain_db;
    e.kit.pads[3].gain_db = -30.0; // on-screen edit
    r.handle(&mut e, &mut d, GAIN, 0.95);
    assert_eq!(e.kit.pads[3].gain_db, -30.0);
    assert_ne!(set, -30.0);
}

#[test]
fn closes_open_step_when_another_knob_moves() {
    let mut e = fake_engine(Some("t1"));
    let mut d = deps(Some(1.0));
    let (mut r, _) = router(None);
    r.handle(&mut e, &mut d, PITCH, 0.5);
    r.handle(&mut e, &mut d, PITCH, 0.6);
    let v = pad_param_to01(PadParam::Decay, &PadSettings::default(), Some(1.0)).unwrap();
    r.handle(&mut e, &mut d, DECAY, v);
    r.handle(&mut e, &mut d, DECAY, 0.2);
    assert_eq!(e.ends(), 1);
    assert_eq!(e.begins(), 2);
    r.dispose(&mut e);
    assert_eq!(e.ends(), 2);
}

#[test]
fn skips_updates_that_do_not_change_the_value() {
    let mut e = fake_engine(Some("t1"));
    let mut d = deps(Some(1.0));
    let (mut r, _) = router(None);
    r.handle(&mut e, &mut d, PITCH, 0.5);
    assert_eq!(e.last_update(), None);
    assert_eq!(e.begins(), 0);
}

#[test]
fn creates_a_drum_track_for_pad_knobs_when_none() {
    let mut e = fake_engine(None);
    let mut d = deps(Some(1.0));
    let (mut r, _) = router(None);
    let v = pad_param_to01(PadParam::GainDb, &PadSettings::default(), Some(1.0)).unwrap();
    r.handle(&mut e, &mut d, GAIN, v);
    r.handle(&mut e, &mut d, GAIN, 1.0);
    assert!(e.count(|c| *c == Call::Ensure) > 0);
    assert!(
        e.calls
            .contains(&Call::UpdatePad("new".into(), 3, PadParam::GainDb, 6.0))
    );
}

#[test]
fn routes_swing_bpm_and_volumes_once_picked_up() {
    let mut e = fake_engine(Some("t1"));
    let mut d = deps(Some(1.0));
    let (mut r, _) = router(None);
    r.handle(&mut e, &mut d, KnobTarget::Swing, 0.0);
    r.handle(&mut e, &mut d, KnobTarget::Swing, 1.0);
    r.handle(&mut e, &mut d, KnobTarget::Bpm, 0.5);
    r.handle(&mut e, &mut d, KnobTarget::Bpm, 0.6);
    r.handle(&mut e, &mut d, KnobTarget::TrackVolume, 0.85); // -6 dB
    r.handle(&mut e, &mut d, KnobTarget::TrackVolume, 0.0);
    r.handle(&mut e, &mut d, KnobTarget::MasterVolume, 0.7); // -12 dB
    r.handle(&mut e, &mut d, KnobTarget::MasterVolume, 1.0);
    assert!(e.calls.contains(&Call::SetSwing(0.75)));
    assert!(e.calls.contains(&Call::SetBpm(132.0)));
    assert!(e.calls.contains(&Call::SetTrackVolume("t1".into(), -40.0)));
    assert!(d.master_sets.contains(&0.0));
}

#[test]
fn bpm_whole_numbers_and_one_undo_step_per_twist() {
    let mut e = fake_engine(Some("t1"));
    let mut d = deps(Some(1.0));
    let (mut r, now) = router(Some(300.0));
    r.handle(&mut e, &mut d, KnobTarget::Bpm, bpm_to01(120.0));
    let mut v = 0.5;
    while v <= 0.6 {
        r.handle(&mut e, &mut d, KnobTarget::Bpm, v);
        v += 0.003;
    }
    let bpms: Vec<f64> = e
        .calls
        .iter()
        .filter_map(|c| {
            if let Call::SetBpm(b) = c {
                Some(*b)
            } else {
                None
            }
        })
        .collect();
    assert!(bpms.iter().all(|b| b.fract() == 0.0));
    assert!(bpms.len() > 5);
    let n = bpms.len();
    r.handle(&mut e, &mut d, KnobTarget::Bpm, 0.6);
    assert_eq!(e.count(|c| matches!(c, Call::SetBpm(_))), n);
    assert_eq!(e.begins(), 1);
    advance(&mut r, &mut e, &now, 300.0);
    assert_eq!(e.ends(), 1);

    r.handle(&mut e, &mut d, KnobTarget::TrackVolume, 0.85);
    r.handle(&mut e, &mut d, KnobTarget::TrackVolume, 0.8);
    r.handle(&mut e, &mut d, KnobTarget::TrackVolume, 0.7);
    assert_eq!(e.begins(), 2);
}

#[test]
fn master_volume_is_not_an_undo_step() {
    let mut e = fake_engine(Some("t1"));
    let mut d = deps(Some(1.0));
    let (mut r, _) = router(None);
    r.handle(&mut e, &mut d, KnobTarget::MasterVolume, 0.7);
    r.handle(&mut e, &mut d, KnobTarget::MasterVolume, 0.5);
    assert_eq!(d.master, -20.0);
    assert_eq!(e.begins(), 0);
}

#[test]
fn ignores_track_volume_knob_without_drum_track() {
    let mut e = fake_engine(None);
    let mut d = deps(None);
    let (mut r, _) = router(None);
    r.handle(&mut e, &mut d, KnobTarget::TrackVolume, 0.5);
    assert_eq!(e.count(|c| matches!(c, Call::SetTrackVolume(..))), 0);
    assert_eq!(e.count(|c| *c == Call::Ensure), 0);
}

// --- golden fixtures ----------------------------------------------------------------------------

fn dec(v: &Value) -> Option<f64> {
    match v {
        Value::Null => None,
        Value::String(s) => Some(match s.as_str() {
            "NaN" => f64::NAN,
            "Infinity" => f64::INFINITY,
            "-Infinity" => f64::NEG_INFINITY,
            other => panic!("bad number {other}"),
        }),
        v => Some(v.as_f64().unwrap()),
    }
}

fn num(v: &Value) -> f64 {
    dec(v).unwrap()
}

fn same(a: Option<f64>, b: Option<f64>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => (a.is_nan() && b.is_nan()) || a == b,
        _ => false,
    }
}

fn param(s: &str) -> PadParam {
    serde_json::from_value(json!(s)).unwrap()
}

fn fixture() -> Value {
    serde_json::from_str(include_str!("../../../parity/fixtures/knobs.json")).unwrap()
}

#[test]
fn golden_math_log_pow_match_v8() {
    let data = fixture();
    let mut n = 0;
    for row in data["mathLog"].as_array().unwrap() {
        let (x, exp) = (num(&row[0]), dec(&row[1]));
        assert!(
            same(Some(js_log(x)), exp),
            "log({x:e}) = {:?}, V8 {exp:?}",
            js_log(x)
        );
        n += 1;
    }
    for row in data["mathPow"].as_array().unwrap() {
        let (x, y, exp) = (num(&row[0]), num(&row[1]), dec(&row[2]));
        assert!(
            same(Some(js_pow(x, y)), exp),
            "pow({x}, {y}) = {:?}, V8 {exp:?}",
            js_pow(x, y)
        );
        n += 1;
    }
    assert!(n > 10000, "{n}");
}

#[test]
fn golden_curves_match_typescript() {
    let data = fixture();
    let base = PadSettings::default();
    let mut n = 0;
    for row in data["forward"].as_array().unwrap() {
        let got = knob_pad_value(
            param(row[0].as_str().unwrap()),
            num(&row[1]),
            &base,
            Some(1.0),
        );
        assert!(same(got, dec(&row[2])), "forward {row} got {got:?}");
        n += 1;
    }
    for row in data["startSec"].as_array().unwrap() {
        let pad = PadSettings {
            end_sec: row[0].as_f64(),
            ..PadSettings::default()
        };
        let got = knob_pad_value(PadParam::StartSec, num(&row[2]), &pad, dec(&row[1]));
        assert!(same(got, dec(&row[3])), "startSec {row} got {got:?}");
        n += 1;
    }
    for row in data["scalar"].as_array().unwrap() {
        let v = num(&row[0]);
        assert!(same(Some(knob_swing(v)), dec(&row[1])), "swing {row}");
        assert!(same(Some(knob_bpm(v)), dec(&row[2])), "bpm {row}");
        assert!(same(Some(knob_volume_db(v)), dec(&row[3])), "volume {row}");
        n += 3;
    }
    for row in data["inverse"].as_array().unwrap() {
        let p = param(row[0].as_str().unwrap());
        let mut pad = PadSettings::default();
        set_pad_param(&mut pad, p, num(&row[1]));
        let got = pad_param_to01(p, &pad, Some(1.0));
        assert!(same(got, dec(&row[2])), "inverse {row} got {got:?}");
        n += 1;
    }
    for row in data["inverseStart"].as_array().unwrap() {
        let pad = PadSettings {
            end_sec: row[0].as_f64(),
            start_sec: num(&row[2]),
            ..PadSettings::default()
        };
        let got = pad_param_to01(PadParam::StartSec, &pad, dec(&row[1]));
        assert!(same(got, dec(&row[3])), "inverseStart {row} got {got:?}");
        n += 1;
    }
    for row in data["inverseScalar"].as_array().unwrap() {
        let x = num(&row[1]);
        let got = match row[0].as_str().unwrap() {
            "swing" => swing_to01(x),
            "bpm" => bpm_to01(x),
            _ => volume_to01(x),
        };
        assert!(
            same(Some(got), dec(&row[2])),
            "inverseScalar {row} got {got}"
        );
        n += 1;
    }
    assert!(n > 20000, "{n}");
}

fn state_from(v: &Value) -> PickupState {
    PickupState {
        last_knob: v["lastKnob"].as_f64(),
        picked_up: v["pickedUp"].as_bool().unwrap(),
        last_set: v["lastSet"].as_f64(),
    }
}

#[test]
fn golden_pickup_matches_typescript() {
    let data = fixture();
    let mut n = 0;
    for seq in data["pickupSteps"].as_array().unwrap() {
        let threshold = seq["threshold"].as_f64().unwrap();
        for step in seq["steps"].as_array().unwrap() {
            let (pass, next) = pickup_step(
                state_from(&step[0]),
                num(&step[1]),
                num(&step[2]),
                threshold,
            );
            assert_eq!(pass, step[3].as_bool().unwrap(), "{step}");
            assert_eq!(next, state_from(&step[4]), "{step}");
            n += 1;
        }
    }
    for ops in data["pickupOps"].as_array().unwrap() {
        let mut p = KnobPickup::new();
        for op in ops.as_array().unwrap() {
            let k = op[1].as_str();
            match op[0].as_str().unwrap() {
                "check" => {
                    let k = k.unwrap();
                    assert_eq!(
                        p.check(k, num(&op[2]), num(&op[3])),
                        op[4].as_bool().unwrap(),
                        "{op}"
                    );
                    assert_eq!(p.is_picked_up(k), op[5].as_bool().unwrap(), "{op}");
                }
                "applied" => {
                    p.applied(k.unwrap(), num(&op[2]));
                    assert_eq!(p.is_picked_up(k.unwrap()), op[3].as_bool().unwrap(), "{op}");
                }
                "external" => continue,
                "reset" => {
                    p.reset(k);
                    assert_eq!(p.is_picked_up(k.unwrap()), op[2].as_bool().unwrap(), "{op}");
                }
                "resetAll" => p.reset(None),
                other => panic!("{other}"),
            }
            n += 1;
        }
    }
    assert!(n > 2000, "{n}");
}

/// Engine for the golden sessions: logs calls as the TS dump does.
struct LogEngine {
    inner: FakeEngine,
    log: Vec<Value>,
}

impl KnobEngine for LogEngine {
    fn get_active_drum_track_id(&self) -> Option<String> {
        self.inner.get_active_drum_track_id()
    }
    fn ensure_drum_track(&mut self) -> String {
        self.log.push(json!(["ensure"]));
        "new".into()
    }
    fn get_selected_pad(&self) -> PadIndex {
        self.inner.selected_pad
    }
    fn get_kit(&self, t: &str) -> KitState {
        self.inner.get_kit(t)
    }
    fn update_pad(&mut self, t: &str, pad: PadIndex, p: PadParam, value: f64) {
        self.log.push(json!(["updatePad", t, pad, p.id(), value]));
        set_pad_param(&mut self.inner.kit.pads[pad], p, value);
    }
    fn begin_undo_group(&mut self, label: &str) {
        self.log.push(json!(["begin", label]));
    }
    fn end_undo_group(&mut self) {
        self.log.push(json!(["end"]));
    }
    fn set_loop_swing(&mut self, swing: f64) {
        self.log.push(json!(["setSwing", swing]));
        self.inner.swing = swing;
    }
    fn get_loop_settings(&self) -> LoopRecordSettings {
        self.inner.get_loop_settings()
    }
    fn set_bpm(&mut self, bpm: f64) {
        self.log.push(json!(["setBpm", bpm]));
        self.inner.bpm = bpm;
    }
    fn get_bpm(&self) -> f64 {
        self.inner.bpm
    }
    fn set_track_volume(&mut self, t: &str, db: f64) {
        self.log.push(json!(["setTrackVolume", t, db]));
        self.inner.track_volume = db;
    }
    fn track_volume(&self, t: &str) -> Option<f64> {
        self.inner.track_volume(t)
    }
}

struct LogDeps {
    master: f64,
    mode: i64,
    log: Rc<std::cell::RefCell<Vec<Value>>>,
}

impl KnobDeps for LogDeps {
    fn set_master_volume(&mut self, db: f64) {
        self.log.borrow_mut().push(json!(["setMaster", db]));
        self.master = db;
    }
    fn get_master_volume(&self) -> f64 {
        self.master
    }
    fn pad_buffer_duration(&self, pad: &PadSettings) -> Option<f64> {
        match self.mode {
            0 => Some(1.0),
            1 => None,
            2 => Some(2.5),
            _ => Some(0.5 + (pad.pitch + 24.0) / 10.0),
        }
    }
}

/// JSON numbers from f64 the way JSON.stringify writes them (integral values without ".0").
fn norm(v: &Value) -> Value {
    match v {
        Value::Number(n) => {
            let f = n.as_f64().unwrap();
            if f.fract() == 0.0 && f.abs() < 1e15 {
                json!(f as i64)
            } else {
                json!(f)
            }
        }
        Value::Array(a) => Value::Array(a.iter().map(norm).collect()),
        other => other.clone(),
    }
}

#[test]
fn golden_router_sessions_match_typescript() {
    let data = fixture();
    let mut n = 0;
    for (si, s) in data["sessions"].as_array().unwrap().iter().enumerate() {
        let track = s["trackStart"].as_str();
        let mut e = LogEngine {
            inner: fake_engine(track),
            log: Vec::new(),
        };
        let shared = Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut d = LogDeps {
            master: -12.0,
            mode: s["durationMode"].as_i64().unwrap(),
            log: shared.clone(),
        };
        let (mut r, now) = {
            let now = Rc::new(Cell::new(0.0));
            let c = now.clone();
            (
                KnobRouter::new(Box::new(move || c.get()), s["idleMs"].as_f64()),
                now,
            )
        };
        for (i, step) in s["ops"].as_array().unwrap().iter().enumerate() {
            let op = &step[0];
            let mut log: Vec<Value> = Vec::new();
            let take = |e: &mut LogEngine, log: &mut Vec<Value>| {
                // Interleave master-volume calls (deps) with engine calls in call order: deps
                // only logs from inside `handle` for master volume, which never touches the
                // engine log, so appending per message keeps the order.
                log.append(&mut e.log);
                log.append(&mut shared.borrow_mut());
            };
            match op[0].as_str().unwrap() {
                "handle" => {
                    let target: KnobTarget = serde_json::from_value(op[1].clone()).unwrap();
                    for p in op[2].as_array().unwrap() {
                        r.handle(&mut e, &mut d, target, p.as_f64().unwrap() / 127.0);
                        take(&mut e, &mut log);
                    }
                }
                "advance" => {
                    now.set(now.get() + op[1].as_f64().unwrap());
                    r.poll(&mut e);
                }
                "selectPad" => e.inner.selected_pad = op[1].as_u64().unwrap() as usize,
                "externalPad" => {
                    let pad = op[1].as_u64().unwrap() as usize;
                    set_pad_param(
                        &mut e.inner.kit.pads[pad],
                        param(op[2].as_str().unwrap()),
                        op[3].as_f64().unwrap(),
                    );
                }
                "externalScalar" => {
                    let v = op[2].as_f64().unwrap();
                    match op[1].as_i64().unwrap() {
                        0 => e.inner.bpm = v,
                        1 => e.inner.swing = v,
                        _ => d.master = v,
                    }
                }
                "setTrack" => e.inner.track_id = op[1].as_str().map(str::to_string),
                "flush" => r.flush(&mut e),
                "resetPickup" => r.reset_pickup(),
                "dispose" => r.dispose(&mut e),
                other => panic!("{other}"),
            }
            take(&mut e, &mut log);
            let got = norm(&Value::Array(log));
            assert_eq!(got, norm(&step[1]), "session {si} step {i} op {op}");
            n += 1;
        }
        for (pi, exp) in s["finalPads"].as_array().unwrap().iter().enumerate() {
            let p = &e.inner.kit.pads[pi];
            let got = [
                p.gain_db,
                p.pan,
                p.pitch,
                p.decay,
                p.filter_cutoff,
                p.start_sec,
            ];
            for (g, x) in got.iter().zip(exp.as_array().unwrap()) {
                assert!(
                    same(Some(*g), dec(x)),
                    "session {si} pad {pi}: {got:?} vs {exp}"
                );
            }
        }
    }
    assert!(n > 5000, "{n}");
}
