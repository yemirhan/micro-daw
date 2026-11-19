//! Per-pad sound shaping DSP (Rust-only, no TS counterpart): the drum pad's amp AHDS envelope,
//! the decay curve shared by the filter and pitch envelopes, a state-variable filter that can be
//! modulated per sample, a soft-clip drive and a one-pole parameter smoother.
//!
//! Everything here is allocation-free and `Copy`, so a voice can carry it in a fixed slot and the
//! audio thread never touches the heap. The engine side lives in `mdaw-engine`'s sampler; see
//! `docs/rune-port/pad-sound-shaping.md`.
//!
//! **Curves.** Every decaying stage uses `(1 − x)³` over its duration: it starts steep like an
//! exponential (slope −3 at x = 0), but lands exactly on its target at x = 1, so a stage has a
//! precise length (the envelope display draws the same function).

use std::f32::consts::PI;

/// The decay shape of every envelope stage: 1 at `x = 0`, 0 at `x ≥ 1`.
#[inline]
pub fn decay_curve(x: f64) -> f64 {
    if x <= 0.0 {
        1.0
    } else if x >= 1.0 {
        0.0
    } else {
        let u = 1.0 - x;
        u * u * u
    }
}

/// An amp envelope's attack / hold / decay / sustain (release is the pad's own fade).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ahds {
    /// Seconds.
    pub attack: f64,
    /// Seconds at full level after the attack.
    pub hold: f64,
    /// Seconds from full level to `sustain`.
    pub decay: f64,
    /// Level 0..1 held after the decay.
    pub sustain: f64,
}

impl Ahds {
    /// Flat at 1: the neutral envelope.
    pub const FLAT: Ahds = Ahds {
        attack: 0.0,
        hold: 0.0,
        decay: 0.0,
        sustain: 1.0,
    };

    /// Level at `t` seconds after the hit (before any release).
    pub fn level(&self, t: f64) -> f64 {
        if t < 0.0 {
            return 0.0;
        }
        if t < self.attack {
            return t / self.attack;
        }
        let t = t - self.attack;
        if t < self.hold {
            return 1.0;
        }
        let t = t - self.hold;
        if self.sustain >= 1.0 {
            return 1.0;
        }
        if self.decay <= 0.0 {
            return self.sustain;
        }
        self.sustain + (1.0 - self.sustain) * decay_curve(t / self.decay)
    }

    /// When the envelope has reached a silent sustain for good (None when it never goes silent).
    pub fn silent_after(&self) -> Option<f64> {
        (self.sustain <= 0.0).then_some(self.attack + self.hold + self.decay.max(0.0))
    }
}

/// A decaying modulation envelope (pitch, filter): `amount · decay_curve(t / decay)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModEnv {
    pub amount: f64,
    /// Seconds.
    pub decay: f64,
}

impl ModEnv {
    pub const OFF: ModEnv = ModEnv {
        amount: 0.0,
        decay: 0.0,
    };

    #[inline]
    pub fn value(&self, t: f64) -> f64 {
        if self.amount == 0.0 || self.decay <= 0.0 {
            return 0.0;
        }
        self.amount * decay_curve(t / self.decay)
    }

    pub fn is_off(&self) -> bool {
        self.amount == 0.0
    }
}

/// How long a slice of `length` buffer seconds lasts when played at `rate` (buffer seconds per
/// second) bent by a pitch envelope of `env` semitones. Closed form (no allocation, a fixed
/// bound on the work: it runs on the audio thread at every hit): over the envelope the read
/// speed is `rate · 2^(a·u³/12)` with `u = 1 − t/decay`, whose integral is the power series
/// [`bend_integral`]; the end inside the envelope is found by a safeguarded Newton solve.
pub fn pitched_duration(length: f64, rate: f64, env: ModEnv) -> f64 {
    if rate <= 0.0 {
        return f64::INFINITY;
    }
    if env.is_off() || env.decay <= 0.0 {
        return length / rate;
    }
    let d = env.decay;
    let c = env.amount * std::f64::consts::LN_2 / 12.0;
    let g1 = bend_integral(c, 1.0);
    // buffer seconds read by `t` (0 ≤ t ≤ d)
    let read = |t: f64| rate * d * (g1 - bend_integral(c, 1.0 - t / d));
    let whole = rate * d * g1;
    if whole <= length {
        return d + (length - whole) / rate;
    }
    // read(t) = length on [0, d]: read is increasing with read' = rate·2^(env(t)/12) > 0
    let (mut lo, mut hi) = (0.0, d);
    let mut t = length / whole * d;
    for _ in 0..MAX_SOLVE_STEPS {
        let f = read(t) - length;
        if f.abs() <= 1e-13 * length.max(1e-9) {
            break;
        }
        if f > 0.0 {
            hi = t;
        } else {
            lo = t;
        }
        let slope = rate * (env.value(t) / 12.0).exp2();
        let next = t - f / slope;
        t = if next > lo && next < hi {
            next
        } else {
            0.5 * (lo + hi)
        };
    }
    t
}

/// Iteration cap of [`pitched_duration`]'s solve (Newton converges in a handful).
const MAX_SOLVE_STEPS: usize = 64;
/// Term cap of [`bend_integral`] (|c| ≤ 48·ln2/12 ≈ 2.8 for the model's pitch range: ~25
/// terms reach f64 precision).
const MAX_SERIES_TERMS: usize = 96;

/// `∫₀ᵘ e^(c·v³) dv = Σₖ cᵏ·u^(3k+1) / (k!·(3k+1))`, for `0 ≤ u ≤ 1`.
pub fn bend_integral(c: f64, u: f64) -> f64 {
    let u = u.clamp(0.0, 1.0);
    let u3 = u * u * u;
    let mut term = u; // cᵏ·u^(3k+1)/k!
    let mut sum = 0.0;
    for k in 0..MAX_SERIES_TERMS {
        let add = term / (3 * k + 1) as f64;
        sum += add;
        if add.abs() <= 1e-17 * sum.abs() && k > 2 {
            break;
        }
        term *= c * u3 / (k + 1) as f64;
    }
    sum
}

/// The filter's response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SvfMode {
    #[default]
    LowPass,
    HighPass,
    /// Band-pass normalised to unity gain at the centre.
    BandPass,
}

/// The pad low-pass's Q (`Tone.Filter` Q 0.707 read as dB by Chromium's lowpass: 10^(0.707/20)).
pub const BASE_Q: f64 = 1.084_726_9;

/// Resonance 0..1 → filter Q: the classic pad low-pass's Q at 0, ×16 (≈ 17) at 1.
pub fn resonance_q(res: f64) -> f64 {
    BASE_Q * 16f64.powf(res.clamp(0.0, 1.0))
}

/// Topology-preserving-transform state-variable filter (Zavalishin / Simper): a bilinear,
/// prewarped 2-pole whose cutoff and Q can change every sample without blowing up or clicking.
/// Its low-pass matches the pad's biquad low-pass (same analog prototype and transform).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Svf {
    ic1: f32,
    ic2: f32,
}

/// One sample's coefficients (`g = tan(π·fc/sr)`, `k = 1/Q`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SvfCoefs {
    a1: f32,
    a2: f32,
    a3: f32,
    k: f32,
}

impl SvfCoefs {
    /// `cutoff` is clamped to 10 Hz .. 0.49·sr.
    #[inline]
    pub fn new(cutoff: f32, q: f32, sr: f32) -> Self {
        let fc = cutoff.clamp(10.0, sr * 0.49);
        let g = (PI * fc / sr).tan();
        let k = 1.0 / q.max(0.05);
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        Self { a1, a2, a3, k }
    }
}

impl Svf {
    /// The silent state.
    pub const ZERO: Svf = Svf { ic1: 0.0, ic2: 0.0 };

    #[inline]
    pub fn tick(&mut self, x: f32, c: &SvfCoefs, mode: SvfMode) -> f32 {
        let v3 = x - self.ic2;
        let v1 = c.a1 * self.ic1 + c.a2 * v3;
        let v2 = self.ic2 + c.a2 * self.ic1 + c.a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        // Keep the state out of the denormal range once the input is gone.
        if self.ic1.abs() < 1e-20 {
            self.ic1 = 0.0;
        }
        if self.ic2.abs() < 1e-20 {
            self.ic2 = 0.0;
        }
        match mode {
            SvfMode::LowPass => v2,
            SvfMode::BandPass => c.k * v1,
            SvfMode::HighPass => x - c.k * v1 - v2,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Soft-clip drive, 0..1. 0 is an exact bypass. The shaper is `tanh(g·x) / tanh(g)` with
/// `g` = 0..+30 dB (peaks stay at ±1, quiet parts are lifted: the "punch" of a driven kick),
/// cross-faded in over the first quarter of the range so the knob has no step at zero.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Drive {
    wet: f32,
    g: f32,
    norm: f32,
}

impl Drive {
    pub fn new(amount: f64) -> Self {
        let a = amount.clamp(0.0, 1.0);
        let g = 10f64.powf(1.5 * a);
        Self {
            wet: (a * 4.0).min(1.0) as f32,
            g: g as f32,
            norm: (1.0 / g.tanh()) as f32,
        }
    }

    /// Interpolate two drives' parameters (`f` 0..1): a block's per-sample drive.
    #[inline]
    pub fn lerp(a: &Drive, b: &Drive, f: f32) -> Drive {
        Drive {
            wet: a.wet + (b.wet - a.wet) * f,
            g: a.g + (b.g - a.g) * f,
            norm: a.norm + (b.norm - a.norm) * f,
        }
    }

    pub fn is_bypass(&self) -> bool {
        self.wet == 0.0
    }

    #[inline]
    pub fn tick(&self, x: f32) -> f32 {
        if self.wet == 0.0 {
            return x;
        }
        let s = (self.g * x).tanh() * self.norm;
        x + self.wet * (s - x)
    }
}

impl Default for Drive {
    fn default() -> Self {
        Self::new(0.0)
    }
}

/// One-pole smoother advanced once per block: [`Smoother::block`] gives the value at the block's
/// start and end, to be interpolated linearly over the block (no per-sample `exp`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Smoother {
    pub value: f64,
}

impl Smoother {
    /// Time constant of every shaping parameter.
    pub const TAU: f64 = 0.015;

    pub fn new(v: f64) -> Self {
        Self { value: v }
    }

    /// Advance by `n` frames towards `target`; returns `(start, end)`. Snaps when within 1e-6
    /// (relative), so a parameter settles on its exact target (drive 0 = exact bypass again).
    pub fn block(&mut self, target: f64, n: usize, sr: f64) -> (f64, f64) {
        let start = self.value;
        let k = 1.0 - (-(n as f64) / (Self::TAU * sr)).exp();
        let mut next = start + (target - start) * k;
        if (target - next).abs() <= 1e-6 * target.abs().max(1.0) {
            next = target;
        }
        self.value = next;
        (start, next)
    }
}

#[cfg(test)]
mod tests;
