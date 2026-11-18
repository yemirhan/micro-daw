//! `Tone.Filter`: 1 / 2 / 4 / 8 identical Chromium `BiquadFilterNode`s in series (rolloff −12 /
//! −24 / −48 / −96), their frequency / detune / Q fed by Tone signals, so Chromium recomputes the
//! coefficients for EVERY frame (`BiquadDSPKernel::UpdateCoefficients`, f64):
//! `f = frequency[k] / nyquist · 2^(detune[k]/1200)` (when detune ≠ 0), then `Biquad::Set*Params`
//! (lowpass / highpass: Q in dB; bandpass / notch: linear Q), normalised by `a0`. Processing is
//! direct form I in f64 with the output rounded to f32 and fed back as such
//! (`Biquad::Process`, sample-accurate path); denormals are flushed at the end of a quantum.
//!
//! The stages share their coefficients (same inputs); each keeps its own state, per channel.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FilterType {
    Lowpass,
    Highpass,
    Bandpass,
    Notch,
}

#[derive(Debug, Clone, Copy, Default)]
struct Coefs {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

fn normalized(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Coefs {
    let s = 1.0 / a0;
    Coefs {
        b0: b0 * s,
        b1: b1 * s,
        b2: b2 * s,
        a1: a1 * s,
        a2: a2 * s,
    }
}

fn coefficients(kind: FilterType, f: f64, q: f64) -> Coefs {
    let f = f.clamp(0.0, 1.0);
    match kind {
        FilterType::Lowpass => {
            if f == 1.0 {
                normalized(1.0, 0.0, 0.0, 1.0, 0.0, 0.0)
            } else if f > 0.0 {
                let g = 10f64.powf(q / 20.0);
                let theta = std::f64::consts::PI * f;
                let alpha = theta.sin() / (2.0 * g);
                let cosw = theta.cos();
                let beta = (1.0 - cosw) / 2.0;
                normalized(
                    beta,
                    2.0 * beta,
                    beta,
                    1.0 + alpha,
                    -2.0 * cosw,
                    1.0 - alpha,
                )
            } else {
                normalized(0.0, 0.0, 0.0, 1.0, 0.0, 0.0)
            }
        }
        FilterType::Highpass => {
            if f == 1.0 {
                normalized(0.0, 0.0, 0.0, 1.0, 0.0, 0.0)
            } else if f > 0.0 {
                let g = 10f64.powf(q / 20.0);
                let theta = std::f64::consts::PI * f;
                let alpha = theta.sin() / (2.0 * g);
                let cosw = theta.cos();
                let beta = (1.0 + cosw) / 2.0;
                normalized(
                    beta,
                    -2.0 * beta,
                    beta,
                    1.0 + alpha,
                    -2.0 * cosw,
                    1.0 - alpha,
                )
            } else {
                normalized(1.0, 0.0, 0.0, 1.0, 0.0, 0.0)
            }
        }
        FilterType::Bandpass => {
            let q = q.max(0.0);
            if f > 0.0 && f < 1.0 {
                if q > 0.0 {
                    let w0 = std::f64::consts::PI * f;
                    let alpha = w0.sin() / (2.0 * q);
                    let k = w0.cos();
                    normalized(alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * k, 1.0 - alpha)
                } else {
                    normalized(1.0, 0.0, 0.0, 1.0, 0.0, 0.0)
                }
            } else {
                normalized(0.0, 0.0, 0.0, 1.0, 0.0, 0.0)
            }
        }
        FilterType::Notch => {
            let q = q.max(0.0);
            if f > 0.0 && f < 1.0 {
                if q > 0.0 {
                    let w0 = std::f64::consts::PI * f;
                    let alpha = w0.sin() / (2.0 * q);
                    let k = w0.cos();
                    normalized(1.0, -2.0 * k, 1.0, 1.0 + alpha, -2.0 * k, 1.0 - alpha)
                } else {
                    normalized(0.0, 0.0, 0.0, 1.0, 0.0, 0.0)
                }
            } else {
                normalized(1.0, 0.0, 0.0, 1.0, 0.0, 0.0)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct State {
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

const MAX_STAGES: usize = 8;

/// Chromium `kMaxTailAmplitude` (−90 dB) and the tail time cap.
const MAX_TAIL_AMPLITUDE: f64 = 1.0 / 32768.0;
const MAX_TAIL_TIME: f64 = 30.0;

/// `BiquadDSPKernel::UpdateTailTime` / `Biquad::TailFrame`: frames until the impulse response
/// stays below −90 dB, from the poles of `z² + a1·z + a2`.
fn tail_time(c: &Coefs, sr: f64) -> f64 {
    let (b0, b1, b2, a1, a2) = (c.b0, c.b1, c.b2, c.a1, c.a2);
    let max_frame = MAX_TAIL_TIME * sr;
    let discrim = a1 * a1 - 4.0 * a2;
    let tail_frame = if discrim > 0.0 {
        let r1 = if a1 >= 0.0 {
            (-a1 - discrim.sqrt()) / 2.0
        } else {
            (-a1 + discrim.sqrt()) / 2.0
        };
        let r2 = a2 / r1;
        let c1 = (b0 * r1 * r1 + b1 * r1 + b2) / (r1 - r2);
        let c2 = (b0 * r2 * r2 + b1 * r2 + b2) / (r1 - r2);
        1.0 + (MAX_TAIL_AMPLITUDE / (c1.abs() + c2.abs())).ln() / r1.abs().ln()
    } else if discrim < 0.0 {
        let x = -a1 / 2.0;
        let y = (-discrim).sqrt() / 2.0;
        let r = x.hypot(y);
        if r == 1.0 {
            max_frame
        } else {
            // |b0·r1² + b1·r1 + b2| / |r1 − r2| for r1 = x + iy, r2 = conj(r1) (same magnitude).
            let (re, im) = (
                b0 * (x * x - y * y) + b1 * x + b2,
                b0 * 2.0 * x * y + b1 * y,
            );
            let c1 = re.hypot(im) / (2.0 * y);
            1.0 + (MAX_TAIL_AMPLITUDE / (2.0 * c1)).ln() / r.ln()
        }
    } else {
        let r = -a1 / 2.0;
        if r == 0.0 {
            2.0
        } else {
            let c1 = (b0 * r * r + b1 * r + b2) / (r * r);
            1.0 + (MAX_TAIL_AMPLITUDE / c1.abs()).ln() / r.abs().ln()
        }
    };
    let tail = tail_frame / sr;
    if tail.is_nan() {
        0.0
    } else {
        tail.clamp(0.0, MAX_TAIL_TIME)
    }
}

/// Number of cascaded biquads for a Tone rolloff.
pub fn stages_for_rolloff(rolloff: f64) -> usize {
    match rolloff as i64 {
        -24 => 2,
        -48 => 4,
        -96 => 8,
        _ => 1,
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ToneFilter {
    pub kind: FilterType,
    stages: usize,
    nyquist: f64,
    sr: f64,
    /// Per stage: end time of the last quantum with non-silent input, tail time (s), and whether
    /// the stage output silence (did not process) in the last quantum.
    last_non_silent: [f64; MAX_STAGES],
    tail: [f64; MAX_STAGES],
    frozen: [bool; MAX_STAGES],
    /// [stage][channel]
    state: [[State; 2]; MAX_STAGES],
    coefs: [Coefs; 128],
    last_key: (u32, u32, u32),
    last: Coefs,
}

impl ToneFilter {
    pub fn new(kind: FilterType, rolloff: f64, sr: f64) -> Self {
        Self {
            kind,
            stages: stages_for_rolloff(rolloff),
            nyquist: 0.5 * sr,
            sr,
            last_non_silent: [0.0; MAX_STAGES],
            tail: [0.0; MAX_STAGES],
            frozen: [true; MAX_STAGES],
            state: [[State::default(); 2]; MAX_STAGES],
            coefs: [Coefs::default(); 128],
            last_key: (u32::MAX, u32::MAX, u32::MAX),
            last: Coefs::default(),
        }
    }

    /// `filter.type = …` (Chromium keeps the state).
    pub fn set_kind(&mut self, kind: FilterType) {
        self.kind = kind;
        self.last_key = (u32::MAX, u32::MAX, u32::MAX);
    }

    /// `filter.rolloff = …`: Tone builds new biquads (fresh state).
    pub fn set_rolloff(&mut self, rolloff: f64) {
        self.stages = stages_for_rolloff(rolloff);
        self.state = [[State::default(); 2]; MAX_STAGES];
    }

    /// Chromium re-creates the kernels when the input's channel count changes (fresh state).
    pub fn reset(&mut self) {
        self.state = [[State::default(); 2]; MAX_STAGES];
    }

    /// Every stage stopped processing (silent input past its tail): the output is silent.
    pub fn is_frozen(&self) -> bool {
        self.frozen[..self.stages].iter().all(|&f| f)
    }

    /// Filter `chans` channels of `data` in place with per-frame `freq` / `detune` / `q`.
    ///
    /// Chromium's silence propagation: a stage whose input is silent (the bus silence hint) and
    /// whose last non-silent input ended more than its tail time ago does not process (its
    /// state stays as it was) and outputs silence; otherwise it processes (also zeros, ringing
    /// out) and its tail time is updated from the quantum's last coefficients
    /// (`Biquad::TailFrame`, −90 dB, at most 30 s). Returns whether the output is silent.
    #[allow(clippy::too_many_arguments)]
    pub fn process(
        &mut self,
        frame: u64,
        input_silent: bool,
        freq: &[f32],
        detune: &[f32],
        q: &[f32],
        data: [&mut [f32]; 2],
        chans: usize,
    ) -> bool {
        let n = data[0].len();
        let now = frame as f64 / self.sr;
        let end = (frame + n as u64) as f64 / self.sr;
        let [d0, d1] = data;
        let mut silent = input_silent;
        let mut coefs_ready = false;
        for s in 0..self.stages {
            if silent && self.last_non_silent[s] + self.tail[s] < now {
                self.frozen[s] = true;
                d0.fill(0.0);
                d1.fill(0.0);
                continue;
            }
            if !coefs_ready {
                self.update_coefficients(freq, detune, q, n);
                coefs_ready = true;
            }
            for (c, buf) in [&mut *d0, &mut *d1].into_iter().enumerate().take(chans) {
                self.run(s, c, buf);
            }
            self.tail[s] = tail_time(&self.coefs[n - 1], self.sr);
            if !silent {
                self.last_non_silent[s] = end;
            }
            self.frozen[s] = false;
            silent = false;
        }
        silent
    }

    fn update_coefficients(&mut self, freq: &[f32], detune: &[f32], q: &[f32], n: usize) {
        for k in 0..n {
            let key = (freq[k].to_bits(), detune[k].to_bits(), q[k].to_bits());
            if key != self.last_key {
                self.last_key = key;
                let mut f = freq[k] as f64 / self.nyquist;
                if detune[k] != 0.0 {
                    f *= ((detune[k] / 1200.0) as f64).exp2();
                }
                self.last = coefficients(self.kind, f, q[k] as f64);
            }
            self.coefs[k] = self.last;
        }
    }

    fn run(&mut self, s: usize, c: usize, buf: &mut [f32]) {
        let st = &mut self.state[s][c];
        let (mut x1, mut x2, mut y1, mut y2) = (st.x1, st.x2, st.y1, st.y2);
        for (k, v) in buf.iter_mut().enumerate() {
            let cf = &self.coefs[k];
            let x = *v as f64;
            let y = (cf.b0 * x + cf.b1 * x1 + cf.b2 * x2 - cf.a1 * y1 - cf.a2 * y2) as f32;
            *v = y;
            x2 = x1;
            x1 = x;
            y2 = y1;
            y1 = y as f64;
        }
        let flush = |v: f64| if (v as f32).is_subnormal() { 0.0 } else { v };
        *st = State {
            x1: flush(x1),
            x2: flush(x2),
            y1: flush(y1),
            y2: flush(y2),
        };
    }
}
