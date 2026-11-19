//! Waveform drawing shared by the Song view's audio regions (`WaveformMinimap`): cached peak
//! tiles, mirrored. (The Samples page itself is the Sound Designer: `ui::p9::designer`.)

use rune::{Color, El, Shape, canvas};

use crate::app::App;
use crate::theme::t;

/// `WaveformMinimap` of an audio region: `max(4, round(length·ppb))` columns of the visible part
/// of the sample (from the region's offset, `length / bpm · 60` seconds), peak over channels,
/// mirrored, in `color-mix(in oklab, color 75%, white)` at 0.9, inset 2 points (the canvas is
/// `width − 4` wide, so the columns are squeezed into it like the TS canvas). Only the columns
/// inside the lanes' viewport are drawn (`x` = the region's left edge in viewport points,
/// `vw` = the viewport width); their peaks come from the cached tiles.
#[allow(clippy::too_many_arguments)]
pub fn region_waveform(
    s: &App,
    audio: &mdaw_model::types::AudioRegionData,
    length_beats: f64,
    ppb: f64,
    color: Color,
    h: f32,
    x: f32,
    vw: f32,
) -> Option<El> {
    let b = s.p9.samples.buffer(&audio.sample_id)?;
    let bpm = s.svc.engine.doc().get_bpm();
    let w = (length_beats * ppb).round().max(4.0) as usize;
    let sr = b.sample_rate as f64;
    let dur = length_beats / bpm * 60.0;
    let start = (audio.offset_seconds * sr).floor() as usize;
    let end = (((audio.offset_seconds + dur) * sr).floor() as usize).min(b.frames());
    if end <= start {
        return None;
    }
    let ch = (h - 4.0).max(1.0);
    let mid = ch / 2.0;
    let sx = (w as f32 - 4.0).max(1.0) / w as f32;
    // Columns on screen (column i is drawn at 2 + i·sx from the region's left edge).
    let c0 = (((-x - 2.0) / sx).floor().max(0.0) as usize).min(w);
    let c1 = ((((vw - x - 2.0) / sx).ceil() + 1.0).max(0.0) as usize).min(w);
    if c1 <= c0 {
        return None;
    }
    let peaks = tiled_peaks(s, &audio.sample_id, start, end, w, c0, c1);
    let fill = crate::ui::song::cmix(color, t().white, 0.75).alpha(0.9);
    let mut shape = Shape::new();
    for (from, to) in runs(&peaks, c0, c1) {
        shape = shape.fill(
            mirrored(&peaks, from, to, |i| i as f32 * sx, mid, mid),
            fill,
        );
    }
    Some(canvas(shape).absolute(2.0, 2.0).size(w as f32 - 4.0, ch))
}

/// Peaks of columns `[c0, c1)` of `cols` columns over frames `[start, end)` of `id` (indexed
/// by column; None where the tile is still being computed).
fn tiled_peaks(
    s: &App,
    id: &str,
    start: usize,
    end: usize,
    cols: usize,
    c0: usize,
    c1: usize,
) -> Vec<Option<f32>> {
    let mut out = vec![None; cols];
    for k in s.p9.samples.peak_keys(id, start, end, cols, c0, c1) {
        if let Some(p) = s.p9.samples.peak_tile(&k) {
            let base = k.tile * crate::samples::PEAK_TILE;
            for (i, v) in p.iter().enumerate() {
                if let Some(o) = out.get_mut(base + i) {
                    *o = Some(*v);
                }
            }
        }
    }
    out
}

/// Runs of ready columns inside `[c0, c1)`.
fn runs(peaks: &[Option<f32>], c0: usize, c1: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = c0;
    while i < c1.min(peaks.len()) {
        if peaks[i].is_none() {
            i += 1;
            continue;
        }
        let from = i;
        while i < c1.min(peaks.len()) && peaks[i].is_some() {
            i += 1;
        }
        out.push((from, i));
    }
    out
}

/// The mirrored polygon of columns `[from, to)` around `mid` (`amp` = the half height of a
/// full-scale peak), column `i` at `x(i)`.
fn mirrored(
    peaks: &[Option<f32>],
    from: usize,
    to: usize,
    x: impl Fn(usize) -> f32,
    mid: f32,
    amp: f32,
) -> Vec<[f32; 2]> {
    let mut pts = vec![[x(from), mid]];
    let cols = peaks[from..to].iter().enumerate();
    for (k, p) in cols.clone() {
        pts.push([x(from + k), mid - p.unwrap_or(0.0) * amp]);
    }
    for (k, p) in cols.rev() {
        pts.push([x(from + k), mid + p.unwrap_or(0.0) * amp]);
    }
    pts
}

/// The waveform of a take being recorded: `peaks` are columns of `frames` frames each at
/// `rate`; drawn at `pps` points per second, mirrored, `h` high, from `x0` (the block's left
/// edge in viewport points) clipped to a viewport `vw` wide.
#[allow(clippy::too_many_arguments)]
pub fn live_take_waveform(
    peaks: &[f32],
    frames: usize,
    rate: u32,
    pps: f64,
    color: Color,
    h: f32,
    x0: f32,
    vw: f32,
) -> Option<El> {
    if peaks.is_empty() {
        return None;
    }
    // Points per peak column.
    let step = (frames as f64 / rate.max(1) as f64 * pps) as f32;
    if step <= 0.0 {
        return None;
    }
    let from = ((-x0 / step).floor().max(0.0) as usize).min(peaks.len());
    let to = ((((vw - x0) / step).ceil() + 1.0).max(0.0) as usize).min(peaks.len());
    if to <= from {
        return None;
    }
    // At most one polygon point per point of width.
    let stride = ((1.0 / step).ceil() as usize).max(1);
    let cols: Vec<Option<f32>> = peaks[from..to]
        .chunks(stride)
        .map(|c| Some(c.iter().fold(0f32, |m, v| m.max(*v)).min(1.0)))
        .collect();
    let mid = h / 2.0;
    let pts = mirrored(
        &cols,
        0,
        cols.len(),
        |i| (from + i * stride) as f32 * step,
        mid,
        mid,
    );
    let w = peaks.len() as f32 * step;
    Some(
        canvas(Shape::new().fill(pts, color))
            .absolute(0.0, 0.0)
            .size(w.max(1.0), h),
    )
}
