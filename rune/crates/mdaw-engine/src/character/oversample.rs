//! Chromium's `WaveShaperNode` with `oversample = '2x'` (Blink `WaveShaperDSPKernel::
//! ProcessCurve2x`): `UpSampler` → curve at twice the rate → `DownSampler`.
//!
//! - `UpSampler` (`up_sampler.cc`, kernel 128): even output frames are the input delayed by 64;
//!   odd frames are the input convolved with a Blackman-windowed sinc offset by half a sample
//!   (`sinc(π(i − 64 + 0.5))`, window at `(i + 0.5) / 128`).
//! - `DownSampler` (`down_sampler.cc`, kernel 256): a half-band filter; only its odd taps
//!   (`0.5·sinc(0.5π(i − 128))`, Blackman at `i / 256`) are convolved with the odd input frames
//!   (delayed by one), plus `0.5 ×` the input delayed by 128 (the centre tap).
//! - Latency: 64 + 64 = 128 context frames, not compensated (as in Chromium).
//!
//! Kernels are stored as f32 (Blink `AudioFloatArray`). The convolutions sum in f64 here (Blink's
//! `DirectConvolver` uses vDSP / SIMD f32 sums, so the order differs: ~1e-7 relative).

use mdaw_dsp::shaping::waveshape;

const UP_K: usize = 128;
const DOWN_K: usize = 256;
const RED_K: usize = DOWN_K / 2;
/// Largest block processed at once (one render quantum).
const BLOCK: usize = 128;

fn blackman(x: f64) -> f64 {
    let alpha = 0.16;
    let a0 = 0.5 * (1.0 - alpha);
    let a1 = 0.5;
    let a2 = 0.5 * alpha;
    let two_pi = 2.0 * std::f64::consts::PI;
    a0 - a1 * (two_pi * x).cos() + a2 * (two_pi * 2.0 * x).cos()
}

fn up_kernel() -> [f32; UP_K] {
    let n = UP_K as f64;
    let half = (UP_K / 2) as f64;
    let offset = -0.5;
    std::array::from_fn(|i| {
        let s = std::f64::consts::PI * (i as f64 - half - offset);
        let sinc = if s == 0.0 { 1.0 } else { s.sin() / s };
        (sinc * blackman((i as f64 - offset) / n)) as f32
    })
}

fn down_kernel() -> [f32; RED_K] {
    let n = DOWN_K as f64;
    let half = (DOWN_K / 2) as f64;
    std::array::from_fn(|j| {
        let i = (2 * j + 1) as f64;
        let s = 0.5 * std::f64::consts::PI * (i - half);
        let sinc = if s == 0.0 { 1.0 } else { s.sin() / s } * 0.5;
        (sinc * blackman(i / n)) as f32
    })
}

/// One channel of a 2× oversampled WaveShaper. History buffers are fixed arrays: no allocation.
#[derive(Clone)]
pub struct Oversampled {
    up_k: [f32; UP_K],
    down_k: [f32; RED_K],
    /// Up-sampler input: `UP_K` frames of history, then the block.
    up_in: [f32; UP_K + BLOCK],
    /// Down-sampler input (2× rate): `DOWN_K` frames of history, then the block.
    down_in: [f32; DOWN_K + 2 * BLOCK],
}

impl Default for Oversampled {
    fn default() -> Self {
        Self::new()
    }
}

impl Oversampled {
    pub fn new() -> Self {
        Self {
            up_k: up_kernel(),
            down_k: down_kernel(),
            up_in: [0.0; UP_K + BLOCK],
            down_in: [0.0; DOWN_K + 2 * BLOCK],
        }
    }

    /// Clear the history (a new node).
    pub fn reset(&mut self) {
        self.up_in = [0.0; UP_K + BLOCK];
        self.down_in = [0.0; DOWN_K + 2 * BLOCK];
    }

    /// Shape `data` (≤ 128 frames) in place through `curve`.
    pub fn process(&mut self, curve: &[f32], data: &mut [f32]) {
        for chunk in data.chunks_mut(BLOCK) {
            self.block(curve, chunk);
        }
    }

    fn block(&mut self, curve: &[f32], data: &mut [f32]) {
        let n = data.len();
        // Up-sample into the down-sampler's input, shaping each 2× frame.
        self.up_in[UP_K..UP_K + n].copy_from_slice(data);
        let base = DOWN_K;
        for i in 0..n {
            let p = UP_K + i;
            let even = self.up_in[p - UP_K / 2];
            let mut odd = 0f64;
            for (j, k) in self.up_k.iter().enumerate() {
                odd += *k as f64 * self.up_in[p - j] as f64;
            }
            self.down_in[base + 2 * i] = waveshape(curve, even);
            self.down_in[base + 2 * i + 1] = waveshape(curve, odd as f32);
        }
        self.up_in.copy_within(n..n + UP_K, 0);
        // Down-sample.
        for (i, out) in data.iter_mut().enumerate() {
            let p = base + 2 * i;
            let mut acc = 0f64;
            for (j, k) in self.down_k.iter().enumerate() {
                acc += *k as f64 * self.down_in[p - 2 * j - 1] as f64;
            }
            let y = acc as f32;
            *out = (y as f64 + 0.5 * self.down_in[p - DOWN_K / 2] as f64) as f32;
        }
        self.down_in.copy_within(2 * n..2 * n + DOWN_K, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_curve_is_a_128_frame_delay_of_a_band_limited_signal() {
        let curve = mdaw_dsp::shaping::tone_curve(2048, |x| x);
        let mut o = Oversampled::new();
        let sr = 48000.0;
        let x: Vec<f32> = (0..2048)
            .map(|i| (2.0 * std::f64::consts::PI * 1000.0 * i as f64 / sr).sin() as f32 * 0.5)
            .collect();
        let mut y = x.clone();
        for c in y.chunks_mut(128) {
            o.process(&curve, c);
        }
        let err: f64 = (512..2048)
            .map(|i| (y[i] as f64 - x[i - 128] as f64).powi(2))
            .sum::<f64>()
            / (512..2048).map(|i| (x[i] as f64).powi(2)).sum::<f64>();
        assert!(10.0 * err.log10() < -60.0, "{}", 10.0 * err.log10());
    }
}
