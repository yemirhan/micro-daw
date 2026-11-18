//! `src/components/beat/chop.ts` (slice ranges of a pad's trimmed sample), with a private port of
//! the slicer it calls (`src/audio/sampler/slicer.ts`: `detectTransients`, `equalSlices`,
//! `slicesToRanges`), and the pure half of `missingPackActions.ts` (`planMissingPadFix`).
//!
//! Float32 semantics are kept: sample data and the slicer's dB / rise envelopes are `f32`, all
//! arithmetic on them happens in `f64` like JS.

use super::SliceRange;
use super::js::{FACTORY_PAD_VOICES, factory_voice_label, js_clamp, js_max, js_min};
use crate::loop_settings::js_round;
use crate::types::{PAD_COUNT, PadIndex, PadSample, PadSampleSource};

/// Minimal `AudioBuffer` surface (chop.ts `BufferLike`, waveform.ts `BufferLike`).
pub trait BufferLike {
    fn number_of_channels(&self) -> usize;
    /// Frames.
    fn length(&self) -> usize;
    fn sample_rate(&self) -> f64;
    fn channel_data(&self, channel: usize) -> &[f32];
}

/// A plain in-memory buffer (channels of equal length).
#[derive(Debug, Clone, PartialEq)]
pub struct SampleBuffer {
    pub channels: Vec<Vec<f32>>,
    pub sample_rate: f64,
}

impl BufferLike for SampleBuffer {
    fn number_of_channels(&self) -> usize {
        self.channels.len()
    }
    fn length(&self) -> usize {
        self.channels.first().map_or(0, Vec::len)
    }
    fn sample_rate(&self) -> f64 {
        self.sample_rate
    }
    fn channel_data(&self, channel: usize) -> &[f32] {
        self.channels.get(channel).map_or(&[], Vec::as_slice)
    }
}

/// `ChopMode`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChopMode {
    Transients {
        sensitivity: f64,
    },
    /// 4, 8 or 16.
    Equal {
        count: usize,
    },
}

/// Trim of `chopRanges` (`endSec: null` = to the end).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChopTrim {
    pub start_sec: f64,
    pub end_sec: Option<f64>,
}

/// `monoRange`: mono mix of `[from, to)` samples.
fn mono_range(buffer: &dyn BufferLike, from: usize, to: usize) -> Vec<f32> {
    let n = to.saturating_sub(from);
    let ch = buffer.number_of_channels().max(1);
    if ch == 1 {
        // `subarray(from, to)` clamps to the data.
        let data = buffer.channel_data(0);
        let a = from.min(data.len());
        let b = to.min(data.len()).max(a);
        return data[a..b].to_vec();
    }
    let mut out = vec![0f32; n];
    for c in 0..ch {
        let data = buffer.channel_data(c);
        for (i, o) in out.iter_mut().enumerate() {
            let v = data.get(from + i).map_or(f64::NAN, |&x| x as f64);
            *o = (*o as f64 + v / ch as f64) as f32;
        }
    }
    out
}

/// `chopRanges(buffer, trim, mode, fromPad = 0)`: slice ranges (absolute buffer seconds) of the
/// trimmed part of a sample, at most as many as there are pads from `fromPad` to the last pad.
pub fn chop_ranges(buffer: &dyn BufferLike, trim: ChopTrim, mode: ChopMode, from_pad: PadIndex) -> Vec<SliceRange> {
    let sr = buffer.sample_rate();
    let total = buffer.length() as f64 / sr;
    // `trim.startSec || 0`: NaN / 0 → 0.
    let start_sec = if trim.start_sec.is_nan() { 0.0 } else { trim.start_sec };
    let start = js_min(total, js_max(0.0, start_sec));
    let end = js_min(total, js_max(start, trim.end_sec.unwrap_or(total)));
    let dur = end - start;
    if !(dur > 0.0) {
        return Vec::new();
    }
    let max_slices = js_max(1.0, PAD_COUNT as f64 - from_pad as f64);
    let starts = match mode {
        ChopMode::Equal { count } => equal_slices(dur, js_min(count as f64, max_slices)),
        ChopMode::Transients { sensitivity } => {
            let data = mono_range(buffer, (start * sr).floor() as usize, (end * sr).floor() as usize);
            detect_transients(&data, sr, sensitivity, max_slices, 0.05)
        }
    };
    slices_to_ranges(&starts, dur)
        .into_iter()
        .map(|r| SliceRange { start_sec: start + r.start_sec, end_sec: start + r.end_sec })
        .collect()
}

/// `detectTransients(data, sampleRate, { sensitivity, maxSlices, minGapSec })` on mono data:
/// slice start times in seconds, ascending; the first is always 0.
pub(crate) fn detect_transients(
    data: &[f32],
    sample_rate: f64,
    sensitivity: f64,
    max_slices: f64,
    min_gap_sec: f64,
) -> Vec<f64> {
    let sensitivity = js_clamp(sensitivity, 0.0, 1.0);
    let max_slices = js_max(1.0, max_slices.floor());
    let min_gap = js_max(0.0, min_gap_sec);
    if data.is_empty() || !(sample_rate > 0.0) || max_slices == 1.0 {
        return vec![0.0];
    }

    let hop = js_max(32.0, js_round(sample_rate * 0.005)) as usize;
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

    let rise_threshold = 14.0 - 11.0 * sensitivity;
    let gate = max_db - (24.0 + 36.0 * sensitivity);
    let lookback = 3;
    let mut rise = vec![0f32; frames];
    for f in 0..frames {
        let mut floor = f64::INFINITY;
        for k in 1..=lookback {
            floor = js_min(floor, if f >= k { db[f - k] as f64 } else { -120.0 });
        }
        rise[f] = (db[f] as f64 - floor) as f32;
    }

    let mut candidates: Vec<(f64, f64)> = Vec::new(); // (time, strength)
    for f in 0..frames {
        if (db[f] as f64) < gate || (rise[f] as f64) < rise_threshold {
            continue;
        }
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
        candidates.push((refine_onset(data, f, hop, win) as f64 / sample_rate, rise[f] as f64));
    }

    // Strongest first (stable), honouring the minimum gap.
    candidates.sort_by(|a, b| (b.1 - a.1).partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut accepted = vec![0.0];
    for (time, _) in candidates {
        if accepted.len() as f64 >= max_slices {
            break;
        }
        if accepted.iter().all(|t| (t - time).abs() >= js_max(min_gap, 1e-6)) {
            accepted.push(time);
        }
    }
    accepted.sort_by(|a, b| (a - b).partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal));
    accepted
}

/// `refineOnset`: first sample around frame `f` whose level reaches 10% of the hit's peak.
fn refine_onset(data: &[f32], f: usize, hop: usize, win: usize) -> usize {
    let from = f.saturating_sub(1) * hop;
    let to = data.len().min(f * hop + win);
    let mut peak = 0f64;
    for &x in &data[from..to] {
        peak = js_max(peak, (x as f64).abs());
    }
    let threshold = peak * 0.1;
    for (i, &x) in data.iter().enumerate().take(to).skip(from) {
        if (x as f64).abs() >= threshold {
            return i;
        }
    }
    f * hop
}

/// `equalSlices(durationSec, count)`: `[0, d/n, 2d/n, ...]`.
pub(crate) fn equal_slices(duration_sec: f64, count: f64) -> Vec<f64> {
    let n = js_max(1.0, count.floor()) as usize;
    let d = js_max(0.0, duration_sec);
    (0..n).map(|i| (d * i as f64) / n as f64).collect()
}

/// `slicesToRanges(starts, durationSec)`: `[start, next start)`; the last ends at the duration.
pub(crate) fn slices_to_ranges(starts: &[f64], duration_sec: f64) -> Vec<SliceRange> {
    let mut sorted = starts.to_vec();
    sorted.sort_by(|a, b| (a - b).partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal));
    (0..sorted.len())
        .map(|i| SliceRange {
            start_sec: sorted[i],
            end_sec: if i + 1 < sorted.len() { sorted[i + 1] } else { duration_sec },
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Missing pack fixes (missingPackActions.ts, pure half)
// ---------------------------------------------------------------------------

/// `MissingPadFix`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MissingPadFix {
    Clear,
    Builtin,
}

impl MissingPadFix {
    pub fn label(self) -> &'static str {
        match self {
            Self::Clear => "clear",
            Self::Builtin => "builtin",
        }
    }

    /// Undo label of `fixMissingPads`.
    pub fn undo_label(self) -> &'static str {
        match self {
            Self::Clear => "Clear pads",
            Self::Builtin => "Replace pads",
        }
    }
}

/// Settings reset applied with a replacement sample: `{ startSec: 0, endSec: null, reverse: false }`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PadTrimReset {
    pub start_sec: f64,
    pub end_sec: Option<f64>,
    pub reverse: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PadFix {
    pub pad: PadIndex,
    pub sample: Option<PadSample>,
    pub patch: Option<PadTrimReset>,
}

/// `planMissingPadFix(pads, fix)`: the sample (and setting resets) each pad gets. `builtin` puts
/// the factory kit's sound for the same pad back; pads outside the kit get cleared.
/// (`fixMissingPads` applies the plan as one undo step labelled [`MissingPadFix::undo_label`]:
/// `setPadSample` per pad, then `updatePad(patch)` when there is a patch.)
pub fn plan_missing_pad_fix(pads: &[PadIndex], fix: MissingPadFix) -> Vec<PadFix> {
    pads.iter()
        .map(|&pad| {
            let voice = if fix == MissingPadFix::Builtin { FACTORY_PAD_VOICES.get(pad).copied() } else { None };
            match voice {
                None => PadFix { pad, sample: None, patch: None },
                Some(voice) => PadFix {
                    pad,
                    sample: Some(PadSample {
                        source: PadSampleSource::Factory { voice },
                        name: factory_voice_label(voice).to_string(),
                    }),
                    patch: Some(PadTrimReset { start_sec: 0.0, end_sec: None, reverse: false }),
                },
            }
        })
        .collect()
}
