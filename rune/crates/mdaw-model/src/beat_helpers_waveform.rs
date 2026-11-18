//! `src/components/beat/waveform.ts`: waveform peaks (pure), the per-buffer peak cache, and the
//! pure geometry of `prepareCanvas` / `drawPeaks` (backing-store size, bar rectangles).

use std::collections::HashMap;
use std::rc::Rc;

use super::BufferLike;
use crate::js::{max as js_max, min as js_min, round as js_round};

/// Per column: min and max sample in -1..1 (`Float32Array`s in TS).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Peaks {
    pub min: Vec<f32>,
    pub max: Vec<f32>,
}

/// `computePeaks(buffer, columns)`: min/max per column over all channels, striding through very
/// long columns (`floor(span / 512)`). `columns` is floored and at least 1.
pub fn compute_peaks(buffer: &dyn BufferLike, columns: f64) -> Peaks {
    let cols = js_max(1.0, columns.floor());
    if cols.is_nan() {
        // `new Float32Array(NaN)` is empty.
        return Peaks::default();
    }
    let cols = cols as usize;
    let mut min = vec![0f32; cols];
    let mut max = vec![0f32; cols];
    let len = buffer.length();
    if len == 0 {
        return Peaks { min, max };
    }
    let channels: Vec<&[f32]> = (0..buffer.number_of_channels().max(1)).map(|c| buffer.channel_data(c)).collect();
    for col in 0..cols {
        let from = (col as u128 * len as u128 / cols as u128) as usize;
        let to = (from + 1).max(((col as u128 + 1) * len as u128 / cols as u128) as usize);
        let mut lo = 1f64;
        let mut hi = -1f64;
        for data in &channels {
            let stride = ((to - from) / 512).max(1);
            let mut i = from;
            while i < to && i < data.len() {
                let v = data[i] as f64;
                if v < lo {
                    lo = v;
                }
                if v > hi {
                    hi = v;
                }
                i += stride;
            }
        }
        min[col] = if lo > hi { 0.0 } else { lo as f32 };
        max[col] = if lo > hi { 0.0 } else { hi as f32 };
    }
    Peaks { min, max }
}

/// `getPeaks(buffer, columns)`: peaks cached per buffer and resolution. TS keys a `WeakMap` by
/// buffer identity; here the caller passes a stable buffer key (and calls [`PeakCache::forget`]
/// when the buffer goes away).
#[derive(Debug, Default)]
pub struct PeakCache {
    by_buffer: HashMap<u64, HashMap<usize, Rc<Peaks>>>,
}

impl PeakCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_peaks(&mut self, buffer_key: u64, buffer: &dyn BufferLike, columns: f64) -> Rc<Peaks> {
        let cols = js_max(1.0, columns.floor());
        let cols = if cols.is_nan() { 0 } else { cols as usize };
        self.by_buffer
            .entry(buffer_key)
            .or_default()
            .entry(cols)
            .or_insert_with(|| Rc::new(compute_peaks(buffer, cols as f64)))
            .clone()
    }

    pub fn forget(&mut self, buffer_key: u64) {
        self.by_buffer.remove(&buffer_key);
    }
}

/// Backing-store size computed by `prepareCanvas`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CanvasBacking {
    pub width: f64,
    pub height: f64,
    /// Transform scale (`setTransform(dpr, 0, 0, dpr, 0, 0)`).
    pub dpr: f64,
}

/// `prepareCanvas` sizing: `dpr = max(1, devicePixelRatio || 1)` (None = no window),
/// backing store = `max(1, round(css × dpr))`.
pub fn canvas_backing(css_w: f64, css_h: f64, device_pixel_ratio: Option<f64>) -> CanvasBacking {
    let dpr = match device_pixel_ratio {
        Some(r) if r != 0.0 && !r.is_nan() => js_max(1.0, r),
        Some(_) => 1.0,
        None => 1.0,
    };
    CanvasBacking { width: js_max(1.0, js_round(css_w * dpr)), height: js_max(1.0, js_round(css_h * dpr)), dpr }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// `drawPeaks` geometry: one mirrored min/max bar per column filling `bx`, at least 1px tall and
/// `colW - 0.25` (≥ 1) wide. Fill each with the peak color.
pub fn peak_rects(peaks: &Peaks, bx: Rect) -> Vec<Rect> {
    let cols = peaks.min.len();
    let mid = bx.y + bx.h / 2.0;
    let half = bx.h / 2.0;
    let col_w = bx.w / cols as f64;
    (0..cols)
        .map(|i| {
            let top = mid - js_max(0.5 / half, peaks.max[i] as f64) * half;
            let bottom = mid - js_min(-0.5 / half, peaks.min[i] as f64) * half;
            Rect { x: bx.x + i as f64 * col_w, y: top, w: js_max(1.0, col_w - 0.25), h: js_max(1.0, bottom - top) }
        })
        .collect()
}
