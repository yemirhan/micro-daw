//! `Tone.LFO` (type sine) as Tremolo and Chorus use it, sample by sample:
//!
//! ```text
//! Oscillator(sine, phase φ) → ×amplitude → AudioToGain WaveShaper((x+1)/2, 1024)
//!   → Scale: ×f32(max − min) → + f32(min)
//! ```
//!
//! - Phase: Tone builds a `PeriodicWave` with `real[1] = −sin φ, imag[1] = cos φ` for φ ≠ 0 (a
//!   plain native sine for φ = 0): `x = sin(θ − φ)`.
//! - Chromium's oscillator advances its table index by `incr = f32(f32(freq) · f32(N / sr))`
//!   (f64 index, wrapped to the table size N). The sample is computed here as `sin` of the exact
//!   index phase; Chromium interpolates its table (linear or Lagrange): ≤ ~3e-7 apart.
//! - Every node rounds to f32 (gain multiplies, the waveshaper, the Add summing junction).
//! - The LFO starts at its start frame with index 0.

use std::sync::LazyLock;

use mdaw_dsp::factory::osc::periodic_wave_size;
use mdaw_dsp::shaping::{tone_curve, waveshape};

/// `AudioToGain`: `WaveShaper(x ↦ (x + 1) / 2)`, 1024 points.
static A2G: LazyLock<Vec<f32>> = LazyLock::new(|| tone_curve(1024, |x| (x + 1.0) / 2.0));

#[derive(Debug, Clone)]
pub struct Lfo {
    size: f64,
    rate_scale: f32,
    phase: f64,
    index: f64,
    start_frame: u64,
    /// `Scale` multiplier `f32(max − min)` and addend `f32(min)`.
    mult: f32,
    add: f32,
}

impl Lfo {
    /// An LFO at context rate `sr` with `phase` in degrees, scaled to `min..max`, started at
    /// `start_frame`.
    pub fn new(sr: u32, phase_deg: f64, min: f64, max: f64, start_frame: u64) -> Self {
        LazyLock::force(&A2G);
        let size = periodic_wave_size(sr) as f64;
        let mut l = Self {
            size,
            rate_scale: (size / sr as f64) as f32,
            phase: phase_deg * std::f64::consts::PI / 180.0,
            index: 0.0,
            start_frame,
            mult: 0.0,
            add: 0.0,
        };
        l.set_range(min, max);
        l
    }

    /// `lfo.min = min; lfo.max = max` (Scale `_setRange`).
    pub fn set_range(&mut self, min: f64, max: f64) {
        self.mult = (max - min) as f32;
        self.add = min as f32;
    }

    /// Restart at `start_frame` (a new node).
    pub fn restart(&mut self, start_frame: u64) {
        self.index = 0.0;
        self.start_frame = start_frame;
    }

    /// The LFO output at context frame `frame` (call once per frame, in order) for an oscillator
    /// frequency `freq` and amplitude `amp`.
    #[inline]
    pub fn tick(&mut self, frame: u64, freq: f32, amp: f32) -> f32 {
        let osc = if frame < self.start_frame {
            0.0
        } else {
            let theta = 2.0 * std::f64::consts::PI * self.index / self.size;
            let s = (theta - self.phase).sin() as f32;
            let incr = freq * self.rate_scale;
            self.index += incr as f64;
            self.index -= (self.index / self.size).floor() * self.size;
            s
        };
        let a = osc * amp;
        let w = waveshape(&A2G, a);
        w * self.mult + self.add
    }
}
