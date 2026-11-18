//! Grid + ruler generation.

use std::collections::HashMap;

use super::math::{is_multiple_of, js_num, js_round, round_beat};
use super::types::*;

/// Default `minSpacing` (px) of the grid functions.
pub const GRID_MIN_SPACING: f64 = 6.0;
/// Default `minLabelPx` of [`compute_ruler_labels`].
pub const RULER_MIN_LABEL_PX: f64 = 44.0;

/// Subdivision actually drawn for a given snap + zoom: the snap value, coarsened (x2) until lines
/// are at least `min_spacing` px apart. With snap off an adaptive power-of-two subdivision is used.
pub fn grid_subdivision(snap: f64, px_per_beat: f64, min_spacing: f64) -> f64 {
    let mut sub = if snap > 0.0 { snap } else { 0.25 };
    if snap <= 0.0 {
        // Adaptive: finest power-of-two subdivision that is >= 12 px apart.
        sub = 1.0;
        while sub / 2.0 >= 1.0 / 16.0 && (sub / 2.0) * px_per_beat >= 12.0 {
            sub /= 2.0;
        }
    }
    while sub * px_per_beat < min_spacing && sub < BEATS_PER_BAR * 64.0 {
        sub *= 2.0;
    }
    sub
}

/// Vertical grid lines between two region-relative beats (sorted, one per position, strongest
/// level wins).
pub fn compute_grid_lines(
    start_beat: f64,
    end_beat: f64,
    px_per_beat: f64,
    snap: f64,
    offset: f64,
    min_spacing: f64,
) -> Vec<GridLine> {
    if end_beat <= start_beat || px_per_beat <= 0.0 {
        return Vec::new();
    }
    // JS Map keyed by tick, insertion ordered; the final stable sort by beat fixes the order.
    let mut order: Vec<i64> = Vec::new();
    let mut by_tick: HashMap<i64, GridLine> = HashMap::new();
    let mut add = |step: f64, level: u8| {
        let first = ((start_beat + offset) / step - EPS).ceil();
        let last = ((end_beat + offset) / step + EPS).floor();
        let mut k = first;
        while k <= last {
            let beat = round_beat(k * step - offset);
            let key = js_round(beat * TICKS_PER_BEAT) as i64;
            match by_tick.get(&key) {
                None => {
                    order.push(key);
                    by_tick.insert(key, GridLine { beat, level });
                }
                Some(prev) if prev.level > level => {
                    by_tick.insert(key, GridLine { beat, level });
                }
                _ => {}
            }
            k += 1.0;
        }
    };
    // Bars can get too dense when zoomed far out: only keep every Nth bar.
    let mut bar_every = 1.0;
    while bar_every * BEATS_PER_BAR * px_per_beat < min_spacing * 2.0 {
        bar_every *= 2.0;
    }
    add(BEATS_PER_BAR * bar_every, 0);
    if px_per_beat >= min_spacing {
        add(1.0, 1);
    }
    let sub = grid_subdivision(snap, px_per_beat, min_spacing);
    if sub < 1.0 && sub * px_per_beat >= min_spacing {
        add(sub, 2);
    }
    let mut lines: Vec<GridLine> = order.iter().map(|k| by_tick[k]).collect();
    lines.sort_by(|a, b| (a.beat - b.beat).partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal));
    lines
}

/// Ruler labels: bar numbers (and bar.beat when zoomed in) at least `min_label_px` apart.
pub fn compute_ruler_labels(
    start_beat: f64,
    end_beat: f64,
    px_per_beat: f64,
    offset: f64,
    min_label_px: f64,
) -> Vec<RulerLabel> {
    let mut labels = Vec::new();
    if end_beat <= start_beat || px_per_beat <= 0.0 {
        return labels;
    }
    let bar_px = BEATS_PER_BAR * px_per_beat;
    let show_beats = px_per_beat >= min_label_px;
    let mut bar_every = 1.0;
    while bar_every * bar_px < min_label_px {
        bar_every *= 2.0;
    }
    let step = if show_beats { 1.0 } else { BEATS_PER_BAR * bar_every };
    let first = ((start_beat + offset) / step - EPS).ceil();
    let last = ((end_beat + offset) / step + EPS).floor();
    let mut k = first;
    while k <= last {
        let abs = k * step;
        let bar = (abs / BEATS_PER_BAR + EPS).floor() + 1.0;
        let major = is_multiple_of(abs, BEATS_PER_BAR);
        let beat_in_bar = js_round(abs - (bar - 1.0) * BEATS_PER_BAR) + 1.0;
        labels.push(RulerLabel {
            beat: round_beat(abs - offset),
            label: if major { js_num(bar) } else { format!("{}.{}", js_num(bar), js_num(beat_in_bar)) },
            major,
        });
        k += 1.0;
    }
    labels
}
