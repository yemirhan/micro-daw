//! Port of `src/audio/sampler/classic/dsp.ts`: a tiny, deterministic, sample-accurate DSP toolkit
//! for the built-in classic kits. Everything is pure (`Vec<f32>` in, `Vec<f32>` out), renders at
//! 44.1 kHz mono and uses a seeded PRNG, so a voice renders to exactly the same samples every time.
//!
//! Numeric model (≙ JS): buffers are `f32` (`Float32Array`), every computation is `f64` and a value
//! is rounded to `f32` only when it is written into a buffer. Filter / oscillator state stays `f64`.

use mdaw_model::js;

pub const SR: f64 = 44100.0;
/// -60 dBFS: trailing audio below this is trimmed.
pub const SILENCE: f64 = 0.001;

const TAU: f64 = std::f64::consts::PI * 2.0;

/// Peak level every rendered voice is normalized to (-1 dBFS): `10 ** (-1 / 20)`.
pub fn target_peak() -> f64 {
    js::pow(10.0, -1.0 / 20.0)
}

fn ln1000() -> f64 {
    js::log(1000.0)
}

/// `Math.sign` (keeps ±0 and NaN, unlike `f64::signum`).
fn sign(x: f64) -> f64 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        x
    }
}

/// `samples(sec)`: `Math.max(1, Math.round(sec * SR))`.
pub fn samples(sec: f64) -> usize {
    js::max(1.0, js::round(sec * SR)) as usize
}

/// mulberry32: small, fast, seeded PRNG in [0, 1).
#[derive(Debug, Clone)]
pub struct Rng(u32);

impl Rng {
    /// `makeRng(seed)` (`seed >>> 0`).
    pub fn new(seed: u32) -> Self {
        Self(seed)
    }

    pub fn next_f64(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x6d2b_79f5);
        let mut t = self.0;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        (t ^ (t >> 14)) as f64 / 4_294_967_296.0
    }
}

/// White noise in [-1, 1).
pub fn noise(n: usize, seed: u32) -> Vec<f32> {
    let mut rnd = Rng::new(seed);
    (0..n)
        .map(|_| (rnd.next_f64() * 2.0 - 1.0) as f32)
        .collect()
}

// ---------------------------------------------------------------------------
// Envelopes
// ---------------------------------------------------------------------------

/// `EnvOpts`: attack / hold / exponential decay (`t60` = time to decay by 60 dB), all seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnvOpts {
    pub attack: f64,
    pub hold: f64,
    pub t60: f64,
    pub delay: f64,
}

impl EnvOpts {
    /// The TS defaults (`attack = 0.0005, hold = 0, delay = 0`); `t60` is required there.
    pub const D: EnvOpts = EnvOpts {
        attack: 0.0005,
        hold: 0.0,
        t60: 0.0,
        delay: 0.0,
    };

    pub const fn t60(t60: f64) -> Self {
        EnvOpts { t60, ..Self::D }
    }
}

/// Attack / hold / exponential decay amplitude envelope.
pub fn env(n: usize, o: EnvOpts) -> Vec<f32> {
    let d = o.delay * SR;
    let a = js::max(1.0, o.attack * SR);
    let h = o.hold * SR;
    let k = ln1000() / (js::max(0.001, o.t60) * SR);
    (0..n)
        .map(|i| {
            let j = i as f64 - d;
            let v = if j < 0.0 {
                0.0
            } else if j < a {
                j / a
            } else if j < a + h {
                1.0
            } else {
                (-k * (j - a - h)).exp()
            };
            v as f32
        })
        .collect()
}

/// Sum of envelopes (e.g. a fast initial drop into a slower tail).
pub fn env_sum(n: usize, parts: &[(f64, EnvOpts)]) -> Vec<f32> {
    let mut out = vec![0.0; n];
    for &(g, o) in parts {
        mix_into(&mut out, &env(n, o), g, 0);
    }
    out
}

/// `clapEnv` options.
#[derive(Debug, Clone, PartialEq)]
pub struct ClapEnvOpts<'a> {
    pub bursts: &'a [f64],
    pub burst_t60: f64,
    pub tail_delay: f64,
    pub tail_t60: f64,
    pub tail_gain: f64,
}

/// Hand-clap envelope: a few fast bursts followed by a longer diffuse "room" tail.
pub fn clap_env(n: usize, o: &ClapEnvOpts) -> Vec<f32> {
    let mut out = vec![0.0; n];
    for &b in o.bursts {
        let e = EnvOpts {
            attack: 0.0003,
            t60: o.burst_t60,
            delay: b,
            ..EnvOpts::D
        };
        mix_into(&mut out, &env(n, e), 1.0, 0);
    }
    let tail = EnvOpts {
        attack: 0.002,
        t60: o.tail_t60,
        delay: o.tail_delay,
        ..EnvOpts::D
    };
    mix_into(&mut out, &env(n, tail), o.tail_gain, 0);
    out
}

// ---------------------------------------------------------------------------
// Oscillators
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Sine,
    Tri,
}

/// `OscOpts` (see dsp.ts for the meaning of each field).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OscOpts {
    pub freq: f64,
    pub sweep: f64,
    pub sweep_tau: f64,
    pub sweep2: f64,
    pub sweep2_tau: f64,
    pub drift: f64,
    pub shape: Shape,
    pub phase: f64,
}

impl OscOpts {
    /// The TS defaults; `freq` is required there.
    pub const D: OscOpts = OscOpts {
        freq: 0.0,
        sweep: 0.0,
        sweep_tau: 0.01,
        sweep2: 0.0,
        sweep2_tau: 0.05,
        drift: 1.0,
        shape: Shape::Sine,
        phase: 0.0,
    };
}

/// Oscillator with an exponential pitch envelope (bridged-T / VCO style).
pub fn osc(n: usize, o: OscOpts) -> Vec<f32> {
    let mut out = Vec::with_capacity(n);
    let mut ph = o.phase;
    let ln_drift = js::log(o.drift);
    for i in 0..n {
        let t = i as f64 / SR;
        let drift = if o.drift == 1.0 {
            1.0
        } else {
            (ln_drift * t).exp()
        };
        let f = o.freq
            * (1.0 + o.sweep * (-t / o.sweep_tau).exp() + o.sweep2 * (-t / o.sweep2_tau).exp())
            * drift;
        let v = match o.shape {
            Shape::Sine => (TAU * ph).sin(),
            Shape::Tri => 1.0 - 4.0 * (((ph + 0.25) % 1.0) - 0.5).abs(),
        };
        out.push(v as f32);
        ph += f / SR;
        ph -= ph.floor();
    }
    out
}

fn poly_blep(t: f64, dt: f64) -> f64 {
    if t < dt {
        let x = t / dt;
        return x + x - x * x - 1.0;
    }
    if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        return x * x + x + x + 1.0;
    }
    0.0
}

/// Band-limited (polyBLEP) square wave.
pub fn square(n: usize, freq: f64, phase: f64) -> Vec<f32> {
    let dt = freq / SR;
    let mut ph = phase % 1.0;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let mut v = if ph < 0.5 { 1.0 } else { -1.0 };
        v += poly_blep(ph, dt);
        v -= poly_blep((ph + 0.5) % 1.0, dt);
        out.push(v as f32);
        ph += dt;
        if ph >= 1.0 {
            ph -= 1.0;
        }
    }
    out
}

/// "Metal" source: several detuned square oscillators summed, seeded random start phases.
pub fn metal(n: usize, freqs: &[f64], seed: u32) -> Vec<f32> {
    let mut rnd = Rng::new(seed);
    let mut out = vec![0.0; n];
    let g = 1.0 / freqs.len() as f64;
    for &f in freqs {
        let ph = rnd.next_f64();
        mix_into(&mut out, &square(n, f, ph), g, 0);
    }
    out
}

/// Sum of exponentially decaying sine partials: `(freq, amplitude, t60)`.
pub fn partials(n: usize, list: &[(f64, f64, f64)], attack: f64) -> Vec<f32> {
    let mut out = vec![0.0; n];
    for (i, &(f, a, t60)) in list.iter().enumerate() {
        let s = osc(
            n,
            OscOpts {
                freq: f,
                phase: (i as f64 * 0.137) % 1.0,
                ..OscOpts::D
            },
        );
        let s = mul(
            s,
            &env(
                n,
                EnvOpts {
                    attack,
                    t60,
                    ..EnvOpts::D
                },
            ),
        );
        mix_into(&mut out, &s, a, 0);
    }
    out
}

/// `partials`' default attack.
pub const PARTIALS_ATTACK: f64 = 0.0005;

// ---------------------------------------------------------------------------
// Filters (RBJ biquads, run in place)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterType {
    Lp,
    Hp,
    Bp,
    Peak,
}

/// `Math.SQRT1_2`, the biquads' default Q.
pub const DEFAULT_Q: f64 = std::f64::consts::FRAC_1_SQRT_2;

pub fn biquad(mut buf: Vec<f32>, ty: FilterType, freq: f64, q: f64, gain_db: f64) -> Vec<f32> {
    let f = js::min(freq, SR * 0.49);
    let w0 = (TAU * f) / SR;
    let cos = w0.cos();
    let alpha = w0.sin() / (2.0 * q);
    let (mut b0, mut b1, mut b2, a0, mut a1, mut a2);
    match ty {
        FilterType::Lp => {
            b0 = (1.0 - cos) / 2.0;
            b1 = 1.0 - cos;
            b2 = b0;
            a0 = 1.0 + alpha;
            a1 = -2.0 * cos;
            a2 = 1.0 - alpha;
        }
        FilterType::Hp => {
            b0 = (1.0 + cos) / 2.0;
            b1 = -(1.0 + cos);
            b2 = b0;
            a0 = 1.0 + alpha;
            a1 = -2.0 * cos;
            a2 = 1.0 - alpha;
        }
        FilterType::Bp => {
            b0 = alpha;
            b1 = 0.0;
            b2 = -alpha;
            a0 = 1.0 + alpha;
            a1 = -2.0 * cos;
            a2 = 1.0 - alpha;
        }
        FilterType::Peak => {
            let a = js::pow(10.0, gain_db / 40.0);
            b0 = 1.0 + alpha * a;
            b1 = -2.0 * cos;
            b2 = 1.0 - alpha * a;
            a0 = 1.0 + alpha / a;
            a1 = -2.0 * cos;
            a2 = 1.0 - alpha / a;
        }
    }
    b0 /= a0;
    b1 /= a0;
    b2 /= a0;
    a1 /= a0;
    a2 /= a0;
    let (mut x1, mut x2, mut y1, mut y2) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for s in buf.iter_mut() {
        let x = *s as f64;
        let y = b0 * x + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
        x2 = x1;
        x1 = x;
        y2 = y1;
        y1 = y;
        *s = y as f32;
    }
    buf
}

/// `lp(b, f, q?)` (`None` = default Q).
pub fn lp(b: Vec<f32>, f: f64, q: Option<f64>) -> Vec<f32> {
    biquad(b, FilterType::Lp, f, q.unwrap_or(DEFAULT_Q), 0.0)
}

pub fn hp(b: Vec<f32>, f: f64, q: Option<f64>) -> Vec<f32> {
    biquad(b, FilterType::Hp, f, q.unwrap_or(DEFAULT_Q), 0.0)
}

pub fn bp(b: Vec<f32>, f: f64, q: Option<f64>) -> Vec<f32> {
    biquad(b, FilterType::Bp, f, q.unwrap_or(DEFAULT_Q), 0.0)
}

pub fn peak(b: Vec<f32>, f: f64, q: f64, gain_db: f64) -> Vec<f32> {
    biquad(b, FilterType::Peak, f, q, gain_db)
}

/// One-pole DC blocker (≈ 10 Hz).
pub fn dc_block(mut buf: Vec<f32>, hz: f64) -> Vec<f32> {
    let r = ((-TAU * hz) / SR).exp();
    let (mut x1, mut y1) = (0.0f64, 0.0f64);
    for s in buf.iter_mut() {
        let x = *s as f64;
        let y = x - x1 + r * y1;
        x1 = x;
        y1 = y;
        *s = y as f32;
    }
    buf
}

// ---------------------------------------------------------------------------
// Arithmetic / shaping
// ---------------------------------------------------------------------------

/// `dst += src * gain` (src may be shorter; `offset` delays it, in samples).
pub fn mix_into(dst: &mut [f32], src: &[f32], gain: f64, offset: usize) {
    let end = dst.len().min(src.len() + offset);
    for i in offset..end {
        dst[i] = (dst[i] as f64 + src[i - offset] as f64 * gain) as f32;
    }
}

/// `a *= b`, element-wise (in place); samples past `b`'s end become 0.
pub fn mul(mut a: Vec<f32>, b: &[f32]) -> Vec<f32> {
    let n = a.len().min(b.len());
    for i in 0..n {
        a[i] = (a[i] as f64 * b[i] as f64) as f32;
    }
    for v in &mut a[n..] {
        *v = 0.0;
    }
    a
}

pub fn scale(mut a: Vec<f32>, g: f64) -> Vec<f32> {
    for v in a.iter_mut() {
        *v = (*v as f64 * g) as f32;
    }
    a
}

pub fn peak_of(a: &[f32]) -> f64 {
    a.iter().fold(0.0f64, |p, &v| {
        let v = (v as f64).abs();
        if v > p { v } else { p }
    })
}

/// Normalize to peak `target` (no-op for silence).
pub fn normalize(a: Vec<f32>, target: f64) -> Vec<f32> {
    let p = peak_of(&a);
    if p > 0.0 { scale(a, target / p) } else { a }
}

/// Soft clip: tanh(drive·x), level-compensated so a full-scale input stays full scale.
pub fn drive(mut a: Vec<f32>, amount: f64) -> Vec<f32> {
    // `peakOf(a) || 1`
    let p = peak_of(&a);
    let p = if p == 0.0 { 1.0 } else { p };
    let k = amount.tanh();
    for v in a.iter_mut() {
        *v = (((amount * *v as f64) / p).tanh() / k * p) as f32;
    }
    a
}

/// `lofi` options (`rate = SR, bits = 16, mu = false`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LofiOpts {
    pub rate: f64,
    pub bits: i32,
    pub mu: bool,
}

impl LofiOpts {
    pub const D: LofiOpts = LofiOpts {
        rate: SR,
        bits: 16,
        mu: false,
    };
}

/// Early-sampler character: band-limit, sample-and-hold at `rate`, quantize to `bits` (optionally
/// μ-law companded) at full scale, and filter the images.
pub fn lofi(mut buf: Vec<f32>, o: LofiOpts) -> Vec<f32> {
    let p = peak_of(&buf);
    if p == 0.0 {
        return buf;
    }
    if o.rate < SR {
        buf = lp(buf, o.rate * 0.45, None);
        buf = lp(buf, o.rate * 0.45, None);
    }
    let l = js::pow(2.0, (o.bits - 1) as f64) - 1.0;
    const MU: f64 = 255.0;
    let ln_mu = js::log(1.0 + MU);
    let step = SR / o.rate;
    let mut acc = step;
    let mut held = 0.0f64;
    for s in buf.iter_mut() {
        if acc >= step {
            acc -= step;
            let x = *s as f64 / p;
            if o.mu {
                let y = (sign(x) * js::log(1.0 + MU * x.abs())) / ln_mu;
                let q = js::round(y * l) / l;
                held = (sign(q) * (js::pow(1.0 + MU, q.abs()) - 1.0)) / MU;
            } else {
                held = js::round(x * l) / l;
            }
        }
        acc += 1.0;
        *s = (held * p) as f32;
    }
    if o.rate < SR {
        buf = lp(buf, js::min(o.rate * 0.48, SR * 0.45), None);
    }
    buf
}

// ---------------------------------------------------------------------------
// Finishing
// ---------------------------------------------------------------------------

/// `finalize` options (`maxSec = 3, padSec = 0.01, fadeSec = 0.006`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FinalizeOpts {
    pub max_sec: f64,
    pub pad_sec: f64,
    pub fade_sec: f64,
}

impl FinalizeOpts {
    pub const D: FinalizeOpts = FinalizeOpts {
        max_sec: 3.0,
        pad_sec: 0.01,
        fade_sec: 0.006,
    };
}

/// Turn a raw render into a pad sample: DC-block, normalize to -1 dBFS, cut everything after the
/// last sample above -60 dBFS (plus `padSec`), cap at `maxSec` and fade the last few ms to zero.
pub fn finalize(raw: &[f32], o: FinalizeOpts) -> Vec<f32> {
    let cap = raw.len().min(samples(o.max_sec));
    let buf = dc_block(raw[..cap].to_vec(), 10.0);
    let buf = normalize(buf, target_peak());
    let last = buf
        .iter()
        .rposition(|&v| (v as f64).abs() > SILENCE)
        .unwrap_or(0);
    let end = buf.len().min(last + 1 + samples(o.pad_sec));
    let mut out = buf;
    out.truncate(end.max(1));
    let fade = out.len().min(samples(o.fade_sec));
    let len = out.len();
    for i in 0..fade {
        let v = &mut out[len - 1 - i];
        *v = (*v as f64 * (i as f64 / fade as f64)) as f32;
    }
    out
}
