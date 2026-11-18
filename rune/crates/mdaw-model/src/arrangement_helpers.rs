//! Arrangement helpers: px/beat mapping, quantize + swing, loop gate lengths, drum hit merging,
//! GM drum note migration and MIDI-export mapping. Port of `src/utils/arrangementHelpers.ts`.
//!
//! `normalizeLoopSettings` / `normalizeBeatState` live in `loop_settings.rs` and are re-exported.

use serde_json::Value;

use crate::classic_meta::get_classic_voice;
use crate::kit::{create_factory_kit, factory_pad_for_gm_note, factory_voice_gm_note, normalize_kit};
use crate::js::{ge as js_ge, max as js_max, min as js_min, round as js_round};
pub use crate::loop_settings::{MAX_SWING, clamp_swing, normalize_beat_state, normalize_loop_settings};
use crate::types::{
    InstrumentType, KitState, LoopRecordSettings, PadSampleSource, QuantizeGrid, RegionNote, SwingGrid, Track,
    note_to_pad, pad_to_note,
};

pub fn beat_to_px(beat: f64, px_per_beat: f64) -> f64 {
    beat * px_per_beat
}

pub fn px_to_beat(px: f64, px_per_beat: f64) -> f64 {
    px / px_per_beat
}

/// `snapBeat`: nearest multiple of `snap_value` (unchanged when `snap_value <= 0`).
pub fn snap_beat(beat: f64, snap_value: f64) -> f64 {
    if snap_value <= 0.0 {
        return beat;
    }
    js_round(beat / snap_value) * snap_value
}

/// Positions closer than this (in beats) are the same step.
pub const STEP_EPSILON: f64 = 1e-4;
/// Velocity of a step drawn in the step grid (0..1).
pub const DEFAULT_STEP_VELOCITY: f64 = 0.8;

/// `DEFAULT_LOOP_SETTINGS`.
pub const DEFAULT_LOOP_SETTINGS: LoopRecordSettings = LoopRecordSettings {
    bars: 2,
    quantize: QuantizeGrid::Q16,
    swing: 0.0,
    swing_grid: SwingGrid::S16,
    count_in_bars: 1,
    metronome: true,
    overdub: true,
};

/// The TS swing functions take `SwingGrid | QuantizeGrid`; both share the labels "1/8" / "1/16".
impl From<SwingGrid> for QuantizeGrid {
    fn from(g: SwingGrid) -> Self {
        match g {
            SwingGrid::S8 => QuantizeGrid::Q8,
            SwingGrid::S16 => QuantizeGrid::Q16,
        }
    }
}

/// `gridStepBeats`: grid step length in beats (quarter notes); None for 'off'.
pub fn grid_step_beats(grid: QuantizeGrid) -> Option<f64> {
    match grid {
        QuantizeGrid::Off => None,
        QuantizeGrid::Q4 => Some(1.0),
        QuantizeGrid::Q8 => Some(0.5),
        QuantizeGrid::Q8T => Some(1.0 / 3.0),
        QuantizeGrid::Q16 => Some(0.25),
        QuantizeGrid::Q16T => Some(1.0 / 6.0),
        QuantizeGrid::Q32 => Some(0.125),
    }
}

/// Triplet steps (1/3, 1/6 beat...) are never swung.
fn is_triplet_step(step_beats: f64) -> bool {
    let l = step_beats.log2();
    (l - js_round(l)).abs() > 1e-6
}

/// Strip float noise (1/3-beat grids) so equal steps compare equal.
fn clean(beat: f64) -> f64 {
    js_round(beat * 1e9) / 1e9
}

/// `patternStepAt`: step-grid cell of a stored (straight) pattern note, clamped to [0, steps).
pub fn pattern_step_at(beat: f64, step_beats: f64, steps: f64) -> f64 {
    let step = js_round(clean(js_max(0.0, beat) / step_beats));
    js_min(js_max(0.0, steps - 1.0), js_max(0.0, step))
}

/// Step length (beats) the swing applies to (1/16 for 'off').
fn swing_step_beats(grid: QuantizeGrid) -> f64 {
    grid_step_beats(grid).unwrap_or(0.25)
}

/// `quantizeBeat(beat, grid, swing, swingGrid)`: nearest step by where it SOUNDS (swing delays the
/// odd steps of `swing_grid`), returned at its STRAIGHT position. The TS default for `swingGrid`
/// is `grid` itself. 'off' returns the beat as is.
pub fn quantize_beat(beat: f64, grid: QuantizeGrid, swing: f64, swing_grid: impl Into<QuantizeGrid>) -> f64 {
    let swing_grid = swing_grid.into();
    let Some(step) = grid_step_beats(grid) else { return beat };
    let k0 = js_round(clean(beat / step));
    if k0.is_infinite() {
        // Deviation: the TS loop never terminates for an infinite beat.
        return clean(k0 * step);
    }
    // The largest swing delay can move a step this many grid steps late.
    let span = ((MAX_SWING * swing_step_beats(swing_grid)) / step).ceil() + 1.0;
    let mut best = k0;
    let mut best_dist = f64::INFINITY;
    let mut k = k0 - span;
    while k <= k0 + span {
        let straight = clean(k * step);
        let dist = (beat - (straight + swing_offset_beats(straight, swing_grid, swing))).abs();
        if dist < best_dist - 1e-9 {
            best = k;
            best_dist = dist;
        }
        k += 1.0;
    }
    clean(best * step)
}

/// `loopSwingGrid`: swing grid of loop settings (older data without one: 1/16).
pub fn loop_swing_grid(swing_grid: Option<SwingGrid>) -> SwingGrid {
    if swing_grid == Some(SwingGrid::S8) { SwingGrid::S8 } else { SwingGrid::S16 }
}

/// `swingOffsetBeats`: playback delay of a straight note at absolute `beat` (odd steps of the
/// swing grid only; triplet grids never swing).
pub fn swing_offset_beats(beat: f64, grid: impl Into<QuantizeGrid>, swing: f64) -> f64 {
    let s = clamp_swing(swing);
    if s == 0.0 {
        return 0.0;
    }
    let step = swing_step_beats(grid.into());
    if is_triplet_step(step) {
        return 0.0;
    }
    let idx = js_round(beat / step);
    if (idx * step - beat).abs() > STEP_EPSILON || (idx % 2.0).abs() != 1.0 {
        return 0.0;
    }
    clean(s * step)
}

/// Shortest recorded gate length (beats): a 1/64 note.
const MIN_GATE_BEATS: f64 = 1.0 / 16.0;

/// `loopGateBeats`: held length (beats, musical time) of a gate pad recorded in a loop.
pub fn loop_gate_beats(on_beat: f64, off_beat: f64, wrapped_beats: f64, loop_beats: f64) -> f64 {
    let wrapped = if wrapped_beats.is_finite() { js_max(0.0, wrapped_beats) } else { 0.0 };
    let mut beats = off_beat - on_beat + wrapped;
    if !beats.is_finite() {
        return MIN_GATE_BEATS;
    }
    // A wrap the clock had not reported yet (or no tick clock): positions only move forward.
    if beats < 0.0 && loop_beats > 0.0 {
        beats += (-beats / loop_beats).ceil() * loop_beats;
    }
    if loop_beats > 0.0 {
        beats = js_min(loop_beats, beats);
    }
    js_max(MIN_GATE_BEATS, beats)
}

/// `wrapBeat`: wrap a beat into [0, length).
pub fn wrap_beat(beat: f64, length: f64) -> f64 {
    if length <= 0.0 {
        return beat;
    }
    let w = ((beat % length) + length) % length;
    if length - w < STEP_EPSILON { 0.0 } else { clean(w) }
}

/// `mergeDrumHit`: add a hit; one on the same pad at the same position (within STEP_EPSILON)
/// merges into the existing note, the louder one wins. Returns a new list.
pub fn merge_drum_hit(notes: &[RegionNote], hit: RegionNote) -> Vec<RegionNote> {
    let found = notes.iter().position(|n| n.note == hit.note && (n.start_beat - hit.start_beat).abs() < STEP_EPSILON);
    let mut out = notes.to_vec();
    match found {
        None => out.push(hit),
        Some(i) if !js_ge(out[i].velocity, hit.velocity) => out[i].velocity = hit.velocity,
        Some(_) => {}
    }
    out
}

/// Result of `migrateGmDrumNotes`.
#[derive(Debug, Clone, PartialEq)]
pub struct GmMigration {
    pub notes: Vec<RegionNote>,
    pub dropped: usize,
}

/// `migrateGmDrumNotes`: GM drum notes → factory-kit pad notes (unmapped notes dropped, hits on
/// the same pad at the same time merged).
pub fn migrate_gm_drum_notes(notes: &[RegionNote]) -> GmMigration {
    let mut out: Vec<RegionNote> = Vec::new();
    let mut dropped = 0;
    for n in notes {
        let Some(pad) = factory_pad_for_gm_note(n.note) else {
            dropped += 1;
            continue;
        };
        out = merge_drum_hit(&out, RegionNote { note: pad_to_note(pad) as f64, is_drum: true, ..n.clone() });
    }
    GmMigration { notes: out, dropped }
}

/// `migrateDrumTrack`: a kitless drum track (legacy GM) gets its notes mapped to factory pads and
/// the factory kit; a drum track with a kit only gets the kit validated. Returns the number of
/// dropped legacy notes. `packs_dir` / `new_id` feed `normalize_kit`.
pub fn migrate_drum_track(track: &mut Track, packs_dir: Option<&str>, new_id: &mut dyn FnMut() -> String) -> usize {
    if track.instrument.kind != InstrumentType::Drums {
        return 0;
    }
    if let Some(kit) = &track.instrument.kit {
        let raw = serde_json::to_value(kit).unwrap_or(Value::Null);
        track.instrument.kit = Some(normalize_kit(&raw, packs_dir, new_id));
        return 0;
    }
    let mut dropped = 0;
    for region in &mut track.regions {
        if region.audio.is_some() {
            continue;
        }
        let res = migrate_gm_drum_notes(&region.notes);
        region.notes = res.notes;
        dropped += res.dropped;
    }
    track.instrument.kit = Some(create_factory_kit());
    dropped
}

/// `drumNoteToGm`: MIDI export note for a drum pad note: the GM note of the factory/classic voice
/// on that pad, else the note itself.
pub fn drum_note_to_gm(note: f64, kit: Option<&KitState>) -> f64 {
    let Some(pad) = note_to_pad(note) else { return note };
    match kit.and_then(|k| k.pads.get(pad)).and_then(|p| p.sample.as_ref()).map(|s| &s.source) {
        Some(PadSampleSource::Factory { voice }) => factory_voice_gm_note(*voice) as f64,
        Some(PadSampleSource::Classic { machine, voice }) => {
            get_classic_voice(*machine, voice).map_or(note, |v| v.gm as f64)
        }
        _ => note,
    }
}

#[cfg(test)]
#[path = "arrangement_helpers_tests.rs"]
mod tests;
