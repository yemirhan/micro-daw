//! Loop-record settings and beat state normalization.
//! Port of `normalizeLoopSettings` / `normalizeBeatState` (src/utils/arrangementHelpers.ts:191-229).

use serde_json::Value;

/// `Math.round`: re-export of [`crate::js::round`] (kept under this name for existing callers).
pub use crate::js::round as js_round;

use crate::types::{
    BeatState, InstrumentType, LoopRecordSettings, PAD_COUNT, QuantizeGrid, SwingGrid, Track,
};

/// MPC-style swing never delays a step by more than 75% of it.
pub const MAX_SWING: f64 = 0.75;

const LOOP_BARS: [u32; 4] = [1, 2, 4, 8];
const COUNT_IN_BARS: [u32; 3] = [0, 1, 2];

/// `clampSwing`: non-finite → 0, else clamp to [0, 0.75].
pub fn clamp_swing(swing: f64) -> f64 {
    if swing.is_finite() { swing.clamp(0.0, MAX_SWING) } else { 0.0 }
}

/// JS strict equality of a JSON number with an integer option (`[1,2,4,8].find(b => b === r.bars)`).
fn int_member(v: Option<&Value>, set: &[u32]) -> Option<u32> {
    let n = v?.as_f64()?;
    set.iter().copied().find(|&b| b as f64 == n)
}

/// `normalizeLoopSettings(raw, base)`.
pub fn normalize_loop_settings(raw: &Value, base: &LoopRecordSettings) -> LoopRecordSettings {
    let empty = serde_json::Map::new();
    let r = raw.as_object().unwrap_or(&empty);
    let bars = int_member(r.get("bars"), &LOOP_BARS).unwrap_or(base.bars);
    let count_in_bars = int_member(r.get("countInBars"), &COUNT_IN_BARS).unwrap_or(base.count_in_bars);
    let raw_quantize = r.get("quantize").and_then(Value::as_str);
    let quantize = raw_quantize.and_then(QuantizeGrid::from_label).unwrap_or(base.quantize);
    let swing_grid = match r.get("swingGrid") {
        Some(Value::String(s)) if s == "1/8" => SwingGrid::S8,
        Some(Value::String(s)) if s == "1/16" => SwingGrid::S16,
        // Older data swung the quantize grid: keep a 1/8 swing sounding the same.
        None if raw_quantize.is_some() => {
            if raw_quantize == Some("1/8") { SwingGrid::S8 } else { SwingGrid::S16 }
        }
        _ => base.swing_grid,
    };
    LoopRecordSettings {
        bars,
        quantize,
        swing: match r.get("swing").and_then(Value::as_f64) {
            Some(s) => clamp_swing(s),
            None => base.swing,
        },
        swing_grid,
        count_in_bars,
        metronome: r.get("metronome").and_then(Value::as_bool).unwrap_or(base.metronome),
        overdub: r.get("overdub").and_then(Value::as_bool).unwrap_or(base.overdub),
    }
}

/// The track-independent half of `normalizeBeatState` (used while deserializing).
pub fn beat_state_from_value(raw: &Value) -> BeatState {
    let empty = serde_json::Map::new();
    let r = raw.as_object().unwrap_or(&empty);
    let pad = r.get("selectedPad").and_then(Value::as_f64).map(js_round).unwrap_or(0.0);
    BeatState {
        active_drum_track_id: r.get("activeDrumTrackId").and_then(Value::as_str).map(str::to_owned),
        r#loop: normalize_loop_settings(r.get("loop").unwrap_or(&Value::Null), &LoopRecordSettings::default()),
        selected_pad: if pad >= 0.0 && pad < PAD_COUNT as f64 { pad as usize } else { 0 },
    }
}

/// `normalizeBeatState(raw, tracks)`: unknown or non-drum active ids fall back to the first drum track.
pub fn normalize_beat_state(raw: Option<&BeatState>, tracks: &[Track]) -> BeatState {
    let mut drum_ids = tracks.iter().filter(|t| t.instrument.kind == InstrumentType::Drums).map(|t| &t.id);
    let first = drum_ids.clone().next().cloned();
    let active = match raw.and_then(|b| b.active_drum_track_id.as_ref()) {
        Some(id) if drum_ids.any(|d| d == id) => Some(id.clone()),
        _ => first,
    };
    BeatState {
        active_drum_track_id: active,
        r#loop: raw.map(|b| b.r#loop).unwrap_or_default(),
        selected_pad: raw.map(|b| b.selected_pad).unwrap_or(0),
    }
}
