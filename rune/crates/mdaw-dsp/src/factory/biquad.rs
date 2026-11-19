//! Web Audio `BiquadFilterNode` (lowpass / highpass / bandpass) and `Tone.Filter`.
//!
//! Coefficients follow the Web Audio spec / Chromium `Biquad::Set*Params` — NOT the RBJ cookbook
//! defaults where they differ: for lowpass and highpass the `Q` attribute is a resonance in dB,
//! `α = sin(w0) / (2·10^(Q/20))`; for bandpass `Q` is linear, `α = sin(w0) / (2Q)` (constant
//! 0 dB peak gain). `w0 = π·f/nyquist`, coefficients in f64, normalised by `a0`.
//!
//! Processing is direct form I with f64 coefficients and state, f32 input/output (Chromium
//! rounds each output to float and feeds that back as `y1`/`y2`).
//!
//! `Tone.Filter({ rolloff })` cascades `|rolloff| / 12` identical biquads (−12 → 1, −24 → 2);
//! its `Q` defaults to 1, which for lowpass/highpass means +1 dB of resonance.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BiquadKind {
    Lowpass,
    Highpass,
    Bandpass,
}

#[derive(Debug, Clone, Copy)]
pub struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl Biquad {
    /// Coefficients for `kind` at `frequency` Hz with `q` (dB for low/highpass, linear for
    /// bandpass). `frequency` and `q` go through f32 first, like AudioParam values.
    pub fn new(kind: BiquadKind, frequency: f64, q: f64, sample_rate: u32) -> Self {
        let nyquist = sample_rate as f64 / 2.0;
        let f = (frequency as f32 as f64 / nyquist).clamp(0.0, 1.0);
        let q = q as f32 as f64;
        let (b0, b1, b2, a0, a1, a2) = match kind {
            BiquadKind::Lowpass => {
                if f >= 1.0 {
                    (1.0, 0.0, 0.0, 1.0, 0.0, 0.0)
                } else if f <= 0.0 {
                    (0.0, 0.0, 0.0, 1.0, 0.0, 0.0)
                } else {
                    let g = 10f64.powf(q / 20.0);
                    let w0 = std::f64::consts::PI * f;
                    let alpha = w0.sin() / (2.0 * g);
                    let cosw = w0.cos();
                    let beta = (1.0 - cosw) / 2.0;
                    (
                        beta,
                        2.0 * beta,
                        beta,
                        1.0 + alpha,
                        -2.0 * cosw,
                        1.0 - alpha,
                    )
                }
            }
            BiquadKind::Highpass => {
                if f >= 1.0 {
                    (0.0, 0.0, 0.0, 1.0, 0.0, 0.0)
                } else if f <= 0.0 {
                    (1.0, 0.0, 0.0, 1.0, 0.0, 0.0)
                } else {
                    let g = 10f64.powf(q / 20.0);
                    let w0 = std::f64::consts::PI * f;
                    let alpha = w0.sin() / (2.0 * g);
                    let cosw = w0.cos();
                    let beta = (1.0 + cosw) / 2.0;
                    (
                        beta,
                        -2.0 * beta,
                        beta,
                        1.0 + alpha,
                        -2.0 * cosw,
                        1.0 - alpha,
                    )
                }
            }
            BiquadKind::Bandpass => {
                if f > 0.0 && f < 1.0 {
                    let w0 = std::f64::consts::PI * f;
                    if q > 0.0 {
                        let alpha = w0.sin() / (2.0 * q);
                        let k = w0.cos();
                        (alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * k, 1.0 - alpha)
                    } else {
                        // Q == 0: Chromium sets an all-pass of gain 1.
                        (1.0, 0.0, 0.0, 1.0, 0.0, 0.0)
                    }
                } else {
                    (0.0, 0.0, 0.0, 1.0, 0.0, 0.0)
                }
            }
        };
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let xd = x as f64;
        let y = (self.b0 * xd + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2) as f32;
        self.x2 = self.x1;
        self.x1 = xd;
        self.y2 = self.y1;
        // Chromium flushes denormals to zero.
        self.y1 = if y.is_subnormal() { 0.0 } else { y as f64 };
        y
    }

    pub fn process(&mut self, data: &mut [f32]) {
        for x in data {
            *x = self.tick(*x);
        }
    }
}

/// `Tone.Filter` with a fixed type / frequency / Q / rolloff (−12, −24, −48 or −96).
pub fn tone_filter(
    kind: BiquadKind,
    frequency: f64,
    q: f64,
    rolloff: i32,
    sample_rate: u32,
    data: &mut [f32],
) {
    let stages = match rolloff {
        -24 => 2,
        -48 => 4,
        -96 => 8,
        _ => 1,
    };
    for _ in 0..stages {
        Biquad::new(kind, frequency, q, sample_rate).process(data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gain_at(kind: BiquadKind, f0: f64, q: f64, f: f64, sr: u32) -> f64 {
        let mut b = Biquad::new(kind, f0, q, sr);
        let n = sr as usize;
        let mut acc = 0.0;
        let mut acc_in = 0.0;
        for i in 0..n {
            let x = (2.0 * std::f64::consts::PI * f * i as f64 / sr as f64).sin() as f32;
            let y = b.tick(x);
            if i > n / 2 {
                acc += (y as f64).powi(2);
                acc_in += (x as f64).powi(2);
            }
        }
        (acc / acc_in).sqrt()
    }

    #[test]
    fn lowpass_q_is_db() {
        // Q = 1 dB of resonance: |H(f0)| = 10^(1/20).
        let g = gain_at(BiquadKind::Lowpass, 1000.0, 1.0, 1000.0, 48000);
        assert!(
            (20.0 * g.log10() - 1.0).abs() < 0.05,
            "{}",
            20.0 * g.log10()
        );
        let g = gain_at(BiquadKind::Highpass, 1000.0, 1.0, 1000.0, 48000);
        assert!((20.0 * g.log10() - 1.0).abs() < 0.05);
    }

    #[test]
    fn bandpass_peak_is_unity() {
        let g = gain_at(BiquadKind::Bandpass, 2000.0, 2.5, 2000.0, 44100);
        assert!((g - 1.0).abs() < 0.01, "{g}");
        let g = gain_at(BiquadKind::Bandpass, 2000.0, 2.5, 500.0, 44100);
        assert!(g < 0.2);
    }
}
