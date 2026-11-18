//! Hardware knobs → app parameters, and soft takeover ("pickup").
//!
//! Ported from `src/services/KnobRouter.ts` (value curves, inverse curves, the router with its
//! undo-gesture grouping) and `src/services/KnobPickup.ts` (soft takeover).
//!
//! Differences forced by Rust (behaviour is the same):
//! - The TS router calls `setTimeout(flush, idleMs)`; here the router holds an injected clock
//!   (milliseconds) and a deadline. An expired gesture is closed at the start of the next
//!   `handle`/`touch` (where the JS timer would already have fired) and by [`KnobRouter::poll`],
//!   which the app calls periodically (e.g. every UI frame) to close idle gestures.
//! - The engine is not owned by the router: every call takes `&mut impl KnobEngine` and
//!   `&mut impl KnobDeps` (master volume + buffer durations, the rest of `KnobRouterDeps`).

use std::collections::HashMap;

use crate::js::{log as js_log, max as js_max, min as js_min, pow as js_pow, round as js_round};
use crate::types::{KitState, KnobTarget, LoopRecordSettings, PadIndex, PadParam, PadSettings};

// ---------------------------------------------------------------------------------------------
// KnobPickup.ts
// ---------------------------------------------------------------------------------------------

/// ~6 of 127 CC steps.
pub const PICKUP_THRESHOLD: f64 = 0.05;
/// A parameter further than this from what the knob last set was changed elsewhere.
const EXTERNAL_CHANGE_EPS: f64 = 0.02;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PickupState {
    /// Last knob position seen (None before the first message).
    pub last_knob: Option<f64>,
    /// The knob is in control.
    pub picked_up: bool,
    /// Parameter value (0..1) right after the knob last set it.
    pub last_set: Option<f64>,
}

pub fn initial_pickup_state() -> PickupState {
    PickupState {
        last_knob: None,
        picked_up: false,
        last_set: None,
    }
}

/// Pure: should a knob at `knob` move a parameter currently at `current`?
/// Returns the decision and the next state.
pub fn pickup_step(state: PickupState, knob: f64, current: f64, threshold: f64) -> (bool, PickupState) {
    let mut picked_up = state.picked_up;
    // Moved elsewhere (UI, undo, load) since the knob last set it: pick up again.
    if picked_up
        && let Some(last_set) = state.last_set
        && (current - last_set).abs() > EXTERNAL_CHANGE_EPS
    {
        picked_up = false;
    }
    if !picked_up {
        let close = (knob - current).abs() <= threshold;
        let crossed = match state.last_knob {
            Some(last) => (last <= current && knob >= current) || (last >= current && knob <= current),
            None => false,
        };
        picked_up = close || crossed;
    }
    (
        picked_up,
        PickupState {
            last_knob: Some(knob),
            picked_up,
            last_set: if picked_up { state.last_set } else { None },
        },
    )
}

/// Pickup state per knob key (e.g. a CC number or a target+context id).
#[derive(Debug, Clone)]
pub struct KnobPickup {
    states: HashMap<String, PickupState>,
    threshold: f64,
}

impl Default for KnobPickup {
    fn default() -> Self {
        Self::new()
    }
}

impl KnobPickup {
    pub fn new() -> Self {
        Self::with_threshold(PICKUP_THRESHOLD)
    }

    pub fn with_threshold(threshold: f64) -> Self {
        Self {
            states: HashMap::new(),
            threshold,
        }
    }

    /// True when the knob `key` at `knob` may set the parameter now at `current`.
    pub fn check(&mut self, key: &str, knob: f64, current: f64) -> bool {
        let prev = self.states.get(key).copied().unwrap_or_else(initial_pickup_state);
        let (pass, state) = pickup_step(prev, knob, current, self.threshold);
        self.states.insert(key.to_string(), state);
        pass
    }

    /// Record the value (0..1) the knob just set, so later outside edits are detected.
    pub fn applied(&mut self, key: &str, value: f64) {
        if let Some(s) = self.states.get_mut(key) {
            s.last_set = Some(value);
        }
    }

    pub fn is_picked_up(&self, key: &str) -> bool {
        self.states.get(key).is_some_and(|s| s.picked_up)
    }

    /// Forget one knob (or all, with `None`): it has to be picked up again.
    pub fn reset(&mut self, key: Option<&str>) {
        match key {
            None => self.states.clear(),
            Some(k) => {
                self.states.remove(k);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// KnobRouter.ts: value curves
// ---------------------------------------------------------------------------------------------

/// `MIN_VOLUME` / `MAX_VOLUME` (src/utils/constants.ts).
const MIN_VOLUME: f64 = -40.0;
const MAX_VOLUME: f64 = 0.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KnobRange {
    pub min: f64,
    pub max: f64,
}

/// Value ranges for knob targets (value01 = 0..1 from the controller). `KNOB_RANGES`.
pub struct KnobRanges {
    pub pitch: KnobRange,
    pub decay: KnobRange,
    pub filter_cutoff: KnobRange,
    pub gain_db: KnobRange,
    pub pan: KnobRange,
    pub swing: KnobRange,
    pub bpm: KnobRange,
    pub volume: KnobRange,
}

pub const KNOB_RANGES: KnobRanges = KnobRanges {
    pitch: KnobRange { min: -24.0, max: 24.0 },
    decay: KnobRange { min: 0.005, max: 4.0 },
    filter_cutoff: KnobRange {
        min: 20.0,
        max: 20000.0,
    },
    gain_db: KnobRange { min: -40.0, max: 6.0 },
    pan: KnobRange { min: -1.0, max: 1.0 },
    swing: KnobRange { min: 0.0, max: 0.75 },
    bpm: KnobRange { min: 60.0, max: 180.0 },
    volume: KnobRange {
        min: MIN_VOLUME,
        max: MAX_VOLUME,
    },
};

fn clamp01(v: f64) -> f64 {
    if v.is_finite() {
        js_min(1.0, js_max(0.0, v))
    } else {
        0.0
    }
}
fn lin(v: f64, min: f64, max: f64) -> f64 {
    min + clamp01(v) * (max - min)
}
fn log(v: f64, min: f64, max: f64) -> f64 {
    min * js_pow(max / min, clamp01(v))
}
fn round_step(v: f64, step: f64) -> f64 {
    js_round(v / step) * step
}

/// JS `!x || !(x > 0)` for `number | null`: the buffer length is unknown / unusable.
fn bad_duration(d: Option<f64>) -> Option<f64> {
    match d {
        Some(d) if d > 0.0 => Some(d),
        _ => None,
    }
}

/// Pad parameter value for a knob position. `buffer_duration` (seconds) is needed for
/// `startSec` (0 .. the trim end); without it the knob does nothing (None).
pub fn knob_pad_value(param: PadParam, value01: f64, pad: &PadSettings, buffer_duration: Option<f64>) -> Option<f64> {
    let r = &KNOB_RANGES;
    Some(match param {
        PadParam::Pitch => js_round(lin(value01, r.pitch.min, r.pitch.max)),
        PadParam::Decay => round_step(log(value01, r.decay.min, r.decay.max), 0.001),
        PadParam::FilterCutoff => js_round(log(value01, r.filter_cutoff.min, r.filter_cutoff.max)),
        PadParam::GainDb => round_step(lin(value01, r.gain_db.min, r.gain_db.max), 0.1),
        PadParam::Pan => round_step(lin(value01, r.pan.min, r.pan.max), 0.01),
        PadParam::StartSec => {
            let duration = bad_duration(buffer_duration)?;
            let end = js_min(duration, pad.end_sec.unwrap_or(duration));
            // Keep at least 10 ms of slice.
            js_max(0.0, round_step(clamp01(value01) * js_max(0.0, end - 0.01), 0.001))
        }
    })
}

pub fn knob_swing(value01: f64) -> f64 {
    round_step(lin(value01, KNOB_RANGES.swing.min, KNOB_RANGES.swing.max), 0.01)
}
pub fn knob_bpm(value01: f64) -> f64 {
    js_round(lin(value01, KNOB_RANGES.bpm.min, KNOB_RANGES.bpm.max))
}
pub fn knob_volume_db(value01: f64) -> f64 {
    round_step(lin(value01, KNOB_RANGES.volume.min, KNOB_RANGES.volume.max), 0.5)
}

fn unlin(v: f64, min: f64, max: f64) -> f64 {
    clamp01((v - min) / (max - min))
}
fn unlog(v: f64, min: f64, max: f64) -> f64 {
    if v > 0.0 {
        clamp01(js_log(v / min) / js_log(max / min))
    } else {
        0.0
    }
}

/// The current numeric value of a pad parameter (`settings[param]`).
pub fn pad_param_value(pad: &PadSettings, param: PadParam) -> f64 {
    match param {
        PadParam::GainDb => pad.gain_db,
        PadParam::Pan => pad.pan,
        PadParam::Pitch => pad.pitch,
        PadParam::Decay => pad.decay,
        PadParam::FilterCutoff => pad.filter_cutoff,
        PadParam::StartSec => pad.start_sec,
    }
}

/// Inverse of [`knob_pad_value`]: where the knob would sit for the pad's current value (0..1),
/// None if unknown.
pub fn pad_param_to01(param: PadParam, pad: &PadSettings, buffer_duration: Option<f64>) -> Option<f64> {
    let r = &KNOB_RANGES;
    Some(match param {
        PadParam::Pitch => unlin(pad.pitch, r.pitch.min, r.pitch.max),
        PadParam::Decay => unlog(pad.decay, r.decay.min, r.decay.max),
        PadParam::FilterCutoff => unlog(pad.filter_cutoff, r.filter_cutoff.min, r.filter_cutoff.max),
        PadParam::GainDb => unlin(pad.gain_db, r.gain_db.min, r.gain_db.max),
        PadParam::Pan => unlin(pad.pan, r.pan.min, r.pan.max),
        PadParam::StartSec => {
            let duration = bad_duration(buffer_duration)?;
            let end = js_min(duration, pad.end_sec.unwrap_or(duration));
            let span = js_max(0.0, end - 0.01);
            // `(pad.startSec || 0)`: NaN / 0 → 0.
            let start = if pad.start_sec.is_nan() { 0.0 } else { pad.start_sec };
            if span > 0.0 { clamp01(start / span) } else { 0.0 }
        }
    })
}

pub fn swing_to01(swing: f64) -> f64 {
    unlin(swing, KNOB_RANGES.swing.min, KNOB_RANGES.swing.max)
}
pub fn bpm_to01(bpm: f64) -> f64 {
    unlin(bpm, KNOB_RANGES.bpm.min, KNOB_RANGES.bpm.max)
}
pub fn volume_to01(db: f64) -> f64 {
    unlin(db, KNOB_RANGES.volume.min, KNOB_RANGES.volume.max)
}

/// Undo labels of the pad parameters (`PARAM_LABELS`).
pub fn pad_param_undo_label(param: PadParam) -> &'static str {
    match param {
        PadParam::GainDb => "Pad Gain",
        PadParam::Pan => "Pad Pan",
        PadParam::Pitch => "Pad Pitch",
        PadParam::Decay => "Pad Decay",
        PadParam::FilterCutoff => "Pad Filter",
        PadParam::StartSec => "Pad Start",
    }
}

// ---------------------------------------------------------------------------------------------
// KnobRouter.ts: the router
// ---------------------------------------------------------------------------------------------

/// The parts of the arrangement engine the router uses (`KnobEngine = Pick<…>`).
pub trait KnobEngine {
    fn get_active_drum_track_id(&self) -> Option<String>;
    fn ensure_drum_track(&mut self) -> String;
    fn get_selected_pad(&self) -> PadIndex;
    fn get_kit(&self, track_id: &str) -> KitState;
    /// `updatePad(trackId, pad, { [param]: value })`.
    fn update_pad(&mut self, track_id: &str, pad: PadIndex, param: PadParam, value: f64);
    fn begin_undo_group(&mut self, label: &str);
    fn end_undo_group(&mut self);
    /// `setLoopSettings({ swing })` (the only patch the router sends).
    fn set_loop_swing(&mut self, swing: f64);
    fn get_loop_settings(&self) -> LoopRecordSettings;
    fn set_bpm(&mut self, bpm: f64);
    fn get_bpm(&self) -> f64;
    fn set_track_volume(&mut self, track_id: &str, db: f64);
    /// `getTracks().find((t) => t.id === trackId)?.volume`.
    fn track_volume(&self, track_id: &str) -> Option<f64>;
}

/// The non-engine parts of `KnobRouterDeps`.
pub trait KnobDeps {
    fn set_master_volume(&mut self, db: f64);
    fn get_master_volume(&self) -> f64;
    /// Loaded buffer length of a pad's sample, if known.
    fn pad_buffer_duration(&self, pad: &PadSettings) -> Option<f64>;
}

pub const KNOB_GESTURE_IDLE_MS: f64 = 400.0;

struct Gesture {
    key: String,
    deadline: f64,
}

pub struct KnobRouter {
    idle_ms: f64,
    clock: Box<dyn Fn() -> f64>,
    gesture: Option<Gesture>,
    pickup: KnobPickup,
    /// Selected pad / active track the pickups were made for.
    pickup_context: Option<String>,
}

impl KnobRouter {
    /// `createKnobRouter`. `clock` returns milliseconds (monotonic); `idle_ms` defaults to
    /// [`KNOB_GESTURE_IDLE_MS`].
    pub fn new(clock: Box<dyn Fn() -> f64>, idle_ms: Option<f64>) -> Self {
        Self {
            idle_ms: idle_ms.unwrap_or(KNOB_GESTURE_IDLE_MS),
            clock,
            gesture: None,
            pickup: KnobPickup::new(),
            pickup_context: None,
        }
    }

    /// Fire the idle timer if it is due (what `setTimeout(flush, idleMs)` would have done by now).
    pub fn poll(&mut self, engine: &mut impl KnobEngine) {
        if let Some(g) = &self.gesture
            && (self.clock)() >= g.deadline
        {
            self.flush(engine);
        }
    }

    /// Close the open undo step now.
    pub fn flush(&mut self, engine: &mut impl KnobEngine) {
        if self.gesture.take().is_none() {
            return;
        }
        engine.end_undo_group();
    }

    /// Every knob has to be picked up again (soft takeover).
    pub fn reset_pickup(&mut self) {
        self.pickup.reset(None);
    }

    pub fn dispose(&mut self, engine: &mut impl KnobEngine) {
        self.flush(engine);
    }

    /// True while a gesture's undo group is open.
    pub fn has_open_gesture(&self) -> bool {
        self.gesture.is_some()
    }

    /// One undo group per (track, pad, param) gesture.
    fn touch(&mut self, engine: &mut impl KnobEngine, key: &str, label: &str) {
        if self.gesture.as_ref().is_some_and(|g| g.key != key) {
            self.flush(engine);
        }
        let deadline = (self.clock)() + self.idle_ms;
        match &mut self.gesture {
            None => {
                engine.begin_undo_group(label);
                self.gesture = Some(Gesture {
                    key: key.to_string(),
                    deadline,
                });
            }
            Some(g) => g.deadline = deadline,
        }
    }

    fn sync_pickup_context(&mut self, engine: &impl KnobEngine) {
        let ctx = format!(
            "{}:{}",
            engine.get_active_drum_track_id().unwrap_or_default(),
            engine.get_selected_pad()
        );
        if self.pickup_context.as_deref() != Some(ctx.as_str()) {
            self.pickup_context = Some(ctx);
            self.pickup.reset(None);
        }
    }

    /// Soft takeover gate: true when the knob may set the value (now at `current01`).
    fn takeover(&mut self, key: &str, value01: f64, current01: Option<f64>) -> bool {
        match current01 {
            None => true,
            Some(c) => self.pickup.check(key, clamp01(value01), c),
        }
    }

    pub fn handle(&mut self, engine: &mut impl KnobEngine, deps: &mut impl KnobDeps, target: KnobTarget, value01: f64) {
        // A due idle timer has fired before this message arrives.
        self.poll(engine);
        self.sync_pickup_context(engine);
        match target {
            KnobTarget::Pad { param } => self.handle_pad(engine, deps, param, value01),
            KnobTarget::Swing => self.handle_swing(engine, value01),
            KnobTarget::Bpm => self.handle_bpm(engine, value01),
            KnobTarget::TrackVolume => self.handle_track_volume(engine, value01),
            KnobTarget::MasterVolume => self.handle_master_volume(deps, value01),
        }
    }

    fn handle_pad(&mut self, engine: &mut impl KnobEngine, deps: &mut impl KnobDeps, param: PadParam, value01: f64) {
        let track_id = match engine.get_active_drum_track_id() {
            Some(id) => id,
            None => engine.ensure_drum_track(),
        };
        let pad = engine.get_selected_pad();
        let Some(settings) = engine.get_kit(&track_id).pads.get(pad).cloned() else {
            return;
        };
        let duration = deps.pad_buffer_duration(&settings);
        let key = format!("pad:{}", param.id());
        if !self.takeover(&key, value01, pad_param_to01(param, &settings, duration)) {
            return;
        }
        let Some(value) = knob_pad_value(param, value01, &settings, duration) else {
            return;
        };
        if value == pad_param_value(&settings, param) {
            return;
        }
        self.touch(
            engine,
            &format!("{}:{}:{}", track_id, pad, param.id()),
            pad_param_undo_label(param),
        );
        engine.update_pad(&track_id, pad, param, value);
        let set01 = engine
            .get_kit(&track_id)
            .pads
            .get(pad)
            .and_then(|after| pad_param_to01(param, after, duration));
        if let Some(set01) = set01 {
            self.pickup.applied(&key, set01);
        }
    }

    fn handle_swing(&mut self, engine: &mut impl KnobEngine, value01: f64) {
        let current = engine.get_loop_settings().swing;
        if !self.takeover("swing", value01, Some(swing_to01(current))) {
            return;
        }
        let swing = knob_swing(value01);
        if swing == current {
            return;
        }
        self.touch(engine, "swing", "Swing");
        engine.set_loop_swing(swing);
        let applied = swing_to01(engine.get_loop_settings().swing);
        self.pickup.applied("swing", applied);
    }

    fn handle_bpm(&mut self, engine: &mut impl KnobEngine, value01: f64) {
        let current = engine.get_bpm();
        if !self.takeover("bpm", value01, Some(bpm_to01(current))) {
            return;
        }
        let bpm = knob_bpm(value01); // whole BPM
        if bpm == current {
            return;
        }
        self.touch(engine, "bpm", "Tempo");
        engine.set_bpm(bpm);
        self.pickup.applied("bpm", bpm_to01(bpm));
    }

    fn handle_track_volume(&mut self, engine: &mut impl KnobEngine, value01: f64) {
        // `trackId ? …` — an empty id is falsy in JS.
        let Some(track_id) = engine.get_active_drum_track_id().filter(|id| !id.is_empty()) else {
            return;
        };
        let Some(volume) = engine.track_volume(&track_id) else {
            return;
        };
        let key = format!("trackVolume:{track_id}");
        if !self.takeover(&key, value01, Some(volume_to01(volume))) {
            return;
        }
        let db = knob_volume_db(value01);
        if db == volume {
            return;
        }
        self.touch(engine, &key, "Track Volume");
        engine.set_track_volume(&track_id, db);
        self.pickup.applied(&key, volume_to01(db));
    }

    fn handle_master_volume(&mut self, deps: &mut impl KnobDeps, value01: f64) {
        if !self.takeover("masterVolume", value01, Some(volume_to01(deps.get_master_volume()))) {
            return;
        }
        let db = knob_volume_db(value01);
        if db == deps.get_master_volume() {
            return;
        }
        deps.set_master_volume(db);
        self.pickup.applied("masterVolume", volume_to01(db));
    }
}

#[cfg(test)]
#[path = "knobs_tests.rs"]
mod tests;
