//! Sample chopping helpers (pure, no audio context needed). Port of `src/audio/sampler/slicer.ts`.
//!
//! Transient detection: short-frame energy envelope in dB; an onset is a frame whose level rises
//! sharply above the recent floor (local maximum of the rise), above a gate relative to the
//! loudest frame. The onset is then refined to the first sample that reaches a fraction of the
//! hit's peak.
//!
//! Float32 semantics are kept: sample data and the dB / rise envelopes are `f32`, all arithmetic
//! on them happens in `f64` like JS.

use std::cmp::Ordering;

use crate::js;

/// A slice `[start_sec, end_sec)` (`{ startSec, endSec }`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SliceRange {
    pub start_sec: f64,
    pub end_sec: f64,
}

/// `TransientOptions` with the TS defaults filled in (`Default`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransientOptions {
    /// 0..1 — higher finds quieter / softer hits. Default 0.5.
    pub sensitivity: f64,
    /// Upper bound on returned slices (including the one at 0). Default 16.
    pub max_slices: f64,
    /// Minimum distance between slices in seconds. Default 0.05.
    pub min_gap_sec: f64,
}

impl Default for TransientOptions {
    fn default() -> Self {
        Self {
            sensitivity: 0.5,
            max_slices: 16.0,
            min_gap_sec: 0.05,
        }
    }
}

/// `toMono(buffer)` for an `AudioBuffer`-like: channel 0 as-is for mono, else the `f32` average of
/// the channels over `length` frames (`out[i] += data[i] / n`; reading past a channel is NaN).
pub fn to_mono(channels: &[&[f32]], length: usize) -> Vec<f32> {
    let n = channels.len();
    if n <= 1 {
        return channels.first().map_or_else(Vec::new, |c| c.to_vec());
    }
    let mut out = vec![0f32; length];
    for data in channels {
        for (i, o) in out.iter_mut().enumerate() {
            let v = data.get(i).map_or(f64::NAN, |&x| x as f64);
            *o = (*o as f64 + v / n as f64) as f32;
        }
    }
    out
}

/// `detectTransients(data, sampleRate, opts)` on mono data: slice start times in seconds,
/// ascending; the first is always 0.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn detect_transients(data: &[f32], sample_rate: f64, opts: TransientOptions) -> Vec<f64> {
    let sensitivity = js::clamp(opts.sensitivity, 0.0, 1.0);
    let max_slices = js::max(1.0, opts.max_slices.floor());
    let min_gap = js::max(0.0, opts.min_gap_sec);
    // `!(sampleRate > 0)`: also true for NaN.
    if data.is_empty() || !(sample_rate > 0.0) || max_slices == 1.0 {
        return vec![0.0];
    }

    let hop = js::max(32.0, js::round(sample_rate * 0.005)) as usize;
    let win = hop * 2;
    let frames = (data.len() - 1) / hop + 1;
    let mut db = vec![0f32; frames];
    let mut max_db = f64::NEG_INFINITY;
    for (f, slot) in db.iter_mut().enumerate() {
        let start = f * hop;
        let end = data.len().min(start + win);
        let mut sum = 0f64;
        for &x in &data[start..end] {
            sum += x as f64 * x as f64;
        }
        let v = 10.0 * (sum / (end - start).max(1) as f64 + 1e-12).log10();
        *slot = v as f32;
        if v > max_db {
            max_db = v;
        }
    }
    if max_db < -100.0 {
        return vec![0.0];
    }

    let rise_threshold = 14.0 - 11.0 * sensitivity; // dB above the recent floor
    let gate = max_db - (24.0 + 36.0 * sensitivity); // ignore frames this far below the loudest
    let lookback = 3;
    let mut rise = vec![0f32; frames];
    for f in 0..frames {
        let mut floor = f64::INFINITY;
        for k in 1..=lookback {
            floor = js::min(floor, if f >= k { db[f - k] as f64 } else { -120.0 });
        }
        rise[f] = (db[f] as f64 - floor) as f32;
    }

    let mut candidates: Vec<(f64, f64)> = Vec::new(); // (time, strength)
    for f in 0..frames {
        if (db[f] as f64) < gate || (rise[f] as f64) < rise_threshold {
            continue;
        }
        // Local maximum of the rise (first one wins on plateaus).
        let mut is_peak = true;
        for k in -2i64..=2 {
            if !is_peak {
                break;
            }
            let g = f as i64 + k;
            if k == 0 || g < 0 || g >= frames as i64 {
                continue;
            }
            let (rg, rf) = (rise[g as usize], rise[f]);
            if if k < 0 { rg >= rf } else { rg > rf } {
                is_peak = false;
            }
        }
        if !is_peak {
            continue;
        }
        candidates.push((
            refine_onset(data, f, hop, win) as f64 / sample_rate,
            rise[f] as f64,
        ));
    }

    // Strongest first (stable), honouring the minimum gap (a hit right at the start is slice 0).
    candidates.sort_by(|a, b| (b.1 - a.1).partial_cmp(&0.0).unwrap_or(Ordering::Equal));
    let mut accepted = vec![0.0];
    for (time, _) in candidates {
        if accepted.len() as f64 >= max_slices {
            break;
        }
        if accepted
            .iter()
            .all(|t| (t - time).abs() >= js::max(min_gap, 1e-6))
        {
            accepted.push(time);
        }
    }
    accepted.sort_by(|a, b| (a - b).partial_cmp(&0.0).unwrap_or(Ordering::Equal));
    accepted
}

/// `refineOnset`: first sample around frame `f` whose level reaches 10% of the hit's peak.
fn refine_onset(data: &[f32], f: usize, hop: usize, win: usize) -> usize {
    let from = f.saturating_sub(1) * hop;
    let to = data.len().min(f * hop + win);
    let mut peak = 0f64;
    for &x in &data[from..to] {
        peak = js::max(peak, (x as f64).abs());
    }
    let threshold = peak * 0.1;
    for (i, &x) in data.iter().enumerate().take(to).skip(from) {
        if (x as f64).abs() >= threshold {
            return i;
        }
    }
    f * hop
}

/// `equalSlices(durationSec, count)`: `count` equal slices, `[0, d/n, 2d/n, ...]`.
pub fn equal_slices(duration_sec: f64, count: f64) -> Vec<f64> {
    let n = js::max(1.0, count.floor()) as usize;
    let d = js::max(0.0, duration_sec);
    (0..n).map(|i| (d * i as f64) / n as f64).collect()
}

/// `slicesToRanges(starts, durationSec)`: `[start, next start)`; the last ends at the duration
/// (what "chop to pads" assigns as trim points).
pub fn slices_to_ranges(starts: &[f64], duration_sec: f64) -> Vec<SliceRange> {
    let mut sorted = starts.to_vec();
    sorted.sort_by(|a, b| (a - b).partial_cmp(&0.0).unwrap_or(Ordering::Equal));
    (0..sorted.len())
        .map(|i| SliceRange {
            start_sec: sorted[i],
            end_sec: if i + 1 < sorted.len() {
                sorted[i + 1]
            } else {
                duration_sec
            },
        })
        .collect()
}

/// Port of `src/audio/sampler/slicer.test.ts`.
#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 44100.0;

    /// Deterministic PRNG (the TS LCG) so the noise is the same on every run.
    fn rng(seed: u32) -> impl FnMut() -> f64 {
        let mut s = seed;
        move || {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            s as f64 / 4_294_967_296.0 - 0.5
        }
    }

    /// Decaying noise bursts ("drum hits") at `(time, amp)` over a quiet noise floor.
    fn synth_floor(duration_sec: f64, hits: &[(f64, f64)], floor: f64) -> Vec<f32> {
        let mut out = vec![0f32; js::round(duration_sec * SR) as usize];
        let mut r = rng(7);
        for o in out.iter_mut() {
            *o = (r() * 2.0 * floor) as f32;
        }
        for &(t, amp) in hits {
            let start = js::round(t * SR) as usize;
            let mut i = 0usize;
            while (i as f64) < 0.15 * SR && start + i < out.len() {
                let v = out[start + i] as f64 + amp * r() * 2.0 * (-(i as f64) / (0.03 * SR)).exp();
                out[start + i] = v as f32;
                i += 1;
            }
        }
        out
    }

    fn synth(duration_sec: f64, hits: &[(f64, f64)]) -> Vec<f32> {
        synth_floor(duration_sec, hits, 0.0005)
    }

    fn hits(times: &[f64]) -> Vec<(f64, f64)> {
        times.iter().map(|&t| (t, 0.8)).collect()
    }

    fn near(actual: &[f64], expected: &[f64]) {
        assert_eq!(actual.len(), expected.len(), "{actual:?} vs {expected:?}");
        for (a, e) in actual.iter().zip(expected) {
            assert!((a - e).abs() < 0.005, "{actual:?} vs {expected:?}");
        }
    }

    fn opts(sensitivity: f64, max_slices: f64, min_gap_sec: f64) -> TransientOptions {
        TransientOptions {
            sensitivity,
            max_slices,
            min_gap_sec,
        }
    }

    #[test]
    fn finds_evenly_spaced_clicks() {
        let data = synth(2.0, &hits(&[0.0, 0.5, 1.0, 1.5]));
        near(
            &detect_transients(&data, SR, TransientOptions::default()),
            &[0.0, 0.5, 1.0, 1.5],
        );
    }

    #[test]
    fn always_starts_with_0_even_when_the_first_hit_is_later() {
        let data = synth(1.5, &hits(&[0.25, 0.8]));
        near(
            &detect_transients(&data, SR, TransientOptions::default()),
            &[0.0, 0.25, 0.8],
        );
    }

    #[test]
    fn accepts_an_audio_buffer_like_mixing_channels_down() {
        let data = synth(1.0, &hits(&[0.0, 0.4]));
        let mono = to_mono(&[&data, &data], data.len());
        near(
            &detect_transients(&mono, SR, TransientOptions::default()),
            &[0.0, 0.4],
        );
        assert_eq!(to_mono(&[&data], data.len()), data);
        assert!(to_mono(&[], 4).is_empty());
    }

    #[test]
    fn sensitivity_controls_whether_quiet_hits_count() {
        let data = synth(1.5, &[(0.0, 0.8), (0.5, 0.8), (1.0, 0.01)]);
        near(
            &detect_transients(&data, SR, opts(0.0, 16.0, 0.05)),
            &[0.0, 0.5],
        );
        near(
            &detect_transients(&data, SR, opts(1.0, 16.0, 0.05)),
            &[0.0, 0.5, 1.0],
        );
    }

    #[test]
    fn keeps_the_strongest_hits_when_capped_by_max_slices() {
        let data = synth(2.0, &[(0.0, 0.8), (0.4, 0.1), (0.8, 0.9), (1.2, 0.2)]);
        near(
            &detect_transients(&data, SR, opts(0.8, 2.0, 0.05)),
            &[0.0, 0.8],
        );
    }

    #[test]
    fn merges_hits_closer_than_min_gap_sec() {
        let data = synth(1.0, &hits(&[0.0, 0.3, 0.33]));
        assert_eq!(detect_transients(&data, SR, opts(0.5, 16.0, 0.1)).len(), 2);
    }

    #[test]
    fn returns_0_for_silence_and_empty_input() {
        assert_eq!(
            detect_transients(&vec![0f32; SR as usize], SR, TransientOptions::default()),
            [0.0]
        );
        assert_eq!(
            detect_transients(&[], SR, TransientOptions::default()),
            [0.0]
        );
    }

    #[test]
    fn equal_slices_split_evenly() {
        assert_eq!(equal_slices(2.0, 4.0), [0.0, 0.5, 1.0, 1.5]);
        assert_eq!(equal_slices(1.0, 0.0), [0.0]);
    }

    #[test]
    fn slices_to_ranges_end_at_the_next_start() {
        let r = |start_sec, end_sec| SliceRange { start_sec, end_sec };
        assert_eq!(
            slices_to_ranges(&[0.0, 0.5, 1.0], 2.0),
            [r(0.0, 0.5), r(0.5, 1.0), r(1.0, 2.0)]
        );
    }
}
