//! Chromium `PeriodicWave` band-limited tables (`periodic_wave.cc`) for every waveform the
//! synth voices play:
//!
//! - **native types** (`OscillatorNode.type = 'sine' | 'square' | 'sawtooth' | 'triangle'`, which
//!   Tone uses for a phase-0 oscillator): Chromium's `GenerateBasicWaveform` coefficients, computed
//!   in f32 (`pi_factor = 2 / (n·π)`), imaginary parts only;
//! - **Tone periodic waves** (`context.createPeriodicWave(real, imag)`), which Tone builds in
//!   `Oscillator._getRealImaginary` for custom partials (organ drawbars) and for any oscillator
//!   with a non-zero phase (the detuned voices of a `FatOscillator` sit at `i/count·360°`):
//!   `real[n] = −b_n·sin(nφ)`, `imag[n] = b_n·cos(nφ)` stored in `Float32Array`s, normalised.
//!
//! `CreateBandLimitedTables`: table size 2048 (sr ≤ 24 kHz) / 4096 (≤ 88.2 kHz) / 16384, 3 ranges
//! per octave (`round(3·log2 size)` ranges); range `r` keeps harmonics `n ≤ floor(half·2^(−r/3))`
//! (and `n < number_of_components`); `table[k] = Σ a_n·cos(2πnk/N) + b_n·sin(2πnk/N)` (an inverse
//! FFT here, f64), and every range is scaled by `1 / max|range 0|` unless normalisation is off.
//!
//! Tables are built on the control side (voice construction) and shared through a process-wide
//! cache; the audio thread only reads them.

use std::collections::HashMap;
use std::f64::consts::PI;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BasicShape {
    Sine,
    Square,
    Sawtooth,
    Triangle,
}

/// Wavetable size Chromium picks for a sample rate.
pub fn periodic_wave_size(sample_rate: f64) -> usize {
    if sample_rate <= 24000.0 {
        2048
    } else if sample_rate <= 88200.0 {
        4096
    } else {
        16384
    }
}

#[derive(Debug)]
pub struct WaveTables {
    size: usize,
    tables: Vec<Vec<f32>>,
}

impl WaveTables {
    pub fn size(&self) -> usize {
        self.size
    }

    #[cfg(test)]
    pub fn ranges(&self) -> usize {
        self.tables.len()
    }

    pub fn table(&self, range: usize) -> &[f32] {
        &self.tables[range]
    }

    /// `CreateBandLimitedTables(real, imag, number_of_components, disable_normalization=false)`.
    fn build(real: &[f32], imag: &[f32], size: usize) -> Self {
        let half = size / 2;
        let components = real.len().min(imag.len()).min(half);
        let ranges = (3.0f32 * (size as f32).log2()).round() as usize;
        let mut tables = Vec::with_capacity(ranges);
        let mut scale = 1.0f32;
        let mut re = vec![0f64; size];
        let mut im = vec![0f64; size];
        for r in 0..ranges {
            let cents_to_cull = r as f32 * 400.0;
            let culling = 2f64.powf(-(cents_to_cull as f64) / 1200.0);
            let partials = (culling * half as f64) as usize;
            let keep = components.min(partials + 1);
            re.fill(0.0);
            im.fill(0.0);
            // Σ (a − i·b)·e^{+iθ} has real part a·cosθ + b·sinθ.
            for n in 1..keep {
                re[n] = real[n] as f64;
                im[n] = -(imag[n] as f64);
            }
            fft_inverse(&mut re, &mut im);
            let mut t: Vec<f32> = re.iter().map(|&x| x as f32).collect();
            if r == 0 {
                let max = t.iter().fold(0f32, |m, &x| m.max(x.abs()));
                if max != 0.0 {
                    scale = 1.0 / max;
                }
            }
            for x in &mut t {
                *x *= scale;
            }
            tables.push(t);
        }
        Self { size, tables }
    }

    /// (higher-partials range, lower-partials range, interpolation factor) for `f` Hz
    /// (`WaveDataForFundamentalFrequency`).
    #[inline]
    pub fn select(&self, f: f32, lowest: f32) -> (usize, usize, f32) {
        let ranges = self.tables.len();
        let f = f.abs();
        let ratio = if f > 0.0 { f / lowest } else { 0.5 };
        let cents = ratio.log2() * 1200.0;
        let pitch_range = (1.0 + cents / 400.0).max(0.0).min((ranges - 1) as f32);
        let r1 = pitch_range as usize;
        let r2 = if r1 < ranges - 1 { r1 + 1 } else { r1 };
        (r1, r2, pitch_range - r1 as f32)
    }
}

/// In-place inverse DFT without scaling: `x[k] = Σ X[n]·e^{+2πink/N}` (radix-2, N a power of 2).
fn fft_inverse(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = 2.0 * PI / len as f64;
        let half = len / 2;
        for k in 0..half {
            let (wi, wr) = (ang * k as f64).sin_cos();
            let mut i = k;
            while i < n {
                let a = i + half;
                let tr = re[a] * wr - im[a] * wi;
                let ti = re[a] * wi + im[a] * wr;
                re[a] = re[i] - tr;
                im[a] = im[i] - ti;
                re[i] += tr;
                im[i] += ti;
                i += len;
            }
        }
        len <<= 1;
    }
}

/// Chromium `GenerateBasicWaveform` coefficients (imaginary parts; f32 like Chromium).
fn basic_coefficients(shape: BasicShape, size: usize) -> (Vec<f32>, Vec<f32>) {
    let half = size / 2;
    let real = vec![0f32; half];
    let mut imag = vec![0f32; half];
    let pi = std::f32::consts::PI;
    for (n, b) in imag.iter_mut().enumerate().skip(1) {
        let pi_factor = 2.0f32 / (n as f32 * pi);
        *b = match shape {
            BasicShape::Sine => {
                if n == 1 {
                    1.0
                } else {
                    0.0
                }
            }
            BasicShape::Square => {
                if n & 1 == 1 {
                    2.0 * pi_factor
                } else {
                    0.0
                }
            }
            BasicShape::Sawtooth => pi_factor * if n & 1 == 1 { 1.0 } else { -1.0 },
            BasicShape::Triangle => {
                if n & 1 == 1 {
                    let s = if ((n - 1) >> 1) & 1 == 1 { -1.0 } else { 1.0 };
                    2.0 * (pi_factor * pi_factor) * s
                } else {
                    0.0
                }
            }
        };
    }
    (real, imag)
}

/// Tone `Oscillator._getRealImaginary(type, phase)` for a basic type (2048 coefficients) or
/// custom partials (`partials.len() + 1` meaningful coefficients in 2048-long arrays).
pub fn tone_coefficients(
    shape: Option<BasicShape>,
    partials: &[f64],
    phase: f64,
) -> (Vec<f32>, Vec<f32>) {
    const FFT_SIZE: usize = 4096;
    let mut real = vec![0f32; FFT_SIZE / 2];
    let mut imag = vec![0f32; FFT_SIZE / 2];
    let wave_size = match shape {
        None => {
            if partials.is_empty() {
                return (real, imag);
            }
            partials.len() + 1
        }
        Some(_) => FFT_SIZE / 2,
    };
    for n in 1..wave_size {
        let nf = n as f64;
        let pi_factor = 2.0 / (nf * PI);
        let b = match shape {
            None => partials[n - 1],
            Some(BasicShape::Sine) => {
                if n <= 1 {
                    1.0
                } else {
                    0.0
                }
            }
            Some(BasicShape::Square) => {
                if n & 1 == 1 {
                    2.0 * pi_factor
                } else {
                    0.0
                }
            }
            Some(BasicShape::Sawtooth) => pi_factor * if n & 1 == 1 { 1.0 } else { -1.0 },
            Some(BasicShape::Triangle) => {
                if n & 1 == 1 {
                    2.0 * (pi_factor * pi_factor) * if ((n - 1) >> 1) & 1 == 1 { -1.0 } else { 1.0 }
                } else {
                    0.0
                }
            }
        };
        if b != 0.0 {
            real[n] = (-b * (phase * nf).sin()) as f32;
            imag[n] = (b * (phase * nf).cos()) as f32;
        }
    }
    (real, imag)
}

/// Cache key: the coefficient source and the table size.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Key {
    Basic(BasicShape, usize),
    /// Tone periodic wave: shape (None = custom), partial bits, phase bits, size.
    Tone(Option<BasicShape>, Vec<u64>, u64, usize),
}

type Cache = Mutex<HashMap<Key, Arc<WaveTables>>>;

fn cache() -> &'static Cache {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

fn cached(key: Key, build: impl FnOnce() -> WaveTables) -> Arc<WaveTables> {
    if let Some(t) = cache()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&key)
    {
        return t.clone();
    }
    let t = Arc::new(build());
    cache()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .entry(key)
        .or_insert(t)
        .clone()
}

/// The tables of a native oscillator type.
pub fn basic_wave(shape: BasicShape, sample_rate: f64) -> Arc<WaveTables> {
    let size = periodic_wave_size(sample_rate);
    cached(Key::Basic(shape, size), || {
        let (re, im) = basic_coefficients(shape, size);
        WaveTables::build(&re, &im, size)
    })
}

/// The tables Tone creates for an oscillator of `shape` (None: custom `partials`) at `phase`
/// radians.
pub fn tone_wave(
    shape: Option<BasicShape>,
    partials: &[f64],
    phase: f64,
    sample_rate: f64,
) -> Arc<WaveTables> {
    let size = periodic_wave_size(sample_rate);
    let key = Key::Tone(
        shape,
        partials.iter().map(|p| p.to_bits()).collect(),
        phase.to_bits(),
        size,
    );
    cached(key, || {
        let (re, im) = tone_coefficients(shape, partials, phase);
        WaveTables::build(&re, &im, size)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_table_is_a_unit_sine() {
        let w = basic_wave(BasicShape::Sine, 48000.0);
        assert_eq!(w.size(), 4096);
        assert_eq!(w.ranges(), 36);
        for k in (0..4096).step_by(97) {
            let want = (2.0 * PI * k as f64 / 4096.0).sin();
            assert!((w.table(0)[k] as f64 - want).abs() < 1e-6);
        }
    }

    #[test]
    fn tone_phase_zero_sawtooth_matches_native() {
        let a = basic_wave(BasicShape::Sawtooth, 48000.0);
        let b = tone_wave(Some(BasicShape::Sawtooth), &[], 0.0, 48000.0);
        for r in [0, 10, 30] {
            let e = a
                .table(r)
                .iter()
                .zip(b.table(r))
                .fold(0f32, |m, (x, y)| m.max((x - y).abs()));
            assert!(e < 1e-5, "range {r}: {e}");
        }
    }

    #[test]
    fn custom_partials_keep_only_their_harmonics() {
        let w = tone_wave(None, &[1.0, 0.5], 0.0, 48000.0);
        let t = w.table(0);
        // max of sin θ + 0.5 sin 2θ: cos θ + cos 2θ = 0 → θ = 60°.
        let max = (PI / 3.0).sin() + 0.5 * (2.0 * PI / 3.0).sin();
        let k = 300;
        let th = 2.0 * PI * k as f64 / 4096.0;
        let want = (th.sin() + 0.5 * (2.0 * th).sin()) / max;
        assert!((t[k] as f64 - want).abs() < 1e-4, "{} vs {want}", t[k]);
    }
}
