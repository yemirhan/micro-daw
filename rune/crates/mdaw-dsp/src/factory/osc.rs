//! Web Audio `OscillatorNode` with Chromium's band-limited `PeriodicWave` wavetables.
//!
//! Instead of polyBLEP, this reproduces Chromium's algorithm (`periodic_wave.cc`,
//! `oscillator_node.cc`):
//! - table size 2048 (sr ≤ 24 kHz), 4096 (sr ≤ 88.2 kHz) or 16384; `half = size/2` partials max;
//! - 3 ranges per octave (`round(3·log2 size)` ranges, 400 cents each). Range `r` keeps
//!   harmonics `1..=floor(half · 2^(−r/3))`; harmonic `n` of the built-in shapes:
//!   sine `b1 = 1`, square `b_n = 4/(nπ)` (odd n), triangle `b_n = 8/(π²n²)·(−1)^((n−1)/2)` (odd n),
//!   sawtooth `b_n = 2/(nπ)·(−1)^(n+1)`; `table[k] = Σ b_n·sin(2π·n·k/size)` (evaluated exactly via
//!   an integer-indexed sine table instead of an inverse FFT);
//! - every table is scaled by `1 / max|range-0 table|` (normalisation: a full-band square's Gibbs
//!   peak becomes 1, so its plateau sits at ≈ 0.85);
//! - per sample: `pitchRange = clamp(1 + 3·log2(f / (nyquist/half)), 0, ranges−1)`, read the two
//!   neighbouring tables with linear interpolation (Chromium uses linear interpolation for phase
//!   increments ≥ 0.3 table samples, i.e. above ≈ 3 Hz) and cross-fade them by the fractional range;
//! - phase starts at 0 at the start frame and advances by `f · size/sr` (f32) after each sample.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OscType {
    Sine,
    Square,
    Triangle,
    Sawtooth,
}

/// Wavetable size Chromium picks for a sample rate.
pub fn periodic_wave_size(sample_rate: u32) -> usize {
    if sample_rate <= 24000 {
        2048
    } else if sample_rate <= 88200 {
        4096
    } else {
        16384
    }
}

/// The oscillator shapes the factory kit plays (what [`prewarm_wavetables`] builds).
pub const KIT_SHAPES: [OscType; 3] = [OscType::Sine, OscType::Square, OscType::Triangle];

/// One lazily built table set; the map only hands out cells, building happens outside its lock.
type TableCell = Arc<OnceLock<Arc<WaveTables>>>;
type TableCache = Mutex<HashMap<(OscType, usize), TableCell>>;

fn table_cache() -> &'static TableCache {
    static CACHE: OnceLock<TableCache> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// Lock a mutex, recovering the data if a previous holder panicked: the map only ever holds
/// fully inserted cells, so a poisoned guard is still consistent.
fn lock_tolerant<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Build (once per process) the [`KIT_SHAPES`] wavetables for every rate in `sample_rates`.
///
/// Building the 16384-point tables Chromium uses above 88.2 kHz takes long enough to glitch a
/// first render, so call this from a worker thread when the engine's rate is known. Rates that
/// share a table size are built once; already built tables are skipped.
pub fn prewarm_wavetables(sample_rates: &[u32]) {
    let mut sizes: Vec<usize> = sample_rates
        .iter()
        .map(|&sr| periodic_wave_size(sr))
        .collect();
    sizes.sort_unstable();
    sizes.dedup();
    for size in sizes {
        for shape in KIT_SHAPES {
            WaveTables::get(shape, size);
        }
    }
}

#[derive(Debug)]
pub struct WaveTables {
    size: usize,
    tables: Vec<Vec<f32>>,
}

impl WaveTables {
    fn build(shape: OscType, size: usize) -> Self {
        let half = size / 2;
        let ranges = (3.0f32 * (size as f32).log2()).round() as usize;
        let sin_table: Vec<f64> = (0..size)
            .map(|m| (2.0 * std::f64::consts::PI * m as f64 / size as f64).sin())
            .collect();
        let pi = std::f64::consts::PI;
        let coef = |n: usize| -> f64 {
            let nf = n as f64;
            match shape {
                OscType::Sine => {
                    if n == 1 {
                        1.0
                    } else {
                        0.0
                    }
                }
                OscType::Square => {
                    if n & 1 == 1 {
                        4.0 / (nf * pi)
                    } else {
                        0.0
                    }
                }
                OscType::Sawtooth => {
                    let s = if n & 1 == 1 { 1.0 } else { -1.0 };
                    2.0 / (nf * pi) * s
                }
                OscType::Triangle => {
                    if n & 1 == 1 {
                        let s = if ((n - 1) >> 1) & 1 == 1 { -1.0 } else { 1.0 };
                        8.0 / (pi * pi * nf * nf) * s
                    } else {
                        0.0
                    }
                }
            }
        };
        let coefs: Vec<(usize, f64)> = (1..half)
            .map(|n| (n, coef(n)))
            .filter(|&(_, b)| b != 0.0)
            .collect();
        let mut tables = Vec::with_capacity(ranges);
        let mut scale = 1.0f32;
        for r in 0..ranges {
            let culling = 2f64.powf(-((r as f32 * 400.0) as f64) / 1200.0);
            let partials = ((culling * half as f64) as usize).min(half);
            let mut t = vec![0f32; size];
            for (k, out) in t.iter_mut().enumerate() {
                let mut acc = 0.0f64;
                for &(n, b) in coefs.iter().take_while(|(n, _)| *n <= partials) {
                    acc += b * sin_table[(n * k) % size];
                }
                *out = acc as f32;
            }
            if r == 0 {
                let max = t.iter().fold(0f32, |m, &x| m.max(x.abs()));
                if max > 0.0 {
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

    /// Shared tables for (shape, size), built once per process. Safe from any thread: concurrent
    /// requests for the same tables build them once; different tables build in parallel.
    pub fn get(shape: OscType, size: usize) -> Arc<WaveTables> {
        let cell = lock_tolerant(table_cache())
            .entry((shape, size))
            .or_default()
            .clone();
        cell.get_or_init(|| Arc::new(Self::build(shape, size)))
            .clone()
    }

    /// Whether the tables for (shape, size) are already built.
    pub fn is_cached(shape: OscType, size: usize) -> bool {
        lock_tolerant(table_cache())
            .get(&(shape, size))
            .is_some_and(|cell| cell.get().is_some())
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn table(&self, range: usize) -> &[f32] {
        &self.tables[range]
    }

    /// (higher-partials table, lower-partials table, interpolation factor) for `f` Hz.
    fn select(&self, f: f32, sample_rate: u32) -> (usize, usize, f32) {
        let ranges = self.tables.len();
        let nyquist = 0.5f32 * sample_rate as f32;
        let lowest = nyquist / (self.size / 2) as f32;
        let f = f.abs();
        let ratio = if f > 0.0 { f / lowest } else { 0.5 };
        let cents = ratio.log2() * 1200.0;
        let pitch_range = (1.0 + cents / 400.0).max(0.0).min((ranges - 1) as f32);
        let r1 = pitch_range as usize;
        let r2 = if r1 < ranges - 1 { r1 + 1 } else { r1 };
        (r1, r2, pitch_range - r1 as f32)
    }
}

/// Render an oscillator of `shape` with per-frame `frequency` (Hz), starting (phase 0) at frame
/// `start` and silent before. The caller applies the stop gain.
pub fn render_oscillator(
    shape: OscType,
    frequency: &[f32],
    start: usize,
    sample_rate: u32,
) -> Vec<f32> {
    let wt = WaveTables::get(shape, periodic_wave_size(sample_rate));
    let size = wt.size();
    let mask = size - 1;
    let rate_scale = size as f32 / sample_rate as f32;
    let mut out = vec![0f32; frequency.len()];
    let mut phase = 0f64;
    for k in start..frequency.len() {
        let f = frequency[k];
        let incr = f * rate_scale;
        let (hi, lo, tf) = wt.select(f, sample_rate);
        let (hi, lo) = (wt.table(hi), wt.table(lo));
        let i0 = phase as usize;
        let i1 = (i0 + 1) & mask;
        let frac = phase - i0 as f64;
        let s_hi = (1.0 - frac) * hi[i0] as f64 + frac * hi[i1] as f64;
        let s_lo = (1.0 - frac) * lo[i0] as f64 + frac * lo[i1] as f64;
        out[k] = ((1.0 - tf as f64) * s_hi + tf as f64 * s_lo) as f32;
        phase += incr as f64;
        phase -= (phase / size as f64).floor() * size as f64;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_is_a_sine() {
        let sr = 48000;
        let f = vec![1000f32; 480];
        let y = render_oscillator(OscType::Sine, &f, 0, sr);
        for (k, v) in y.iter().enumerate() {
            let want = (2.0 * std::f64::consts::PI * 1000.0 * k as f64 / sr as f64).sin();
            assert!((*v as f64 - want).abs() < 2e-5, "{k}: {v} vs {want}");
        }
    }

    #[test]
    fn prewarm_populates_the_table_cache() {
        prewarm_wavetables(&[22050, 22050, 44100]);
        for size in [2048, 4096] {
            for shape in KIT_SHAPES {
                assert!(WaveTables::is_cached(shape, size), "{shape:?} {size}");
            }
        }
        // A prewarmed set is the one renders get.
        let a = WaveTables::get(OscType::Triangle, 2048);
        let b = WaveTables::get(OscType::Triangle, 2048);
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(a.size(), 2048);
    }

    #[test]
    fn concurrent_gets_share_one_build() {
        let got: Vec<Arc<WaveTables>> = std::thread::scope(|s| {
            let hs: Vec<_> = (0..4)
                .map(|_| s.spawn(|| WaveTables::get(OscType::Sine, 2048)))
                .collect();
            hs.into_iter().map(|h| h.join().unwrap()).collect()
        });
        assert!(got.iter().all(|t| Arc::ptr_eq(t, &got[0])));
    }

    #[test]
    fn lock_tolerates_poisoning() {
        let m = Mutex::new(1);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _g = m.lock().unwrap();
            panic!("poison");
        }));
        assert!(m.is_poisoned());
        *lock_tolerant(&m) += 1;
        assert_eq!(*lock_tolerant(&m), 2);
    }

    #[test]
    fn square_is_normalised_and_band_limited() {
        let wt = WaveTables::get(OscType::Square, 4096);
        let max0 = wt.table(0).iter().fold(0f32, |m, x| m.max(x.abs()));
        assert!((max0 - 1.0).abs() < 1e-6);
        // Plateau of the full-band square ≈ 1/1.179.
        let mid = wt.table(0)[1024];
        assert!((mid - 0.849).abs() < 0.01, "{mid}");
        let y = render_oscillator(OscType::Square, &vec![205.3f32; 44100], 0, 44100);
        let peak = y.iter().fold(0f32, |m, x| m.max(x.abs()));
        assert!(peak > 0.85 && peak <= 1.0, "{peak}");
    }
}
