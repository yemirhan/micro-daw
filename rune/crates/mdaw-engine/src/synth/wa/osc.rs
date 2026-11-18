//! Chromium `OscillatorNode` (`oscillator_node.cc`) as Tone drives it: frequency and detune are
//! always fed by Tone signals (input connections), so every quantum takes the sample-accurate
//! path:
//!
//! - phase increment per frame: `incr = clamp(f32(freq · exp2f(detune · f32(1/1200))), ±nyquist)
//!   · rateScale` (`rateScale = f32(size / sr)`), all f32;
//! - table pair and cross-fade from `WaveDataForFundamentalFrequency(incr / rateScale)`;
//! - read with linear interpolation for `|incr| ≥ 0.3`, 3-point Lagrange for `≥ 0.16`, else
//!   5-point Lagrange (`DoInterpolation`), the phase (`virtual_read_index`, f64) advancing by
//!   `incr` and wrapping into `[0, size)`;
//! - a started node begins at `start_frame = ceil(round(t·sr·1024)/1024)` with phase 0 (Chromium
//!   only adjusts the start phase on the k-rate path); when that rounding lands below `t·sr` the
//!   first frame stays silent;
//! - in the quantum a node starts in, Chromium reads the phase increments from the START of the
//!   quantum's increment array (`phase_increments++` from index 0 while the output pointer is
//!   offset to the start frame): a node starting at quantum frame `s` plays frame `s + j` with
//!   the frequency / detune of frame `j`. Audible: a note's first partial quantum runs at the
//!   frequency the signal had before the note (e.g. Tone's 440 Hz default for a fresh voice).
//!
//! Every `Tone.Oscillator.start` from the stopped state creates a NEW native node (phase 0), while
//! a restart of a running oscillator keeps it (phase continuous): [`OscNode::start_new_at`]
//! queues a phase reset at the node's start frame.

use std::sync::Arc;

use super::wave::WaveTables;

/// Chromium's AudioParam detune range (`1200·log2(FLT_MAX)`).
pub(crate) const DETUNE_LIMIT: f32 = 153_600.0;
const MAX_PENDING: usize = 8;

/// Blink `audio_utilities::TimeToSampleFrame(t, sr, kRoundUp)`, and whether Chromium's
/// `start_frame_offset` is positive (the first frame is then skipped).
pub(crate) fn start_frame(time: f64, sr: f64) -> (u64, bool) {
    let exact = time * sr;
    let frame = ((exact * 1024.0).round() / 1024.0).ceil().max(0.0);
    (frame as u64, exact - frame > 0.0)
}

#[derive(Debug, Clone)]
pub(crate) struct OscNode {
    wave: Arc<WaveTables>,
    mask: usize,
    size: f64,
    lowest: f32,
    rate_scale: f32,
    inv_rate_scale: f32,
    nyquist: f32,
    /// `virtual_read_index_`.
    phase: f64,
    running: bool,
    /// Scheduled new-node starts: (first frame, skip that frame), in time order.
    pending: [(u64, bool); MAX_PENDING],
    /// Offset of the current node's increments in this quantum (its start frame in the quantum).
    shift: usize,
    n_pending: usize,
    /// Cache of the last table selection.
    last_incr: f32,
    sel: (usize, usize, f32),
}

impl OscNode {
    pub fn new(wave: Arc<WaveTables>, sr: f64) -> Self {
        let size = wave.size();
        let nyquist = (sr / 2.0) as f32;
        let rate_scale = size as f32 / sr as f32;
        Self {
            mask: size - 1,
            size: size as f64,
            lowest: nyquist / (size / 2) as f32,
            rate_scale,
            inv_rate_scale: 1.0 / rate_scale,
            nyquist,
            wave,
            phase: 0.0,
            running: false,
            pending: [(0, false); MAX_PENDING],
            n_pending: 0,
            shift: 0,
            last_incr: f32::NAN,
            sel: (0, 0, 0.0),
        }
    }

    /// Swap the waveform (`oscillator.type = …` / `setPeriodicWave` on the live node).
    pub fn set_wave(&mut self, wave: Arc<WaveTables>) {
        self.wave = wave;
        self.last_incr = f32::NAN;
    }

    /// A new native node starting at `time` (phase 0 from its start frame).
    pub fn start_new_at(&mut self, time: f64, sr: f64) {
        let s = start_frame(time, sr);
        if self.n_pending == MAX_PENDING {
            self.pending.copy_within(1.., 0);
            self.n_pending -= 1;
        }
        let at = self.pending[..self.n_pending].partition_point(|p| p.0 <= s.0);
        self.pending.copy_within(at..self.n_pending, at + 1);
        self.pending[at] = s;
        self.n_pending += 1;
    }

    /// A NEW native node (Tone creates one per start from the stopped state): forget the
    /// previous node, start at `time` with phase 0.
    pub fn restart_at(&mut self, time: f64, sr: f64) {
        self.running = false;
        self.n_pending = 0;
        self.phase = 0.0;
        self.start_new_at(time, sr);
    }

    /// Render `out.len()` frames from context frame `frame`. `freq` / `detune` are the final
    /// (input-summed) param values. Frames before the node's start are 0.
    pub fn render(&mut self, frame: u64, freq: &[f32], detune: &[f32], out: &mut [f32]) {
        let n = out.len();
        let mut k = 0usize;
        // A node that started in an earlier quantum reads its increments aligned.
        self.shift = 0;
        while k < n {
            let next_start = (self.n_pending > 0)
                .then(|| self.pending[0])
                .filter(|p| p.0 < frame + n as u64);
            let until = next_start.map_or(n, |p| (p.0.saturating_sub(frame) as usize).max(k));
            if self.running {
                self.render_span(k, until, freq, detune, out);
            } else {
                out[k..until].fill(0.0);
            }
            k = until;
            if let Some((_, skip)) = next_start {
                self.pending.copy_within(1..self.n_pending, 0);
                self.n_pending -= 1;
                self.running = true;
                self.phase = 0.0;
                if skip && k < n {
                    out[k] = 0.0;
                    k += 1;
                }
                self.shift = k;
            }
        }
    }

    #[allow(clippy::needless_range_loop)] // parallel per-frame arrays
    fn render_span(
        &mut self,
        from: usize,
        to: usize,
        freq: &[f32],
        detune: &[f32],
        out: &mut [f32],
    ) {
        let k1200 = (1.0f64 / 1200.0) as f32;
        let wave = &*self.wave;
        for i in from..to {
            let j = i - self.shift;
            let det = (detune[j].clamp(-DETUNE_LIMIT, DETUNE_LIMIT) * k1200).exp2();
            let f = (freq[j].clamp(-self.nyquist, self.nyquist) * det)
                .clamp(-self.nyquist, self.nyquist);
            let incr = f * self.rate_scale;
            if incr.to_bits() != self.last_incr.to_bits() {
                self.last_incr = incr;
                self.sel = wave.select(self.inv_rate_scale * incr, self.lowest);
            }
            let (r1, r2, tf) = self.sel;
            out[i] = interpolate(
                self.phase,
                incr.abs(),
                self.mask,
                tf,
                wave.table(r2),
                wave.table(r1),
            );
            self.phase += incr as f64;
            self.phase -= (self.phase / self.size).floor() * self.size;
        }
    }
}

/// Chromium `DoInterpolation`.
#[inline]
fn interpolate(vri: f64, incr: f32, mask: usize, tf: f32, lower: &[f32], higher: &[f32]) -> f32 {
    let i0 = vri as usize;
    let (mut lo, mut hi) = (0f64, 0f64);
    if incr >= 0.3 {
        let a = i0 & mask;
        let b = (i0 + 1) & mask;
        let f = (vri as f32 - i0 as f32) as f64;
        hi = (1.0 - f) * higher[a] as f64 + f * higher[b] as f64;
        lo = (1.0 - f) * lower[a] as f64 + f * lower[b] as f64;
    } else if incr >= 0.16 {
        let t = vri - i0 as f64;
        let w = [0.5 * t * (t - 1.0), 1.0 - t * t, 0.5 * t * (t + 1.0)];
        for (j, wj) in w.iter().enumerate() {
            let idx = (i0.wrapping_add(j).wrapping_sub(1)) & mask;
            lo += wj * lower[idx] as f64;
            hi += wj * higher[idx] as f64;
        }
    } else {
        let t = vri - i0 as f64;
        let t2 = t * t;
        let w = [
            t * (t2 - 1.0) * (t - 2.0) / 24.0,
            -t * (t - 1.0) * (t2 - 4.0) / 6.0,
            (t2 - 1.0) * (t2 - 4.0) / 4.0,
            -t * (t + 1.0) * (t2 - 4.0) / 6.0,
            t * (t2 - 1.0) * (t + 2.0) / 24.0,
        ];
        for (j, wj) in w.iter().enumerate() {
            let idx = (i0.wrapping_add(j).wrapping_sub(2)) & mask;
            lo += wj * lower[idx] as f64;
            hi += wj * higher[idx] as f64;
        }
    }
    ((1.0 - tf) as f64 * hi + tf as f64 * lo) as f32
}

#[cfg(test)]
mod tests {
    use super::super::wave::{BasicShape, basic_wave};
    use super::*;

    #[test]
    fn starts_on_the_rounded_up_frame_with_phase_zero() {
        let sr = 48000.0;
        let mut o = OscNode::new(basic_wave(BasicShape::Sine, sr), sr);
        o.start_new_at(100.5 / sr, sr);
        let f = [1000f32; 128];
        let d = [0f32; 128];
        let mut out = [9f32; 128];
        o.render(0, &f, &d, &mut out);
        assert!(out[..101].iter().all(|&x| x == 0.0));
        let want = (2.0 * std::f64::consts::PI * 1000.0 / sr).sin();
        assert!((out[102] as f64 - want).abs() < 1e-4, "{}", out[102]);
    }
}
