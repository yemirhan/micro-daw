//! `src/components/beat/chop.ts` (slice ranges of a pad's trimmed sample, via [`crate::slicer`])
//! and the pure half of `missingPackActions.ts` (`planMissingPadFix`).
//!
//! Float32 semantics are kept: sample data is `f32`, all arithmetic on it happens in `f64` like JS.

use super::SliceRange;
use crate::js::{max as js_max, min as js_min};
use crate::kit::{FACTORY_PAD_VOICES, factory_voice_label};
use crate::slicer::{TransientOptions, detect_transients, equal_slices, slices_to_ranges};
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
            detect_transients(&data, sr, TransientOptions { sensitivity, max_slices, min_gap_sec: 0.05 })
        }
    };
    slices_to_ranges(&starts, dur)
        .into_iter()
        .map(|r| SliceRange { start_sec: start + r.start_sec, end_sec: start + r.end_sec })
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
