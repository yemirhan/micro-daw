//! Note edits: move / transpose / resize / lengths / draw / delete / duplicate / clipboard /
//! quantize / velocity.

use super::layout::RowLayout;
use super::math::{
    clamp, clamp_velocity, js_max, js_min, js_round, round_beat, snap_beat, snap_floor,
};
use super::types::*;
use crate::types::RegionNote;

/// `uniqueSorted(indices, length)`: deduped, in range, ascending.
fn unique_sorted(indices: &[usize], length: usize) -> Vec<usize> {
    let mut v: Vec<usize> = indices.iter().copied().filter(|&i| i < length).collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// Membership mask for `new Set(indices).has(i)` over `0..len`.
fn mask(indices: &[usize], len: usize) -> Vec<bool> {
    let mut m = vec![false; len];
    for &i in indices {
        if i < len {
            m[i] = true;
        }
    }
    m
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MoveDelta {
    pub d_beat: f64,
    pub d_row: f64,
}

/// Clamp a move so every selected note stays at startBeat >= 0 and inside the row range.
/// `d_row` is in rows (positive = down = lower pitch for the piano layout).
pub fn clamp_move_delta(
    notes: &[RegionNote],
    indices: &[usize],
    d_beat: f64,
    d_row: f64,
    layout: &RowLayout,
) -> MoveDelta {
    let idx = unique_sorted(indices, notes.len());
    if idx.is_empty() {
        return MoveDelta {
            d_beat: 0.0,
            d_row: 0.0,
        };
    }
    let mut min_start = f64::INFINITY;
    let mut min_row = f64::INFINITY;
    let mut max_row = f64::NEG_INFINITY;
    for &i in &idx {
        min_start = js_min(min_start, notes[i].start_beat);
        let r = layout.pitch_to_row(notes[i].note);
        if r >= 0.0 {
            min_row = js_min(min_row, r);
            max_row = js_max(max_row, r);
        }
    }
    let beat = js_max(d_beat, -min_start);
    let mut row = d_row;
    if min_row != f64::INFINITY {
        row = clamp(d_row, -min_row, layout.row_count as f64 - 1.0 - max_row);
    }
    MoveDelta {
        d_beat: round_beat(beat),
        d_row: js_round(row),
    }
}

/// Beat delta that lands the anchor note's start on the grid.
pub fn snapped_move_delta(anchor_start: f64, raw_delta: f64, snap: f64, offset: f64) -> f64 {
    if snap <= 0.0 {
        return round_beat(raw_delta);
    }
    round_beat(snap_beat(anchor_start + raw_delta, snap, offset) - anchor_start)
}

/// Move the given notes by d_beat / d_row (clamped). Returns a new list.
pub fn move_notes(
    notes: &[RegionNote],
    indices: &[usize],
    d_beat: f64,
    d_row: f64,
    layout: &RowLayout,
) -> Vec<RegionNote> {
    let idx = unique_sorted(indices, notes.len());
    let d = clamp_move_delta(notes, &idx, d_beat, d_row, layout);
    if d.d_beat == 0.0 && d.d_row == 0.0 {
        return notes.to_vec();
    }
    let m = mask(&idx, notes.len());
    notes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            if !m[i] {
                return n.clone();
            }
            let row = layout.pitch_to_row(n.note);
            let note = if row >= 0.0 && d.d_row != 0.0 {
                layout.row_to_pitch(row + d.d_row)
            } else {
                n.note
            };
            RegionNote {
                start_beat: round_beat(n.start_beat + d.d_beat),
                note,
                ..n.clone()
            }
        })
        .collect()
}

/// Transpose by semitones (piano) — positive = up. Clamped to the MIDI range.
pub fn transpose_notes(
    notes: &[RegionNote],
    indices: &[usize],
    semitones: f64,
    layout: &RowLayout,
) -> Vec<RegionNote> {
    move_notes(notes, indices, 0.0, -semitones, layout)
}

/// Resize from the start or end edge (`edge == End` → end, anything else → start, as in TS). The
/// anchor note's edge snaps to the grid and the same delta is applied to all selected notes. A note
/// never gets shorter than min(grid, its original length) or the absolute minimum length.
pub fn resize_notes(
    notes: &[RegionNote],
    indices: &[usize],
    edge: NoteEdge,
    anchor_index: usize,
    raw_delta: f64,
    snap: f64,
    offset: f64,
) -> Vec<RegionNote> {
    let idx = unique_sorted(indices, notes.len());
    let Some(anchor) = notes.get(anchor_index) else {
        return notes.to_vec();
    };
    let is_end = edge == NoteEdge::End;
    let delta = if is_end {
        let end = anchor.start_beat + anchor.duration_beats;
        if snap > 0.0 {
            snap_beat(end + raw_delta, snap, offset) - end
        } else {
            raw_delta
        }
    } else if snap > 0.0 {
        snap_beat(anchor.start_beat + raw_delta, snap, offset) - anchor.start_beat
    } else {
        raw_delta
    };
    let min_len = |dur: f64| {
        js_max(
            MIN_NOTE_LENGTH,
            js_min(if snap > 0.0 { snap } else { MIN_NOTE_LENGTH }, dur),
        )
    };
    let m = mask(&idx, notes.len());
    notes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            if !m[i] {
                return n.clone();
            }
            if is_end {
                let dur = js_max(min_len(n.duration_beats), n.duration_beats + delta);
                return RegionNote {
                    duration_beats: round_beat(dur),
                    ..n.clone()
                };
            }
            let end = n.start_beat + n.duration_beats;
            let start = clamp(n.start_beat + delta, 0.0, end - min_len(n.duration_beats));
            RegionNote {
                start_beat: round_beat(start),
                duration_beats: round_beat(end - start),
                ..n.clone()
            }
        })
        .collect()
}

/// Change lengths by a fixed amount (keyboard Alt+Left/Right). `min_length` defaults to
/// [`MIN_NOTE_LENGTH`] in TS.
pub fn change_lengths(
    notes: &[RegionNote],
    indices: &[usize],
    delta: f64,
    min_length: f64,
) -> Vec<RegionNote> {
    let m = mask(&unique_sorted(indices, notes.len()), notes.len());
    notes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            if m[i] {
                RegionNote {
                    duration_beats: round_beat(js_max(min_length, n.duration_beats + delta)),
                    ..n.clone()
                }
            } else {
                n.clone()
            }
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawSpan {
    pub start_beat: f64,
    pub duration_beats: f64,
}

/// Start + duration of a note being drawn: anchored at `anchor_start`, extending to the grid line
/// nearest the cursor (at least one grid step).
pub fn draw_span(anchor_start: f64, cursor_beat: f64, snap: f64, offset: f64) -> DrawSpan {
    let step = if snap > 0.0 { snap } else { MIN_NOTE_LENGTH };
    if cursor_beat >= anchor_start {
        let end = if snap > 0.0 {
            snap_beat(cursor_beat, snap, offset)
        } else {
            cursor_beat
        };
        return DrawSpan {
            start_beat: anchor_start,
            duration_beats: round_beat(js_max(step, end - anchor_start)),
        };
    }
    // Dragging left of the anchor: the note grows leftwards, ending one step after the anchor.
    let start = js_max(
        0.0,
        if snap > 0.0 {
            snap_floor(cursor_beat, snap, offset)
        } else {
            cursor_beat
        },
    );
    DrawSpan {
        start_beat: round_beat(start),
        duration_beats: round_beat(anchor_start + step - start),
    }
}

pub fn delete_notes(notes: &[RegionNote], indices: &[usize]) -> Vec<RegionNote> {
    let m = mask(indices, notes.len());
    notes
        .iter()
        .enumerate()
        .filter(|(i, _)| !m[*i])
        .map(|(_, n)| n.clone())
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelectionSpan {
    pub start: f64,
    pub end: f64,
}

/// [start, end) of the selected notes, or None.
pub fn selection_span(notes: &[RegionNote], indices: &[usize]) -> Option<SelectionSpan> {
    let mut start = f64::INFINITY;
    let mut end = f64::NEG_INFINITY;
    for &i in indices {
        let Some(n) = notes.get(i) else { continue };
        start = js_min(start, n.start_beat);
        end = js_max(end, n.start_beat + n.duration_beats);
    }
    if start == f64::INFINITY {
        None
    } else {
        Some(SelectionSpan { start, end })
    }
}

/// Append copies of the given notes, shifted by d_beat / d_row; the copies become the selection.
/// (TS defaults: `d_beat = 0`, `d_row = 0`, no layout.)
pub fn append_copies(
    notes: &[RegionNote],
    indices: &[usize],
    d_beat: f64,
    d_row: f64,
    layout: Option<&RowLayout>,
) -> NotesEdit {
    let idx = unique_sorted(indices, notes.len());
    let copies: Vec<RegionNote> = idx
        .iter()
        .map(|&i| {
            let n = &notes[i];
            let mut note = n.note;
            if let Some(layout) = layout.filter(|_| d_row != 0.0) {
                let row = layout.pitch_to_row(n.note);
                if row >= 0.0 {
                    note = layout.row_to_pitch(row + d_row);
                }
            }
            RegionNote {
                note,
                start_beat: round_beat(js_max(0.0, n.start_beat + d_beat)),
                ..n.clone()
            }
        })
        .collect();
    let selection = (0..copies.len()).map(|k| notes.len() + k).collect();
    let mut out = notes.to_vec();
    out.extend(copies);
    NotesEdit {
        notes: out,
        selection,
    }
}

/// Cmd+D: duplicate the selection right after itself. The shift is the selection's span rounded
/// up to the grid (so patterns stay on the grid).
pub fn duplicate_notes(notes: &[RegionNote], indices: &[usize], snap: f64) -> NotesEdit {
    let idx = unique_sorted(indices, notes.len());
    let Some(span) = selection_span(notes, &idx) else {
        return NotesEdit {
            notes: notes.to_vec(),
            selection: Vec::new(),
        };
    };
    let len = span.end - span.start;
    let unit = if snap > 0.0 { snap } else { MIN_NOTE_LENGTH };
    let shift = js_max(unit, (len / unit - EPS).ceil() * unit);
    append_copies(notes, &idx, round_beat(shift), 0.0, None)
}

/// Copy notes to a clipboard payload, normalised so the earliest note starts at 0.
pub fn copy_notes(notes: &[RegionNote], indices: &[usize]) -> Vec<RegionNote> {
    let idx = unique_sorted(indices, notes.len());
    let Some(span) = selection_span(notes, &idx) else {
        return Vec::new();
    };
    idx.iter()
        .map(|&i| RegionNote {
            start_beat: round_beat(notes[i].start_beat - span.start),
            ..notes[i].clone()
        })
        .collect()
}

/// Paste clipboard notes at `at_beat`; the pasted notes become the selection. `is_drum: None`
/// keeps each note's own flag.
pub fn paste_notes(
    notes: &[RegionNote],
    clipboard: &[RegionNote],
    at_beat: f64,
    is_drum: Option<bool>,
) -> NotesEdit {
    let at = js_max(0.0, at_beat);
    let mut out = notes.to_vec();
    out.extend(clipboard.iter().map(|n| RegionNote {
        is_drum: is_drum.unwrap_or(n.is_drum),
        start_beat: round_beat(n.start_beat + at),
        ..n.clone()
    }));
    NotesEdit {
        notes: out,
        selection: (0..clipboard.len()).map(|k| notes.len() + k).collect(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QuantizeOptions<'a> {
    /// Only these notes (default: all).
    pub indices: Option<&'a [usize]>,
    /// Also snap note ends (length).
    pub ends: bool,
    /// Region start (absolute beat) so the grid matches the song's bars.
    pub offset: f64,
    /// 0..1, how far to move towards the grid.
    pub strength: f64,
}

impl Default for QuantizeOptions<'_> {
    fn default() -> Self {
        Self {
            indices: None,
            ends: false,
            offset: 0.0,
            strength: 1.0,
        }
    }
}

/// Quantize note starts (and optionally ends) to the grid. `snap_value <= 0` returns the notes
/// unchanged (the TS returns the same array).
pub fn quantize_notes(
    notes: &[RegionNote],
    snap_value: f64,
    opts: &QuantizeOptions<'_>,
) -> Vec<RegionNote> {
    if snap_value <= 0.0 {
        return notes.to_vec();
    }
    let QuantizeOptions {
        ends,
        offset,
        strength,
        ..
    } = *opts;
    let only = opts.indices.map(|ix| mask(ix, notes.len()));
    notes
        .iter()
        .enumerate()
        .map(|(i, note)| {
            if only.as_ref().is_some_and(|m| !m[i]) {
                return note.clone();
            }
            let start = js_max(
                0.0,
                note.start_beat
                    + (snap_beat(note.start_beat, snap_value, offset) - note.start_beat) * strength,
            );
            let mut dur = note.duration_beats;
            if ends {
                let end = note.start_beat + note.duration_beats;
                let q_end = end + (snap_beat(end, snap_value, offset) - end) * strength;
                let s = snap_value * strength;
                // `snapValue * strength || MIN_NOTE_LENGTH`
                let floor = if s == 0.0 || s.is_nan() {
                    MIN_NOTE_LENGTH
                } else {
                    s
                };
                dur = js_max(floor, q_end - start);
            }
            RegionNote {
                start_beat: round_beat(start),
                duration_beats: round_beat(js_max(MIN_NOTE_LENGTH, dur)),
                ..note.clone()
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Velocity
// ---------------------------------------------------------------------------

pub fn set_velocities(notes: &[RegionNote], indices: &[usize], velocity: f64) -> Vec<RegionNote> {
    let m = mask(indices, notes.len());
    let v = clamp_velocity(velocity);
    notes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            if m[i] {
                RegionNote {
                    velocity: v,
                    ..n.clone()
                }
            } else {
                n.clone()
            }
        })
        .collect()
}

pub fn offset_velocities(notes: &[RegionNote], indices: &[usize], delta: f64) -> Vec<RegionNote> {
    let m = mask(indices, notes.len());
    notes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            if m[i] {
                RegionNote {
                    velocity: clamp_velocity(n.velocity + delta),
                    ..n.clone()
                }
            } else {
                n.clone()
            }
        })
        .collect()
}

/// Linear ramp: every candidate note whose start lies between beat_a and beat_b gets the velocity
/// on the line (beat_a, vel_a) -> (beat_b, vel_b). `candidates: None` = all notes.
pub fn ramp_velocities(
    notes: &[RegionNote],
    candidates: Option<&[usize]>,
    beat_a: f64,
    vel_a: f64,
    beat_b: f64,
    vel_b: f64,
) -> Vec<RegionNote> {
    let only = candidates.map(|c| mask(c, notes.len()));
    let b0 = js_min(beat_a, beat_b);
    let b1 = js_max(beat_a, beat_b);
    notes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            if only.as_ref().is_some_and(|m| !m[i]) {
                return n.clone();
            }
            if n.start_beat < b0 - EPS || n.start_beat > b1 + EPS {
                return n.clone();
            }
            let t = if beat_b == beat_a {
                1.0
            } else {
                (n.start_beat - beat_a) / (beat_b - beat_a)
            };
            RegionNote {
                velocity: clamp_velocity(vel_a + (vel_b - vel_a) * t),
                ..n.clone()
            }
        })
        .collect()
}

/// Default `tolPx` of [`notes_at_x`].
pub const NOTES_AT_X_TOL_PX: f64 = 5.0;

/// Notes whose velocity stem is within `tol_px` of x (px, content coords).
pub fn notes_at_x(
    notes: &[RegionNote],
    x: f64,
    px_per_beat: f64,
    tol_px: f64,
    candidates: Option<&[usize]>,
) -> Vec<usize> {
    notes
        .iter()
        .enumerate()
        .filter(|(i, n)| {
            candidates.is_none_or(|c| c.contains(i))
                && (n.start_beat * px_per_beat - x).abs() <= tol_px
        })
        .map(|(i, _)| i)
        .collect()
}

/// 0..1 velocity -> bucket index for pre-computed colors.
pub fn velocity_bucket(v: f64, buckets: f64) -> f64 {
    clamp(
        js_round(clamp_velocity(v) * (buckets - 1.0)),
        0.0,
        buckets - 1.0,
    )
}
