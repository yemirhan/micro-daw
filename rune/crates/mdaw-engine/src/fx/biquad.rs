//! Chromium `BiquadFilterNode` with per-sample coefficients (blink `biquad.cc`,
//! `biquad_dsp_kernel.cc`), as `Tone.Filter` drives it: every param of a Tone filter is fed by a
//! `Tone.Signal`, so Chromium always takes the sample-accurate path (`UpdateCoefficients` per
//! frame, scalar direct form I with f64 coefficients and state, f32 output). Values per frame:
//! `frequency` (Hz, clamped to [0, nyquist]) and `Q`; detune and gain are 0.
//!
//! Coefficients (normalized frequency `f = freq / nyquist`):
//! - lowpass / highpass: Q is a resonance in dB, `α = sin(πf) / (2·10^(Q/20))`;
//! - bandpass / notch / allpass: Q linear (≥ 0), `α = sin(πf) / (2Q)`, with Chromium's limits at
//!   f ∈ {0, 1} and Q = 0.
//!
//! Coefficient sets are cached: a quantum whose (frequency, Q) pairs repeat reuses them, which
//! gives the same numbers as recomputing per frame.

use mdaw_model::types::FilterType;

/// Normalized coefficients (`a0 = 1`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coefs {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
}

impl Coefs {
    const fn raw(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Self {
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    const IDENTITY: Coefs = Coefs::raw(1.0, 0.0, 0.0, 1.0, 0.0, 0.0);
    const ZERO: Coefs = Coefs::raw(0.0, 0.0, 0.0, 1.0, 0.0, 0.0);

    /// `BiquadDSPKernel::UpdateCoefficients` for one frame.
    pub fn compute(kind: FilterType, frequency: f32, q: f32, sample_rate: f64) -> Self {
        let nyquist = 0.5 * sample_rate;
        let f = frequency as f64 / nyquist;
        let q = q as f64;
        match kind {
            FilterType::Lowpass => {
                let f = f.clamp(0.0, 1.0);
                if f == 1.0 {
                    Self::IDENTITY
                } else if f > 0.0 {
                    let g = 10f64.powf(q / 20.0);
                    let w0 = std::f64::consts::PI * f;
                    let alpha = w0.sin() / (2.0 * g);
                    let cosw = w0.cos();
                    let beta = (1.0 - cosw) / 2.0;
                    Self::raw(
                        beta,
                        2.0 * beta,
                        beta,
                        1.0 + alpha,
                        -2.0 * cosw,
                        1.0 - alpha,
                    )
                } else {
                    Self::ZERO
                }
            }
            FilterType::Highpass => {
                let f = f.clamp(0.0, 1.0);
                if f == 1.0 {
                    Self::ZERO
                } else if f > 0.0 {
                    let g = 10f64.powf(q / 20.0);
                    let w0 = std::f64::consts::PI * f;
                    let alpha = w0.sin() / (2.0 * g);
                    let cosw = w0.cos();
                    let beta = (1.0 + cosw) / 2.0;
                    Self::raw(
                        beta,
                        -2.0 * beta,
                        beta,
                        1.0 + alpha,
                        -2.0 * cosw,
                        1.0 - alpha,
                    )
                } else {
                    Self::IDENTITY
                }
            }
            FilterType::Bandpass => {
                let f = f.max(0.0);
                let q = q.max(0.0);
                if f > 0.0 && f < 1.0 {
                    if q > 0.0 {
                        let w0 = std::f64::consts::PI * f;
                        let alpha = w0.sin() / (2.0 * q);
                        let k = w0.cos();
                        Self::raw(alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * k, 1.0 - alpha)
                    } else {
                        Self::IDENTITY
                    }
                } else {
                    Self::ZERO
                }
            }
            FilterType::Notch => {
                let f = f.clamp(0.0, 1.0);
                let q = q.max(0.0);
                if f > 0.0 && f < 1.0 {
                    if q > 0.0 {
                        let w0 = std::f64::consts::PI * f;
                        let alpha = w0.sin() / (2.0 * q);
                        let k = w0.cos();
                        Self::raw(1.0, -2.0 * k, 1.0, 1.0 + alpha, -2.0 * k, 1.0 - alpha)
                    } else {
                        Self::ZERO
                    }
                } else {
                    Self::IDENTITY
                }
            }
            FilterType::Allpass => {
                let f = f.clamp(0.0, 1.0);
                let q = q.max(0.0);
                if f > 0.0 && f < 1.0 {
                    if q > 0.0 {
                        let w0 = std::f64::consts::PI * f;
                        let alpha = w0.sin() / (2.0 * q);
                        let k = w0.cos();
                        Self::raw(
                            1.0 - alpha,
                            -2.0 * k,
                            1.0 + alpha,
                            1.0 + alpha,
                            -2.0 * k,
                            1.0 - alpha,
                        )
                    } else {
                        Self::raw(-1.0, 0.0, 0.0, 1.0, 0.0, 0.0)
                    }
                } else {
                    Self::IDENTITY
                }
            }
        }
    }
}

/// Direct form I state of one channel.
#[derive(Debug, Clone, Copy, Default)]
pub struct State {
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl State {
    #[inline]
    pub fn tick(&mut self, c: &Coefs, x: f32) -> f32 {
        let xd = x as f64;
        let y =
            (c.b0 * xd + c.b1 * self.x1 + c.b2 * self.x2 - c.a1 * self.y1 - c.a2 * self.y2) as f32;
        self.x2 = self.x1;
        self.x1 = xd;
        self.y2 = self.y1;
        self.y1 = y as f64;
        y
    }

    /// Chromium flushes denormal state after each block.
    pub fn flush(&mut self) {
        let f = |v: &mut f64| {
            if (*v as f32).is_subnormal() {
                *v = 0.0;
            }
        };
        f(&mut self.x1);
        f(&mut self.x2);
        f(&mut self.y1);
        f(&mut self.y2);
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Coefficients for one quantum, recomputed only where (frequency, Q) change.
#[derive(Debug, Clone)]
pub struct CoefCache {
    key: (FilterType, f32, f32),
    coefs: Coefs,
}

impl CoefCache {
    pub fn new(kind: FilterType, frequency: f32, q: f32, sr: f64) -> Self {
        Self {
            key: (kind, frequency, q),
            coefs: Coefs::compute(kind, frequency, q, sr),
        }
    }

    #[inline]
    pub fn get(&mut self, kind: FilterType, frequency: f32, q: f32, sr: f64) -> &Coefs {
        let key = (kind, frequency, q);
        if key != self.key {
            self.key = key;
            self.coefs = Coefs::compute(kind, frequency, q, sr);
        }
        &self.coefs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdaw_dsp::factory::biquad::{Biquad, BiquadKind};

    #[test]
    fn matches_the_dsp_crate_biquad() {
        let mut a = Biquad::new(BiquadKind::Lowpass, 1234.5, 3.0, 48000);
        let c = Coefs::compute(FilterType::Lowpass, 1234.5, 3.0, 48000.0);
        let mut s = State::default();
        for k in 0..2000 {
            let x = ((k * 7919 % 1000) as f32 / 500.0) - 1.0;
            assert_eq!(a.tick(x), s.tick(&c, x));
        }
    }

    #[test]
    fn limits() {
        let sr = 48000.0;
        assert_eq!(
            Coefs::compute(FilterType::Lowpass, 30000.0, 1.0, sr),
            Coefs::IDENTITY
        );
        assert_eq!(
            Coefs::compute(FilterType::Highpass, 0.0, 1.0, sr),
            Coefs::IDENTITY
        );
        assert_eq!(
            Coefs::compute(FilterType::Notch, 1000.0, 0.0, sr),
            Coefs::ZERO
        );
        assert_eq!(
            Coefs::compute(FilterType::Allpass, 1000.0, 0.0, sr).b0,
            -1.0
        );
        assert_eq!(
            Coefs::compute(FilterType::Lowpass, 0.0, 1.0, sr),
            Coefs::ZERO
        );
    }
}
