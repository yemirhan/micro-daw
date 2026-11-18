//! `Tone.Filter` with rolloff −12 (one native `BiquadFilterNode`) for the presence (high-shelf)
//! and tone (lowpass) stages.
//!
//! Tone drives the biquad's frequency / Q / gain from `Signal`s (ConstantSourceNodes), so the
//! native params are audio-rate: Chromium computes the coefficients per frame from the f32 param
//! values (only when they change here: the result is the same). Coefficients follow Blink
//! `Biquad::SetLowpassParams` (Q = resonance in dB) and `SetHighShelfParams` (shelf slope 1, Q
//! unused), in f64 and normalised by a0; processing is direct form I in f64 with each output
//! rounded to f32 and fed back (denormals flushed), like `mdaw_dsp::factory::biquad`.

use super::param::Automated;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterKind {
    Lowpass,
    HighShelf,
}

#[derive(Debug, Clone, Copy, Default)]
struct State {
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

#[derive(Debug, Clone)]
pub struct ToneFilter {
    kind: FilterKind,
    nyquist: f64,
    pub frequency: Automated,
    /// Gain in dB (shelf only).
    pub gain: Automated,
    q: f32,
    coef: [f64; 5],
    last: Option<(f32, f32)>,
    state: [State; 2],
}

/// Blink normalised coefficients `[b0, b1, b2, a1, a2]` for frequency `f` (f32 Hz), Q / gain.
pub fn coefficients(kind: FilterKind, f: f32, q: f32, gain_db: f32, nyquist: f64) -> [f64; 5] {
    let cutoff = (f as f64 / nyquist).clamp(0.0, 1.0);
    let (b0, b1, b2, a0, a1, a2) = match kind {
        FilterKind::Lowpass => {
            if cutoff >= 1.0 {
                (1.0, 0.0, 0.0, 1.0, 0.0, 0.0)
            } else if cutoff > 0.0 {
                let resonance = q as f64;
                let g = 10f64.powf(-0.05 * resonance);
                let w0 = std::f64::consts::PI * cutoff;
                let cos_w0 = w0.cos();
                let alpha = 0.5 * w0.sin() * g;
                let b1 = 1.0 - cos_w0;
                let b0 = 0.5 * b1;
                (b0, b1, b0, 1.0 + alpha, -2.0 * cos_w0, 1.0 - alpha)
            } else {
                (0.0, 0.0, 0.0, 1.0, 0.0, 0.0)
            }
        }
        FilterKind::HighShelf => {
            let a = 10f64.powf(gain_db as f64 / 40.0);
            if cutoff >= 1.0 {
                (1.0, 0.0, 0.0, 1.0, 0.0, 0.0)
            } else if cutoff > 0.0 {
                let w0 = std::f64::consts::PI * cutoff;
                let s = 1.0;
                let alpha = 0.5 * w0.sin() * ((a + 1.0 / a) * (1.0 / s - 1.0) + 2.0).sqrt();
                let k = w0.cos();
                let k2 = 2.0 * a.sqrt() * alpha;
                let ap1 = a + 1.0;
                let am1 = a - 1.0;
                (
                    a * (ap1 + am1 * k + k2),
                    -2.0 * a * (am1 + ap1 * k),
                    a * (ap1 + am1 * k - k2),
                    ap1 - am1 * k + k2,
                    2.0 * (am1 - ap1 * k),
                    ap1 - am1 * k - k2,
                )
            } else {
                (a * a, 0.0, 0.0, 1.0, 0.0, 0.0)
            }
        }
    };
    [b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0]
}

impl ToneFilter {
    /// `new Tone.Filter({ type, frequency, Q, gain })` at context rate `sr`.
    pub fn new(kind: FilterKind, frequency: f64, q: f64, gain_db: f64, sr: u32) -> Self {
        Self {
            kind,
            nyquist: sr as f64 / 2.0,
            frequency: Automated::new(frequency),
            gain: Automated::new(gain_db),
            q: q as f32,
            coef: [0.0; 5],
            last: None,
            state: [State::default(); 2],
        }
    }

    /// Fresh node (state cleared) with constant params.
    pub fn reset(&mut self, frequency: f64, gain_db: f64) {
        self.frequency = Automated::new(frequency);
        self.gain = Automated::new(gain_db);
        self.last = None;
        self.state = [State::default(); 2];
    }

    /// Filter `l` / `r` in place; frame `i` is at context time `(frame + i) / sr`.
    pub fn process(&mut self, frame: u64, sr: f64, l: &mut [f32], r: &mut [f32]) {
        for i in 0..l.len() {
            let t = (frame + i as u64) as f64 / sr;
            let p = (self.frequency.at(t), self.gain.at(t));
            if self.last != Some(p) {
                self.coef = coefficients(self.kind, p.0, self.q, p.1, self.nyquist);
                self.last = Some(p);
            }
            l[i] = tick(&mut self.state[0], &self.coef, l[i]);
            r[i] = tick(&mut self.state[1], &self.coef, r[i]);
        }
    }
}

#[inline]
fn tick(s: &mut State, c: &[f64; 5], x: f32) -> f32 {
    let xd = x as f64;
    let y = (c[0] * xd + c[1] * s.x1 + c[2] * s.x2 - c[3] * s.y1 - c[4] * s.y2) as f32;
    s.x2 = s.x1;
    s.x1 = xd;
    s.y2 = s.y1;
    s.y1 = if y.is_subnormal() { 0.0 } else { y as f64 };
    y
}
