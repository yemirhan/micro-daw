//! Pure helpers for the Beat view (pad layout, computer-keyboard pads, step grid math, sample
//! drops, chopping, knob formatting, loop hints). Port of `src/components/beat/beatHelpers.ts`
//! plus the other non-React helpers of `src/components/beat/` (sibling files):
//!
//! - `beat_helpers_keys.rs`: `stepKeys.ts`, `keyScope.ts` and `isTypingTarget` (DOM-free model);
//! - `beat_helpers_chop.rs`: `chop.ts` (on top of [`crate::slicer`]) and the pure half of
//!   `missingPackActions.ts`;
//! - `beat_helpers_waveform.rs`: `waveform.ts` (peaks, peak cache, canvas sizing, peak rects);
//! - `beat_helpers_ui.rs`: small pure formatters living in the `.tsx` components.
//!
//! JS number semantics come from [`crate::js`].

// `!(x > 0.0)` is the literal TS check: it is also true for NaN.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use crate::arrangement_helpers::pattern_step_at;
use crate::classic_meta::classic_sample_name;
use crate::fs_entries::is_audio_file_name;
use crate::js::{max as js_max, min as js_min, round as js_round, round_string as js_round_string};
use crate::kit::factory_voice_label;
use crate::notes::midi_to_note_name;
use crate::paths::display_name;
use crate::types::{
    LoopMarkers, PAD_COUNT, PadIndex, PadSample, PadSampleSource, PadSettings, Region, RegionNote,
    pad_to_note,
};

#[path = "beat_helpers_chop.rs"]
mod chop;
#[path = "beat_helpers_keys.rs"]
mod keys;
#[path = "beat_helpers_ui.rs"]
mod ui;
#[path = "beat_helpers_waveform.rs"]
mod waveform;

pub use crate::arrangement_helpers::STEP_EPSILON;
pub use crate::js::{number_to_string as js_number_to_string, to_fixed as js_to_fixed};
pub use crate::kit::PAD_COLORS;
pub use crate::slicer::SliceRange;
pub use chop::*;
pub use keys::*;
pub use ui::*;
pub use waveform::*;

// ---------------------------------------------------------------------------
// Pad layout
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PadLayout {
    /// MPC: pad 0 bottom-left, bank A = bottom two rows, bank B = top two.
    FourByFour,
    /// MPK Mini Play: bank A | bank B side by side, each 2x4 with pad 0/8 bottom-left.
    TwoByEight,
}

impl PadLayout {
    pub fn label(self) -> &'static str {
        match self {
            Self::FourByFour => "4x4",
            Self::TwoByEight => "2x8",
        }
    }

    pub fn from_label(s: &str) -> Option<Self> {
        match s {
            "4x4" => Some(Self::FourByFour),
            "2x8" => Some(Self::TwoByEight),
            _ => None,
        }
    }
}

/// `padRows(layout)`: pad indices row by row, top to bottom, as they appear on screen.
pub fn pad_rows(layout: PadLayout) -> Vec<Vec<PadIndex>> {
    match layout {
        PadLayout::TwoByEight => vec![
            vec![4, 5, 6, 7, 12, 13, 14, 15],
            vec![0, 1, 2, 3, 8, 9, 10, 11],
        ],
        PadLayout::FourByFour => {
            vec![
                vec![12, 13, 14, 15],
                vec![8, 9, 10, 11],
                vec![4, 5, 6, 7],
                vec![0, 1, 2, 3],
            ]
        }
    }
}

/// `padBank(pad)`: 'A' for pads 0..7, else 'B'.
pub fn pad_bank(pad: PadIndex) -> char {
    if pad < PAD_COUNT / 2 { 'A' } else { 'B' }
}

/// `padLabel(pad)`: "A1".."A8", "B1".."B8" — how the hardware labels its pads.
pub fn pad_label(pad: PadIndex) -> String {
    format!("{}{}", pad_bank(pad), (pad % 8) + 1)
}

/// `padColor(settings, pad)`: `settings?.color ?? PAD_COLORS[pad % 16]`.
pub fn pad_color(settings: Option<&PadSettings>, pad: PadIndex) -> String {
    match settings.and_then(|s| s.color.as_ref()) {
        Some(c) => c.clone(),
        None => PAD_COLORS[pad % PAD_COLORS.len()].to_string(),
    }
}

/// `padName(settings)`: '' without a sample, else its name, else a name derived from the source.
pub fn pad_name(settings: Option<&PadSettings>) -> String {
    let Some(sample) = settings.and_then(|s| s.sample.as_ref()) else {
        return String::new();
    };
    if !sample.name.is_empty() {
        return sample.name.clone();
    }
    match &sample.source {
        PadSampleSource::Factory { voice } => factory_voice_label(*voice).to_string(),
        PadSampleSource::Classic { machine, voice } => classic_sample_name(*machine, voice),
        PadSampleSource::Pack { file, .. } => display_name(file),
        PadSampleSource::File { path, .. } => display_name(path),
    }
}

/// `hardwareNoteLabel(note)`: "—" for null / non-finite, else `midiToNoteName(Math.round(note))`.
pub fn hardware_note_label(note: Option<f64>) -> String {
    match note {
        Some(n) if n.is_finite() => midi_to_note_name(js_round(n)),
        _ => "—".to_string(),
    }
}

/// `velocityFromPointer(clientY, rectTop, rectHeight)`: top edge = 1, bottom = 0.3; 0.8 for a
/// non-positive (or NaN) height.
pub fn velocity_from_pointer(client_y: f64, rect_top: f64, rect_height: f64) -> f64 {
    if !(rect_height > 0.0) {
        return 0.8;
    }
    let t = js_min(1.0, js_max(0.0, (client_y - rect_top) / rect_height));
    js_round((1.0 - t * 0.7) * 100.0) / 100.0
}

// ---------------------------------------------------------------------------
// Computer keyboard pads
// ---------------------------------------------------------------------------

/// `KEY_CODE_TO_PAD`: 4x4 MPC layout on the left of a QWERTY keyboard (physical key code).
/// Pad 11 is on T, not R: in the beat view R toggles loop recording.
const KEY_CODE_TO_PAD: [(&str, PadIndex); 16] = [
    ("KeyZ", 0),
    ("KeyX", 1),
    ("KeyC", 2),
    ("KeyV", 3),
    ("KeyA", 4),
    ("KeyS", 5),
    ("KeyD", 6),
    ("KeyF", 7),
    ("KeyQ", 8),
    ("KeyW", 9),
    ("KeyE", 10),
    ("KeyT", 11),
    ("Digit1", 12),
    ("Digit2", 13),
    ("Digit3", 14),
    ("Digit4", 15),
];

/// `KEY_TO_PAD`: the same layout by character.
const KEY_TO_PAD: [(&str, PadIndex); 16] = [
    ("z", 0),
    ("x", 1),
    ("c", 2),
    ("v", 3),
    ("a", 4),
    ("s", 5),
    ("d", 6),
    ("f", 7),
    ("q", 8),
    ("w", 9),
    ("e", 10),
    ("t", 11),
    ("1", 12),
    ("2", 13),
    ("3", 14),
    ("4", 15),
];

/// `PAD_KEY_HINTS`: key cap shown on each pad.
pub const PAD_KEY_HINTS: [&str; PAD_COUNT] = [
    "Z", "X", "C", "V", "A", "S", "D", "F", "Q", "W", "E", "T", "1", "2", "3", "4",
];

/// The parts of a `KeyboardEvent` the beat view's key handlers read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeyEventLike {
    pub key: String,
    pub code: String,
    pub meta: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

/// `padForKeyEvent(e)`: pad for a key event, or None. Modifier chords (⌘S, Ctrl+Z…) never hit
/// pads; Shift is not checked here (the caller rejects it).
pub fn pad_for_key_event(e: &KeyEventLike) -> Option<PadIndex> {
    if e.meta || e.ctrl || e.alt {
        return None;
    }
    if !e.code.is_empty()
        && let Some(&(_, pad)) = KEY_CODE_TO_PAD.iter().find(|(c, _)| *c == e.code)
    {
        return Some(pad);
    }
    let k = e.key.to_lowercase();
    if k.is_empty() {
        return None;
    }
    KEY_TO_PAD.iter().find(|(c, _)| *c == k).map(|&(_, p)| p)
}

// ---------------------------------------------------------------------------
// Step grid
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StepGrid {
    S8,
    S16,
    S32,
}

impl StepGrid {
    pub fn label(self) -> &'static str {
        match self {
            Self::S8 => "1/8",
            Self::S16 => "1/16",
            Self::S32 => "1/32",
        }
    }

    pub fn from_label(s: &str) -> Option<Self> {
        STEP_GRIDS.into_iter().find(|g| g.label() == s)
    }
}

/// `STEP_GRIDS`.
pub const STEP_GRIDS: [StepGrid; 3] = [StepGrid::S8, StepGrid::S16, StepGrid::S32];

/// `stepBeatsFor(grid)`.
pub fn step_beats_for(grid: StepGrid) -> f64 {
    match grid {
        StepGrid::S8 => 0.5,
        StepGrid::S32 => 0.125,
        StepGrid::S16 => 0.25,
    }
}

/// `stepCount(bars, stepBeats)`: `max(1, round(bars * 4 / stepBeats))`. (JS NaN → 0 here, which
/// is also the length `new Float32Array(NaN)` gets.)
pub fn step_count(bars: f64, step_beats: f64) -> usize {
    js_max(1.0, js_round((bars * 4.0) / step_beats)) as usize
}

/// `stepsPerBeat(stepBeats)`: steps per beat-group shading block (one beat).
pub fn steps_per_beat(step_beats: f64) -> usize {
    js_max(1.0, js_round(1.0 / step_beats)) as usize
}

/// `patternStepAt(beat, stepBeats, steps)` (re-exported by beatHelpers from arrangementHelpers):
/// nearest straight step, clamped to `[0, steps - 1]`.
pub fn pattern_step_at_index(beat: f64, step_beats: f64, steps: usize) -> usize {
    pattern_step_at(beat, step_beats, steps as f64) as usize
}

/// `normVelocity(v)`: velocity (0..1 or 0..127) as 0..1.
pub fn norm_velocity(v: f64) -> f64 {
    js_min(1.0, js_max(0.0, if v > 1.0 { v / 127.0 } else { v }))
}

/// `Pick<Region, 'startBeat' | 'notes'>`.
#[derive(Debug, Clone, Copy)]
pub struct StepRegion<'a> {
    pub start_beat: f64,
    pub notes: &'a [RegionNote],
}

impl<'a> From<&'a Region> for StepRegion<'a> {
    fn from(r: &'a Region) -> Self {
        Self {
            start_beat: r.start_beat,
            notes: &r.notes,
        }
    }
}

/// `buildStepMatrix(region, patternStartBeat, stepBeats, steps)`: `matrix[pad * steps + step]` =
/// velocity 0..1 (0 = empty), as `f32` like the TS `Float32Array`. The loudest note in a cell
/// wins; a velocity-0 note still shows as 0.02.
pub fn build_step_matrix(
    region: Option<StepRegion<'_>>,
    pattern_start_beat: f64,
    step_beats: f64,
    steps: usize,
) -> Vec<f32> {
    let mut matrix = vec![0f32; PAD_COUNT * steps];
    let Some(region) = region else { return matrix };
    for n in region.notes {
        let pad = n.note - pad_to_note(0) as f64;
        if pad < 0.0 || pad >= PAD_COUNT as f64 {
            continue;
        }
        let beat = region.start_beat + n.start_beat - pattern_start_beat;
        if beat < -STEP_EPSILON || beat >= steps as f64 * step_beats - STEP_EPSILON {
            continue;
        }
        let step = pattern_step_at(beat, step_beats, steps as f64);
        let i = pad * steps as f64 + step;
        // A non-integer index is a no-op on a typed array (fractional notes, NaN steps).
        if i.fract() != 0.0 || !(i >= 0.0) || i >= matrix.len() as f64 {
            continue;
        }
        let i = i as usize;
        matrix[i] = js_max(matrix[i] as f64, js_max(0.02, norm_velocity(n.velocity))) as f32;
    }
    matrix
}

/// `playheadStep(...)`: fractional step of the playhead, or None outside the pattern.
pub fn playhead_step(
    position_beat: f64,
    pattern_start_beat: f64,
    pattern_beats: f64,
    step_beats: f64,
) -> Option<f64> {
    if !(pattern_beats > 0.0) {
        return None;
    }
    let rel = position_beat - pattern_start_beat;
    if rel < 0.0 || rel >= pattern_beats {
        return None;
    }
    Some(rel / step_beats)
}

/// A step-grid cell (`StepCell` in stepKeys.ts, `{ pad, step }` in cellAt).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StepCell {
    pub pad: PadIndex,
    pub step: usize,
}

/// `cellAt(x, y, cellW, rowH, steps)`: cell under a point in canvas CSS pixels. Rows run from
/// pad 15 (top) to pad 0 (bottom). (NaN coordinates give None; TS would return a NaN cell.)
pub fn cell_at(x: f64, y: f64, cell_w: f64, row_h: f64, steps: usize) -> Option<StepCell> {
    if x < 0.0 || y < 0.0 || !(cell_w > 0.0) || !(row_h > 0.0) || x.is_nan() || y.is_nan() {
        return None;
    }
    let step = (x / cell_w).floor();
    let row = (y / row_h).floor();
    if step >= steps as f64 || row >= PAD_COUNT as f64 {
        return None;
    }
    Some(StepCell {
        pad: PAD_COUNT - 1 - row as usize,
        step: step as usize,
    })
}

/// `rowForPad(pad)`.
pub fn row_for_pad(pad: PadIndex) -> usize {
    PAD_COUNT - 1 - pad
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaintMode {
    Draw,
    Erase,
}

impl PaintMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Draw => "draw",
            Self::Erase => "erase",
        }
    }
}

/// `paintModeFor(v)`: start on an empty cell = draw, on a filled one = erase.
pub fn paint_mode_for(cell_velocity: f64) -> PaintMode {
    if cell_velocity > 0.0 {
        PaintMode::Erase
    } else {
        PaintMode::Draw
    }
}

/// `needsToggle(mode, v)`: whether painting over a cell needs a toggle.
pub fn needs_toggle(mode: PaintMode, cell_velocity: f64) -> bool {
    match mode {
        PaintMode::Draw => cell_velocity <= 0.0,
        PaintMode::Erase => cell_velocity > 0.0,
    }
}

/// Default `pxPerFull` of `adjustVelocity`.
pub const ADJUST_VELOCITY_PX_PER_FULL: f64 = 120.0;

/// `adjustVelocity(start, deltaUp, pxPerFull = 120)`: never below 0.05, whole percent.
pub fn adjust_velocity(start: f64, delta_up: f64, px_per_full: f64) -> f64 {
    let v = start + delta_up / px_per_full;
    js_round(js_min(1.0, js_max(0.05, v)) * 100.0) / 100.0
}

// ---------------------------------------------------------------------------
// Sample drops
// ---------------------------------------------------------------------------

/// `SAMPLE_DRAG_MIME` (browserHelpers.ts): drag payload type carrying `{ path, name }` JSON.
pub const SAMPLE_DRAG_MIME: &str = "application/x-micro-daw-sample";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DroppedSample {
    pub path: String,
    pub name: String,
}

/// The parts of a `DataTransfer` the drop handlers read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DragDataLike {
    /// `dt.types`.
    pub types: Vec<String>,
    /// `dt.getData(SAMPLE_DRAG_MIME)` ('' when absent).
    pub sample_payload: String,
    /// `dt.files` names, in order.
    pub file_names: Vec<String>,
}

/// `dragHasSample(dt)`: the drag carries a browser sample or files.
pub fn drag_has_sample(types: Option<&[String]>) -> bool {
    let Some(types) = types else { return false };
    types.iter().any(|t| t == SAMPLE_DRAG_MIME || t == "Files")
}

/// `readDroppedSample(dt, getPathForFile?)`: the browser's JSON payload first, then the first
/// audio file (by name) whose resolved path is non-empty.
pub fn read_dropped_sample(
    dt: Option<&DragDataLike>,
    get_path_for_file: Option<&dyn Fn(&str) -> String>,
) -> Option<DroppedSample> {
    let dt = dt?;
    let raw = &dt.sample_payload;
    if !raw.is_empty()
        && let Ok(serde_json::Value::Object(p)) = serde_json::from_str::<serde_json::Value>(raw)
        && let Some(serde_json::Value::String(path)) = p.get("path")
        && !path.is_empty()
    {
        let name = match p.get("name") {
            Some(serde_json::Value::String(n)) if !n.is_empty() => n.clone(),
            _ => display_name(path.rsplit(['\\', '/']).next().unwrap_or(path)),
        };
        return Some(DroppedSample {
            path: path.clone(),
            name,
        });
    }
    for file in &dt.file_names {
        if !is_audio_file_name(file) {
            continue;
        }
        let path = get_path_for_file.map(|f| f(file)).unwrap_or_default();
        if !path.is_empty() {
            return Some(DroppedSample {
                path,
                name: display_name(file),
            });
        }
    }
    None
}

/// `fileSample(s)`.
pub fn file_sample(s: &DroppedSample) -> PadSample {
    PadSample {
        source: PadSampleSource::File {
            path: s.path.clone(),
            sample_id: None,
        },
        name: s.name.clone(),
    }
}

// ---------------------------------------------------------------------------
// Chop to pads
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChopAssignment {
    pub pad: PadIndex,
    pub start_sec: f64,
    pub end_sec: f64,
}

/// `planChop(fromPad, ranges)`: slices → consecutive pads from `fromPad`; past pad 15 dropped.
pub fn plan_chop(from_pad: PadIndex, ranges: &[SliceRange]) -> Vec<ChopAssignment> {
    let mut out = Vec::new();
    for (i, r) in ranges.iter().enumerate() {
        let pad = from_pad + i;
        if pad >= PAD_COUNT {
            break;
        }
        out.push(ChopAssignment {
            pad,
            start_sec: r.start_sec,
            end_sec: r.end_sec,
        });
    }
    out
}

// ---------------------------------------------------------------------------
// Log-scaled knob params
// ---------------------------------------------------------------------------

/// `fromLog01(t, min, max)`: map 0..1 onto [min, max] logarithmically.
pub fn from_log01(t: f64, min: f64, max: f64) -> f64 {
    min * (max / min).powf(js_min(1.0, js_max(0.0, t)))
}

/// `toLog01(v, min, max)`.
pub fn to_log01(v: f64, min: f64, max: f64) -> f64 {
    js_min(
        1.0,
        js_max(0.0, (js_max(min, v) / min).ln() / (max / min).ln()),
    )
}

/// `formatSeconds(sec)`: "250ms" below a second, else "1.25s".
pub fn format_seconds(sec: f64) -> String {
    if sec < 1.0 {
        format!("{}ms", js_round_string(sec * 1000.0))
    } else {
        format!("{}s", js_to_fixed(sec, 2))
    }
}

/// `formatHz(hz)`: "440", "2.5k", "12k".
pub fn format_hz(hz: f64) -> String {
    if hz >= 1000.0 {
        format!(
            "{}k",
            js_to_fixed(hz / 1000.0, if hz >= 10000.0 { 0 } else { 1 })
        )
    } else {
        js_round_string(hz)
    }
}

// ---------------------------------------------------------------------------
// Loop range vs pattern range
// ---------------------------------------------------------------------------

/// `beatsLabel(beats)` (private in TS): "2 bars", "1 bar", "6 beats".
fn beats_label(beats: f64) -> String {
    let bars = beats / 4.0;
    if (bars - js_round(bars)).abs() < STEP_EPSILON {
        let n = js_round(bars);
        return format!(
            "{} bar{}",
            js_number_to_string(n),
            if n == 1.0 { "" } else { "s" }
        );
    }
    let fixed: f64 = js_to_fixed(beats, 2).parse().unwrap_or(f64::NAN);
    format!("{} beats", js_number_to_string(fixed))
}

/// Options of `describeLoopMismatch`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoopMismatchInput {
    pub r#loop: Option<LoopMarkers>,
    pub loop_enabled: bool,
    pub playing: bool,
    pub pattern_start_beat: f64,
    pub bars: f64,
}

/// `describeLoopMismatch(opts)`: why the transport would not loop exactly the pattern the step
/// grid shows (None = it does).
pub fn describe_loop_mismatch(opts: &LoopMismatchInput) -> Option<String> {
    if opts.playing && !opts.loop_enabled {
        return Some("Loop is off: playback runs past the pattern".to_string());
    }
    let lp = opts.r#loop?;
    let pattern_beats = opts.bars * 4.0;
    let loop_beats = lp.end_beat - lp.start_beat;
    let same_start = (lp.start_beat - opts.pattern_start_beat).abs() < STEP_EPSILON;
    let same_length = (loop_beats - pattern_beats).abs() < STEP_EPSILON;
    if same_start && same_length {
        return None;
    }
    if !same_length {
        return Some(format!(
            "Loop is {}, pattern is {}",
            beats_label(loop_beats),
            beats_label(pattern_beats)
        ));
    }
    Some("Loop range starts away from the pattern".to_string())
}

#[cfg(test)]
#[path = "beat_helpers_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "beat_helpers_golden_tests.rs"]
pub(crate) mod golden_tests;
