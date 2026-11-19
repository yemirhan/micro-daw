//! Hit-testing & selection.

use super::layout::RowLayout;
use super::math::{js_max, js_min, y_to_row};
use super::types::*;
use crate::types::RegionNote;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HitTestOptions<'a> {
    /// Width of the resize handle zone at each end, px.
    pub edge_px: f64,
    /// Minimum drawn width of a note, px (short notes are still grabbable).
    pub min_width_px: f64,
    /// Drum notes / one-shots can't be resized.
    pub resizable: bool,
    /// Selected notes are drawn on top, so they win hit-tests.
    pub selected: Option<&'a [usize]>,
}

impl Default for HitTestOptions<'_> {
    fn default() -> Self {
        Self {
            edge_px: 6.0,
            min_width_px: 4.0,
            resizable: true,
            selected: None,
        }
    }
}

/// Default `minWidthPx` of [`note_visual_width`].
pub const NOTE_MIN_WIDTH_PX: f64 = 4.0;

pub fn note_visual_width(note: &RegionNote, px_per_beat: f64, min_width_px: f64) -> f64 {
    js_max(note.duration_beats * px_per_beat, min_width_px)
}

/// Find the note under grid-content coordinates (x, y in px, relative to the content origin), and
/// which part of it (start edge / body / end edge).
pub fn hit_test_note(
    notes: &[RegionNote],
    x: f64,
    y: f64,
    layout: &RowLayout,
    px_per_beat: f64,
    row_height: f64,
    opts: &HitTestOptions<'_>,
) -> Option<NoteHit> {
    let row = y_to_row(y, row_height);
    if row < 0.0 || row >= layout.row_count as f64 {
        return None;
    }
    let mut best: Option<NoteHit> = None;
    let mut best_selected = false;
    for i in (0..notes.len()).rev() {
        let n = &notes[i];
        if layout.pitch_to_row(n.note) != row {
            continue;
        }
        let x0 = n.start_beat * px_per_beat;
        let w = note_visual_width(n, px_per_beat, opts.min_width_px);
        if x < x0 || x > x0 + w {
            continue;
        }
        let is_sel = opts.selected.is_some_and(|s| s.contains(&i));
        if best.is_some() && (best_selected || !is_sel) {
            continue;
        }
        let mut edge = NoteEdge::Body;
        if opts.resizable {
            let zone = js_min(opts.edge_px, w / 3.0);
            if x >= x0 + w - zone {
                edge = NoteEdge::End;
            } else if x <= x0 + zone {
                edge = NoteEdge::Start;
            }
        }
        best = Some(NoteHit { index: i, edge });
        best_selected = is_sel;
        if is_sel {
            break;
        }
    }
    best
}

/// Indices of notes intersecting a beat/row rectangle (inclusive rows).
pub fn notes_in_rect(
    notes: &[RegionNote],
    beat_a: f64,
    beat_b: f64,
    row_a: f64,
    row_b: f64,
    layout: &RowLayout,
) -> Vec<usize> {
    let b0 = js_min(beat_a, beat_b);
    let b1 = js_max(beat_a, beat_b);
    let r0 = js_min(row_a, row_b);
    let r1 = js_max(row_a, row_b);
    let mut out = Vec::new();
    for (i, n) in notes.iter().enumerate() {
        let row = layout.pitch_to_row(n.note);
        if row < r0 || row > r1 {
            continue;
        }
        let end = n.start_beat + js_max(n.duration_beats, EPS);
        if n.start_beat <= b1 && end >= b0 {
            out.push(i);
        }
    }
    out
}

/// Shift-click semantics: toggle membership. Plain click: select just this one (unless already
/// selected). `current` and the result are in JS `Set` insertion order.
pub fn apply_click_selection(current: &[usize], index: usize, toggle: bool) -> Vec<usize> {
    let mut next = dedup(current);
    if toggle {
        if let Some(pos) = next.iter().position(|&i| i == index) {
            next.remove(pos);
        } else {
            next.push(index);
        }
        return next;
    }
    if next.contains(&index) {
        return next;
    }
    vec![index]
}

/// `[...new Set(xs)]`: first occurrence wins, order kept.
pub(crate) fn dedup(xs: &[usize]) -> Vec<usize> {
    let mut out: Vec<usize> = Vec::with_capacity(xs.len());
    for &x in xs {
        if !out.contains(&x) {
            out.push(x);
        }
    }
    out
}
