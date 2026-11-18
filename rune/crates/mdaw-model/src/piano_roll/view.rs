//! View helpers: region length, pitch range, centring, fit, follow.

use super::layout::{LayoutKind, RowLayout};
use super::math::{clamp, js_max, js_min, js_round};
use super::types::*;
use crate::types::RegionNote;

/// Region length needed to contain every note (rounded up to a bar), never shrinks.
pub fn required_region_length(notes: &[RegionNote], current_length: f64) -> f64 {
    let mut end = 0.0;
    for n in notes {
        end = js_max(end, n.start_beat + n.duration_beats);
    }
    if end <= current_length + EPS {
        return current_length;
    }
    (end / BEATS_PER_BAR - EPS).ceil() * BEATS_PER_BAR
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PitchRange {
    pub lo: f64,
    pub hi: f64,
}

/// Pitch range [lo, hi] of the notes, or None.
pub fn pitch_range(notes: &[RegionNote]) -> Option<PitchRange> {
    if notes.is_empty() {
        return None;
    }
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for n in notes {
        lo = js_min(lo, n.note);
        hi = js_max(hi, n.note);
    }
    Some(PitchRange { lo, hi })
}

const DEFAULT_RANGE: PitchRange = PitchRange { lo: 60.0, hi: 72.0 };

/// Vertical scroll (px) that centres the notes (or C4 when empty) in a viewport of `view_height`.
pub fn center_scroll_top(
    notes: &[RegionNote],
    layout: &RowLayout,
    row_height: f64,
    view_height: f64,
) -> f64 {
    if layout.kind == LayoutKind::Drum {
        return 0.0;
    }
    let range = pitch_range(notes).unwrap_or(DEFAULT_RANGE);
    let row_top = layout.pitch_to_row(js_min(f64::from(PR_NOTE_MAX), range.hi));
    let row_bottom = layout.pitch_to_row(js_max(f64::from(PR_NOTE_MIN), range.lo));
    let center = ((row_top + row_bottom + 1.0) / 2.0) * row_height;
    let max = js_max(0.0, layout.row_count as f64 * row_height - view_height);
    clamp(js_round(center - view_height / 2.0), 0.0, max)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FitView {
    pub px_per_beat: f64,
    pub row_height: f64,
    pub scroll_left: f64,
    pub scroll_top: f64,
}

/// "Fit": zoom so the region (or notes, whichever is longer) fills the width and the note range
/// fills the height (within the row-height limits).
pub fn fit_view(
    notes: &[RegionNote],
    layout: &RowLayout,
    region_length: f64,
    view_width: f64,
    view_height: f64,
) -> FitView {
    let mut end = region_length;
    for n in notes {
        end = js_max(end, n.start_beat + n.duration_beats);
    }
    let px_per_beat = clamp(
        (view_width - 16.0) / js_max(1.0, end),
        PR_MIN_PX_PER_BEAT,
        PR_MAX_PX_PER_BEAT,
    );
    let row_height = if layout.kind == LayoutKind::Drum {
        clamp(
            (view_height / layout.row_count as f64).floor(),
            PR_MIN_ROW_HEIGHT,
            PR_MAX_ROW_HEIGHT,
        )
    } else {
        let range = pitch_range(notes).unwrap_or(DEFAULT_RANGE);
        let rows = range.hi - range.lo + 1.0 + 4.0; // two rows of padding each side
        clamp((view_height / rows).floor(), PR_MIN_ROW_HEIGHT, 24.0)
    };
    FitView {
        px_per_beat,
        row_height,
        scroll_left: 0.0,
        scroll_top: center_scroll_top(notes, layout, row_height, view_height),
    }
}

/// Default `margin` of [`follow_scroll`].
pub const FOLLOW_MARGIN: f64 = 0.1;

/// Where the "page" should scroll to so the playhead stays visible while following.
pub fn follow_scroll(
    playhead_x: f64,
    scroll_left: f64,
    view_width: f64,
    margin: f64,
) -> Option<f64> {
    if view_width <= 0.0 {
        return None;
    }
    if playhead_x < scroll_left || playhead_x > scroll_left + view_width * (1.0 - margin / 2.0) {
        return Some(js_max(0.0, playhead_x - view_width * margin));
    }
    None
}
