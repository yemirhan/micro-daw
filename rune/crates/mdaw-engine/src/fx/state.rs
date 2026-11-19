//! Plain-data snapshots of a track's effect settings (`TrackEffectState` without its `extra`
//! maps), small and `Copy`, so they cross to the audio thread inside a command without
//! allocating or freeing there.

use mdaw_model::types::{FilterType, LfoWaveform, TrackEffectState};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FxState {
    pub reverb_on: bool,
    pub reverb_wet: f64,
    pub delay_on: bool,
    pub delay_time: f64,
    pub delay_feedback: f64,
    pub delay_wet: f64,
    pub chorus_on: bool,
    pub chorus_depth: f64,
    pub dist_on: bool,
    pub dist_amount: f64,
    pub dist_wet: f64,
    pub eq_on: bool,
    pub eq: [f64; 3],
    pub comp_on: bool,
    pub comp_threshold: f64,
    pub comp_ratio: f64,
    pub comp_attack: f64,
    pub comp_release: f64,
    pub filter_on: bool,
    pub filter_type: FilterType,
    pub cutoff: f64,
    pub resonance: f64,
    pub lfo_rate: f64,
    pub lfo_depth: f64,
    pub lfo_wave: LfoWaveform,
}

impl From<&TrackEffectState> for FxState {
    fn from(f: &TrackEffectState) -> Self {
        Self {
            reverb_on: f.reverb.enabled,
            reverb_wet: f.reverb.wet,
            delay_on: f.delay.enabled,
            delay_time: f.delay.time,
            delay_feedback: f.delay.feedback,
            delay_wet: f.delay.wet,
            chorus_on: f.chorus.enabled,
            chorus_depth: f.chorus.depth,
            dist_on: f.distortion.enabled,
            dist_amount: f.distortion.amount,
            dist_wet: f.distortion.wet,
            eq_on: f.eq.enabled,
            eq: [f.eq.low, f.eq.mid, f.eq.high],
            comp_on: f.compressor.enabled,
            comp_threshold: f.compressor.threshold,
            comp_ratio: f.compressor.ratio,
            comp_attack: f.compressor.attack,
            comp_release: f.compressor.release,
            filter_on: f.filter.enabled,
            filter_type: f.filter.kind,
            cutoff: f.filter.cutoff,
            resonance: f.filter.resonance,
            lfo_rate: f.filter.lfo_rate,
            lfo_depth: f.filter.lfo_depth,
            lfo_wave: f.filter.lfo_wave,
        }
    }
}

impl Default for FxState {
    /// `DEFAULT_TRACK_EFFECTS`.
    fn default() -> Self {
        Self::from(&TrackEffectState::default())
    }
}

/// JS `x || d` for a number: 0 and NaN are falsy.
pub fn or_default(x: f64, d: f64) -> f64 {
    if x == 0.0 || x.is_nan() { d } else { x }
}

/// `compressorActive(enabled, ratio)`: a compressor at ratio 1 does nothing, so it stays
/// bypassed (no 6 ms look-ahead).
pub fn compressor_active(enabled: bool, ratio: f64) -> bool {
    enabled && ratio > 1.001
}

/// The filter LFO range for `cutoff` / `depth` (`trackAudioFactory.ts`, `AudioEngine.ts`).
pub fn lfo_range(cutoff: f64, depth: f64) -> (f64, f64) {
    (
        60f64.max(cutoff * (1.0 - 0.75 * depth)),
        18000f64.min(cutoff * (1.0 + 0.75 * depth)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compressor_active_needs_enabled_and_a_ratio_above_one() {
        assert!(compressor_active(true, 4.0));
        assert!(!compressor_active(false, 4.0));
        assert!(!compressor_active(true, 1.0));
        assert!(!compressor_active(true, 1.001));
        assert!(compressor_active(true, 1.0011));
    }

    #[test]
    fn js_or_and_lfo_range() {
        assert_eq!(or_default(0.0, 0.3), 0.3);
        assert_eq!(or_default(f64::NAN, 0.3), 0.3);
        assert_eq!(or_default(0.1, 0.3), 0.1);
        assert_eq!(lfo_range(1000.0, 0.5), (625.0, 1375.0));
        assert_eq!(lfo_range(100.0, 1.0), (60.0, 175.0));
        assert_eq!(lfo_range(18000.0, 0.2), (15300.0, 18000.0));
    }
}
