//! Chromium `OscillatorNode` for LFOs (blink `oscillator_node.cc`, `periodic_wave.cc`), realtime:
//! the band-limited tables of [`mdaw_dsp::factory::osc::WaveTables`], phase in table samples
//! (f64), per-frame increment `f32(freq · size/sr)` (Tone connects the frequency and detune
//! signals, so Chromium always takes the a-rate path), and Chromium's `DoInterpolation`: linear
//! for increments ≥ 0.3 table samples, 3-point Lagrange for ≥ 0.16, 5-point Lagrange below
//! (LFOs live in the Lagrange range), then a cross-fade between the two tables of the pitch range.
//!
//! Tone's LFO with a `phase` other than 0 builds a custom `PeriodicWave` from
//! `real[n] = −b·sin(φn), imag[n] = b·cos(φn)`; the only one the app uses is the chorus' right
//! LFO (sine, φ = 180°): the normalized table is exactly the negated sine table (the 1e-16 real
//! part is below f32 resolution), modelled by [`Osc::negate`].

use std::sync::Arc;

use mdaw_dsp::factory::osc::{OscType, WaveTables, periodic_wave_size};
use mdaw_model::types::LfoWaveform;

const INTERPOLATE_2_POINT: f32 = 0.3;
const INTERPOLATE_3_POINT: f32 = 0.16;

pub fn osc_type(w: LfoWaveform) -> OscType {
    match w {
        LfoWaveform::Sine => OscType::Sine,
        LfoWaveform::Triangle => OscType::Triangle,
        LfoWaveform::Square => OscType::Square,
        LfoWaveform::Sawtooth => OscType::Sawtooth,
    }
}

/// The four built-in table sets at one size (built on the control side; LFO type changes then
/// swap tables without allocating).
#[derive(Clone)]
pub struct Tables {
    sets: [Arc<WaveTables>; 4],
}

impl Tables {
    pub fn new(sample_rate: u32) -> Self {
        let size = periodic_wave_size(sample_rate);
        let get = |t| WaveTables::get(t, size);
        Self {
            sets: [
                get(OscType::Sine),
                get(OscType::Triangle),
                get(OscType::Square),
                get(OscType::Sawtooth),
            ],
        }
    }

    fn get(&self, t: OscType) -> &Arc<WaveTables> {
        match t {
            OscType::Sine => &self.sets[0],
            OscType::Triangle => &self.sets[1],
            OscType::Square => &self.sets[2],
            OscType::Sawtooth => &self.sets[3],
        }
    }
}

/// One running oscillator.
#[derive(Clone)]
pub struct Osc {
    tables: Tables,
    shape: OscType,
    negate: bool,
    sr: f64,
    phase: f64,
}

impl Osc {
    pub fn new(tables: Tables, shape: OscType, negate: bool, sample_rate: f64) -> Self {
        Self {
            tables,
            shape,
            negate,
            sr: sample_rate,
            phase: 0.0,
        }
    }

    /// `oscillator.type = t` (the phase carries on).
    pub fn set_type(&mut self, shape: OscType) {
        self.shape = shape;
    }

    /// Restart at phase 0 (a new `OscillatorNode`).
    pub fn restart(&mut self) {
        self.phase = 0.0;
    }

    /// One sample at `freq` Hz, then advance.
    #[inline]
    pub fn tick(&mut self, freq: f32) -> f32 {
        let wt = self.tables.get(self.shape);
        let size = wt.size();
        let mask = size - 1;
        let rate_scale = size as f32 / self.sr as f32;
        let incr = freq * rate_scale;
        // WaveDataForFundamentalFrequency
        let ranges = (3.0f32 * (size as f32).log2()).round() as usize;
        let nyquist = 0.5f32 * self.sr as f32;
        let lowest = nyquist / (size / 2) as f32;
        let f = (incr * (1.0 / rate_scale)).abs();
        let ratio = if f > 0.0 { f / lowest } else { 0.5 };
        let cents = ratio.log2() * 1200.0;
        let pitch_range = (1.0 + cents / 400.0).max(0.0).min((ranges - 1) as f32);
        let r1 = pitch_range as usize;
        let r2 = if r1 < ranges - 1 { r1 + 1 } else { r1 };
        let tf = pitch_range - r1 as f32;
        let (hi, lo) = (wt.table(r1), wt.table(r2));
        let v = self.phase;
        let i0 = v as usize;
        let a = incr.abs();
        let (s_hi, s_lo) = if a >= INTERPOLATE_2_POINT {
            let i1 = (i0 + 1) & mask;
            let i0 = i0 & mask;
            let t = v - i0 as f64;
            (
                (1.0 - t) * hi[i0] as f64 + t * hi[i1] as f64,
                (1.0 - t) * lo[i0] as f64 + t * lo[i1] as f64,
            )
        } else if a >= INTERPOLATE_3_POINT {
            let t = v - i0 as f64;
            let c = [0.5 * t * (t - 1.0), 1.0 - t * t, 0.5 * t * (t + 1.0)];
            let mut s = (0.0, 0.0);
            for (k, ck) in c.iter().enumerate() {
                let i = (i0 + size + k - 1) & mask;
                s.0 += ck * hi[i] as f64;
                s.1 += ck * lo[i] as f64;
            }
            s
        } else {
            let t = v - i0 as f64;
            let t2 = t * t;
            let c = [
                t * (t2 - 1.0) * (t - 2.0) / 24.0,
                -t * (t - 1.0) * (t2 - 4.0) / 6.0,
                (t2 - 1.0) * (t2 - 4.0) / 4.0,
                -t * (t + 1.0) * (t2 - 4.0) / 6.0,
                t * (t2 - 1.0) * (t + 2.0) / 24.0,
            ];
            let mut s = (0.0, 0.0);
            for (k, ck) in c.iter().enumerate() {
                let i = (i0 + size + k - 2) & mask;
                s.0 += ck * hi[i] as f64;
                s.1 += ck * lo[i] as f64;
            }
            s
        };
        let out = ((1.0 - tf as f64) * s_hi + tf as f64 * s_lo) as f32;
        let mut p = self.phase + incr as f64;
        p -= (p * (1.0 / size as f64)).floor() * size as f64;
        self.phase = p;
        if self.negate { -out } else { out }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slow_sine_is_a_sine_and_the_flip_negates_it() {
        let sr = 48000.0;
        let t = Tables::new(48000);
        let mut a = Osc::new(t.clone(), OscType::Sine, false, sr);
        let mut b = Osc::new(t, OscType::Sine, true, sr);
        for k in 0..48000 {
            let (x, y) = (a.tick(0.7), b.tick(0.7));
            let want = (2.0 * std::f64::consts::PI * 0.7 * k as f64 / sr).sin();
            assert!((x as f64 - want).abs() < 2e-5, "{k}: {x} {want}");
            assert_eq!(x, -y);
        }
    }
}
