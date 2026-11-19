//! Shared test helpers: a click-track render closure and onset / tempo measurement.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{SharedRender, shared_render};

/// A render callback writing a unit impulse (both channels) at context frames `k · period`,
/// plus a counter of context frames rendered.
pub(crate) fn click_render(period: u64) -> (SharedRender, Arc<AtomicU64>) {
    let frames = Arc::new(AtomicU64::new(0));
    let f = frames.clone();
    let mut pos = 0u64;
    let render = shared_render(move |l, r, _| {
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            if pos.is_multiple_of(period) {
                *a = 1.0;
                *b = 1.0;
            }
            pos += 1;
        }
        f.fetch_add(l.len() as u64, Ordering::Relaxed);
    });
    (render, frames)
}

/// Sub-sample positions of clicks: the peak of |x| (parabolic interpolation) in each region
/// above `threshold`, regions separated by at least `gap` samples.
pub(crate) fn onsets(x: &[f32], threshold: f32, gap: usize) -> Vec<f64> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < x.len() {
        if x[i].abs() <= threshold {
            i += 1;
            continue;
        }
        let end = (i + gap).min(x.len());
        let peak = (i..end)
            .max_by(|&a, &b| x[a].abs().total_cmp(&x[b].abs()))
            .unwrap();
        let mut p = peak as f64;
        if peak > 0 && peak + 1 < x.len() {
            let (a, b, c) = (
                x[peak - 1].abs() as f64,
                x[peak].abs() as f64,
                x[peak + 1].abs() as f64,
            );
            let den = a - 2.0 * b + c;
            if den.abs() > 1e-12 {
                p += 0.5 * (a - c) / den;
            }
        }
        out.push(p);
        i = end;
    }
    out
}

/// Tempo (BPM, one click per beat) from click positions at `rate`.
pub(crate) fn bpm(onsets: &[f64], rate: u32) -> f64 {
    assert!(onsets.len() >= 2, "need two clicks, got {}", onsets.len());
    let span = onsets[onsets.len() - 1] - onsets[0];
    60.0 * rate as f64 * (onsets.len() - 1) as f64 / span
}

/// Left channel of an interleaved stereo buffer.
pub(crate) fn left(interleaved: &[f32]) -> Vec<f32> {
    interleaved
        .as_chunks::<2>()
        .0
        .iter()
        .map(|f| f[0])
        .collect()
}
