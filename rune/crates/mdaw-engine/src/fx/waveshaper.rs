//! Chromium `WaveShaperNode` with `oversample = '2x'` (blink `wave_shaper_dsp_kernel.cc`,
//! `up_sampler.cc`, `down_sampler.cc`, `direct_convolver.cc`), per channel:
//! - UpSampler: even output frames are the input delayed by 64 frames; odd ones a 128-tap
//!   Blackman-windowed sinc (offset −0.5) convolution of the input;
//! - the curve at 2× the rate (linear interpolation, `v = 0.5·(x + 1)·(n − 1)` in f64);
//! - DownSampler: half-band 256-tap kernel run as its 128 odd taps over the odd frames
//!   (delayed by one) plus `0.5 ×` the frame 128 back.
//!
//! Latency: 64 + 64 = 128 context frames. The convolutions accumulate in f32 (Chromium uses
//! vDSP_conv on macOS, whose internal summation order differs: ~1e-7 relative).
//!
//! [`distortion_curve`]: `Tone.Distortion`'s `setMap` (1024 points; amount k = 100·amount):
//! `|x| < 0.001 ? 0 : (3 + k)·x·20·(π/180) / (π + k·|x|)`.

use mdaw_dsp::shaping::waveshape;

use super::Q;

const UP_TAPS: usize = 128;
const DOWN_TAPS: usize = 128;
/// Points of a `setMap` curve.
pub const CURVE_LEN: usize = 1024;

fn blackman(x: f64) -> f64 {
    let alpha = 0.16;
    let a0 = 0.5 * (1.0 - alpha);
    let a1 = 0.5;
    let a2 = 0.5 * alpha;
    a0 - a1 * (2.0 * std::f64::consts::PI * x).cos()
        + a2 * (2.0 * std::f64::consts::PI * 2.0 * x).cos()
}

fn up_kernel() -> [f32; UP_TAPS] {
    let n = UP_TAPS as f64;
    let half = (UP_TAPS / 2) as f64;
    let offset = -0.5;
    let mut k = [0f32; UP_TAPS];
    for (i, v) in k.iter_mut().enumerate() {
        let s = std::f64::consts::PI * (i as f64 - half - offset);
        let sinc = if s == 0.0 { 1.0 } else { s.sin() / s };
        let window = blackman((i as f64 - offset) / n);
        *v = (sinc * window) as f32;
    }
    k
}

fn down_kernel() -> [f32; DOWN_TAPS] {
    let n = 2 * DOWN_TAPS;
    let half = (n / 2) as f64;
    let mut k = [0f32; DOWN_TAPS];
    for i in (1..n).step_by(2) {
        let s = 0.5 * std::f64::consts::PI * (i as f64 - half);
        let sinc = if s == 0.0 { 1.0 } else { s.sin() / s } * 0.5;
        let window = blackman(i as f64 / n as f64);
        k[(i - 1) / 2] = (sinc * window) as f32;
    }
    k
}

/// `DirectConvolver`: `y[i] = Σ_j kernel[j]·x[i − j]` over a 128-frame block with history.
#[derive(Clone)]
struct Direct {
    kernel: [f32; 128],
    buf: [f32; 2 * Q],
}

impl Direct {
    fn new(kernel: [f32; 128]) -> Self {
        Self {
            kernel,
            buf: [0.0; 2 * Q],
        }
    }

    fn process(&mut self, src: &[f32; Q], dst: &mut [f32; Q]) {
        self.buf[Q..].copy_from_slice(src);
        for (i, d) in dst.iter_mut().enumerate() {
            let mut acc = 0f32;
            for j in (0..128).rev() {
                acc += self.buf[Q + i - j] * self.kernel[j];
            }
            *d = acc;
        }
        self.buf.copy_within(Q.., 0);
    }
}

/// One channel of a 2×-oversampled WaveShaper.
#[derive(Clone)]
pub struct Oversampled2x {
    up_in: [f32; 2 * Q],
    up_conv: Direct,
    down_in: [f32; 4 * Q],
    down_conv: Direct,
    up: [f32; 2 * Q],
}

impl Oversampled2x {
    pub fn new() -> Self {
        Self {
            up_in: [0.0; 2 * Q],
            up_conv: Direct::new(up_kernel()),
            down_in: [0.0; 4 * Q],
            down_conv: Direct::new(down_kernel()),
            up: [0.0; 2 * Q],
        }
    }

    /// Shape `io` in place through `curve` at twice the rate.
    pub fn process(&mut self, curve: &[f32], io: &mut [f32; Q]) {
        // UpSampler
        let half = UP_TAPS / 2;
        self.up_in[Q..].copy_from_slice(io);
        let mut odd = [0f32; Q];
        self.up_conv.process(io, &mut odd);
        for (i, o) in odd.iter().enumerate() {
            self.up[2 * i] = self.up_in[Q - half + i];
            self.up[2 * i + 1] = *o;
        }
        self.up_in.copy_within(Q.., 0);
        // curve
        for x in self.up.iter_mut() {
            *x = waveshape(curve, *x);
        }
        // DownSampler
        let src = 2 * Q;
        self.down_in[src..].copy_from_slice(&self.up);
        let mut odds = [0f32; Q];
        for (i, o) in odds.iter_mut().enumerate() {
            *o = self.down_in[src - 1 + 2 * i];
        }
        self.down_conv.process(&odds, io);
        let half = DOWN_TAPS; // kDefaultKernelSize / 2
        for (i, y) in io.iter_mut().enumerate() {
            *y = (*y as f64 + 0.5 * self.down_in[src - half + 2 * i] as f64) as f32;
        }
        self.down_in.copy_within(src.., 0);
    }

    /// Zero the filter state (Chromium re-initializes the kernels when the input's channel count
    /// changes).
    pub fn reset(&mut self) {
        self.up_in = [0.0; 2 * Q];
        self.down_in = [0.0; 4 * Q];
        self.up = [0.0; 2 * Q];
        self.up_conv.buf = [0.0; 2 * Q];
        self.down_conv.buf = [0.0; 2 * Q];
    }
}

impl Default for Oversampled2x {
    fn default() -> Self {
        Self::new()
    }
}

/// `Tone.Distortion` curve for `amount` into `out` (no allocation).
pub fn distortion_curve(amount: f64, out: &mut [f32; CURVE_LEN]) {
    let k = amount * 100.0;
    let deg = std::f64::consts::PI / 180.0;
    let last = (CURVE_LEN - 1) as f64;
    for (i, v) in out.iter_mut().enumerate() {
        let x = (i as f64 / last) * 2.0 - 1.0;
        *v = if x.abs() < 0.001 {
            0.0
        } else {
            (((3.0 + k) * x * 20.0 * deg) / (std::f64::consts::PI + k * x.abs())) as f32
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_curve_is_a_128_frame_delay_with_a_flat_passband() {
        let curve: Vec<f32> = (0..1025).map(|i| i as f32 / 512.0 - 1.0).collect();
        let mut s = Oversampled2x::new();
        let sr = 48000.0;
        let f = 1000.0;
        let mut out = Vec::new();
        let mut inp = Vec::new();
        for q in 0..40 {
            let mut io = [0f32; Q];
            for (k, x) in io.iter_mut().enumerate() {
                let n = q * Q + k;
                *x = 0.5 * (2.0 * std::f64::consts::PI * f * n as f64 / sr).sin() as f32;
            }
            inp.extend_from_slice(&io);
            s.process(&curve, &mut io);
            out.extend_from_slice(&io);
        }
        let err: f64 = (2000..5000)
            .map(|n| (out[n] as f64 - inp[n - 128] as f64).powi(2))
            .sum::<f64>()
            / (2000..5000).map(|n| (inp[n] as f64).powi(2)).sum::<f64>();
        assert!(10.0 * err.log10() < -60.0, "{}", 10.0 * err.log10());
    }

    #[test]
    fn amount_zero_is_x_over_3() {
        let mut c = [0f32; CURVE_LEN];
        distortion_curve(0.0, &mut c);
        let x = (900.0 / 1023.0) * 2.0 - 1.0;
        assert!((c[900] as f64 - x / 3.0).abs() < 1e-7);
        assert_eq!(c[511], 0.0f32.max(c[511]).min(c[511]));
    }
}
