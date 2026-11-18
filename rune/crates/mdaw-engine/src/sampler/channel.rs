//! A pad's channel strip, as SamplerKit builds it with Tone and Chromium renders it:
//! `amp (Tone.Gain, gainDb) → [Tone.Filter lowpass −12 dB/oct, Q 0.707] → Tone.Panner → output`.
//!
//! - **amp**: `GainNode`, a-rate gain: `out = in·g` in f32.
//! - **filter**: `Tone.Filter({ type: 'lowpass', rolloff: -12, Q: 0.707 })` is one
//!   `BiquadFilterNode` whose frequency and Q come from Tone `Signal`s (ConstantSourceNodes
//!   connected to the params). Because the params have input connections, Chromium recomputes the
//!   coefficients every quantum: once when the frequency is constant over the quantum (then the
//!   macOS `vDSP_deq22D` path runs: f64 feedback of the unrounded output), per frame otherwise
//!   (f32-rounded output fed back, denormals flushed at the quantum end). Lowpass `Q` is a
//!   resonance in dB (`Biquad::SetLowpassParams`), coefficients normalised by `1/a0`.
//!   The filter is created the first time the cutoff drops below 20 kHz and is never removed:
//!   bypassing only reconnects the amp straight to the panner, the filter stays connected to the
//!   panner and rings out its state.
//! - **panner**: `Tone.Panner` = `StereoPannerNode` with `channelCount 1, explicit`: the input is
//!   mixed down to mono (`0.5·L + 0.5·R` per connection), then equal-power panned with f64 gains
//!   (`cos`/`sin` of `(pan·0.5 + 0.5)·π/2`), rounded to f32 per sample. The captures show
//!   Chromium 153 uses f64 gains for a constant pan too (Blink's current `PanToTargetValue` rounds
//!   them to f32 first; that would be off by an ulp).

#![allow(clippy::needless_range_loop)] // per-frame DSP over parallel buffers

use super::param::{AutoParam, Clock};
use std::f64::consts::FRAC_PI_2;

const Q: usize = crate::contract::RENDER_QUANTUM;
/// `Tone.Filter` Q signal value (0.707) as the biquad sees it.
const FILTER_Q: f32 = 0.707;

/// Lowpass coefficients `[b0, b1, b2, a1, a2]` (Chromium `SetLowpassParams` + `NormalizeFrequency`).
fn lowpass(freq: f32, nyquist: f64) -> [f64; 5] {
    let cutoff = (freq as f64 / nyquist).clamp(0.0, 1.0);
    if cutoff == 1.0 {
        return [1.0, 0.0, 0.0, 0.0, 0.0];
    }
    if cutoff <= 0.0 {
        return [0.0; 5];
    }
    let resonance = 10f64.powf(FILTER_Q as f64 / 20.0);
    let theta = std::f64::consts::PI * cutoff;
    let alpha = theta.sin() / (2.0 * resonance);
    let cosw = theta.cos();
    let beta = (1.0 - cosw) / 2.0;
    let (b0, b1, b2) = (beta, 2.0 * beta, beta);
    let (a0, a1, a2) = (1.0 + alpha, -2.0 * cosw, 1.0 - alpha);
    let inv = 1.0 / a0;
    [b0 * inv, b1 * inv, b2 * inv, a1 * inv, a2 * inv]
}

#[derive(Debug, Clone, Copy, Default)]
struct BiquadState {
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl BiquadState {
    fn is_zero(&self) -> bool {
        self.x1 == 0.0 && self.x2 == 0.0 && self.y1 == 0.0 && self.y2 == 0.0
    }

    /// Constant coefficients (`ProcessFast`, vDSP double precision).
    fn run_const(&mut self, c: &[f64; 5], data: &mut [f32]) {
        let [b0, b1, b2, a1, a2] = *c;
        let (mut x1, mut x2, mut y1, mut y2) = (self.x1, self.x2, self.y1, self.y2);
        for s in data.iter_mut() {
            let x = *s as f64;
            let y = b0 * x + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
            *s = y as f32;
            x2 = x1;
            x1 = x;
            y2 = y1;
            y1 = y;
        }
        *self = Self { x1, x2, y1, y2 };
    }

    /// Per-frame coefficients (Chromium's sample-accurate loop).
    fn run_varying(&mut self, c: &[[f64; 5]], data: &mut [f32]) {
        let (mut x1, mut x2, mut y1, mut y2) = (self.x1, self.x2, self.y1, self.y2);
        for (s, k) in data.iter_mut().zip(c) {
            let x = *s;
            let y = (k[0] * x as f64 + k[1] * x1 + k[2] * x2 - k[3] * y1 - k[4] * y2) as f32;
            *s = y;
            x2 = x1;
            x1 = x as f64;
            y2 = y1;
            y1 = y as f64;
        }
        let flush = |v: f64| if (v as f32).is_subnormal() { 0.0 } else { v };
        *self = Self {
            x1: flush(x1),
            x2: flush(x2),
            y1: flush(y1),
            y2: flush(y2),
        };
    }
}

#[derive(Debug, Clone)]
struct Filter {
    /// The `Tone.Filter` frequency Signal (ConstantSource offset, native default 1).
    freq: AutoParam,
    state: [BiquadState; 2],
}

/// One pad's channel. Created for all 16 pads when the kit is built; no allocation afterwards.
#[derive(Debug, Clone)]
pub(crate) struct Channel {
    amp: AutoParam,
    pan: AutoParam,
    filter: Option<Filter>,
    /// The amp feeds the filter (cutoff < 20 kHz) instead of the panner.
    filtered: bool,
    /// A multi-channel voice has fed the pad (the bus is stereo).
    pub stereo: bool,
}

/// Per-quantum scratch shared by all channels.
#[derive(Debug, Clone)]
pub(crate) struct Scratch {
    vals: [f32; Q],
    coefs: [[f64; 5]; Q],
    direct: [[f32; Q]; 2],
    filt: [[f32; Q]; 2],
}

impl Default for Scratch {
    fn default() -> Self {
        Self {
            vals: [0.0; Q],
            coefs: [[0.0; 5]; Q],
            direct: [[0.0; Q]; 2],
            filt: [[0.0; Q]; 2],
        }
    }
}

impl Channel {
    /// `new Tone.Gain(1)` + `new Tone.Panner(0)` (no events: the values equal the defaults).
    pub fn new(cx: Clock) -> Self {
        Self {
            amp: AutoParam::new(1.0, 1.0, f32::MIN, f32::MAX, cx),
            pan: AutoParam::new(0.0, 0.0, -1.0, 1.0, cx),
            filter: None,
            filtered: false,
            stereo: false,
        }
    }

    /// `SamplerKit.applyChannel`: gain (dB→lin, already clamped), pan, cutoff (clamped to
    /// 20..20000). `immediate` at build time, 20 ms ramps otherwise.
    pub fn apply(&mut self, gain: f64, pan: f64, cutoff: f64, immediate: bool, cx: Clock) {
        const PARAM_RAMP: f64 = 0.02;
        if immediate {
            self.amp.set_value(gain, cx);
            self.pan.set_value(pan, cx);
        } else {
            self.amp.linear_ramp_to(gain, PARAM_RAMP, cx);
            self.pan.linear_ramp_to(pan, PARAM_RAMP, cx);
        }
        let want = cutoff < super::FILTER_BYPASS_HZ;
        if want && self.filter.is_none() {
            // `new Tone.Filter({ frequency: cutoff })`: its Signal schedules set(cutoff, 0).
            self.filter = Some(Filter {
                freq: AutoParam::new(1.0, cutoff, f32::MIN, f32::MAX, cx),
                state: [BiquadState::default(); 2],
            });
        }
        if want && let Some(f) = &mut self.filter {
            if self.filtered && !immediate {
                f.freq.exponential_ramp_to(cutoff, PARAM_RAMP, cx);
            } else {
                f.freq.set_value(cutoff, cx);
            }
        }
        self.filtered = want;
    }

    /// The amp currently feeds the low-pass (cutoff below 20 kHz).
    #[cfg(test)]
    pub fn is_filtered(&self) -> bool {
        self.filtered
    }

    /// The low-pass exists (it is created once and kept).
    #[cfg(test)]
    pub fn has_filter(&self) -> bool {
        self.filter.is_some()
    }

    /// Bound the Tone-mirror timelines (call between quanta).
    pub fn prune(&mut self, now: f64) {
        self.amp.prune_tone(now);
        self.pan.prune_tone(now);
        if let Some(f) = &mut self.filter {
            f.freq.prune_tone(now);
        }
    }

    /// Render one quantum: `bus` holds the summed voices (L, R; R == L for a mono bus) and is
    /// clobbered. The panned result is ADDED to `out_l` / `out_r`. `silent` = no voice fed the
    /// bus this quantum.
    #[allow(clippy::too_many_arguments)]
    pub fn process(
        &mut self,
        bus: &mut [[f32; Q]; 2],
        silent: bool,
        frame: u64,
        sr: f64,
        n: usize,
        sc: &mut Scratch,
        out_l: &mut [f32],
        out_r: &mut [f32],
    ) {
        let nch = if self.stereo { 2 } else { 1 };
        // amp
        self.amp.render(frame, sr, &mut sc.vals[..n]);
        if !silent {
            for ch in bus.iter_mut().take(nch) {
                for (x, g) in ch[..n].iter_mut().zip(&sc.vals[..n]) {
                    *x *= *g;
                }
            }
        }
        let filter_live = self.filter.as_ref().is_some_and(|f| {
            self.filtered && !silent || f.state.iter().any(|s| !s.is_zero()) || !f.freq.is_idle()
        });
        // routing: amp → filter or amp → panner
        let amp_direct = !self.filtered && !silent;
        for c in 0..nch {
            if amp_direct {
                sc.direct[c][..n].copy_from_slice(&bus[c][..n]);
            }
        }
        if let Some(f) = &mut self.filter {
            f.freq.render(frame, sr, &mut sc.vals[..n]);
            if filter_live {
                for c in 0..nch {
                    if self.filtered && !silent {
                        sc.filt[c][..n].copy_from_slice(&bus[c][..n]);
                    } else {
                        sc.filt[c][..n].fill(0.0);
                    }
                }
                let nyquist = sr * 0.5;
                let v = &sc.vals[..n];
                if v.iter().all(|&x| x == v[0]) {
                    let c = lowpass(v[0], nyquist);
                    for ch in 0..nch {
                        f.state[ch].run_const(&c, &mut sc.filt[ch][..n]);
                    }
                } else {
                    for (k, &x) in sc.coefs[..n].iter_mut().zip(v) {
                        *k = lowpass(x, nyquist);
                    }
                    for ch in 0..nch {
                        f.state[ch].run_varying(&sc.coefs[..n], &mut sc.filt[ch][..n]);
                    }
                }
            }
        }
        // panner input: down-mix each connection (0.5·L + 0.5·R) and sum them.
        let m = &mut bus[0];
        m[..n].fill(0.0);
        let mut any = false;
        let mut sum_in = |src: &[[f32; Q]; 2]| {
            any = true;
            if nch == 2 {
                for i in 0..n {
                    m[i] += src[0][i] * 0.5;
                    m[i] += src[1][i] * 0.5;
                }
            } else {
                for i in 0..n {
                    m[i] += src[0][i];
                }
            }
        };
        if amp_direct {
            sum_in(&sc.direct);
        }
        if filter_live {
            sum_in(&sc.filt);
        }
        // pan (f64 gains per frame: Chromium M153's PanToTargetValue computes them in double
        // too, so both of its paths give the same samples)
        self.pan.render(frame, sr, &mut sc.vals[..n]);
        if !any {
            return;
        }
        for i in 0..n {
            let pan = (sc.vals[i] as f64).clamp(-1.0, 1.0);
            let rad = (pan * 0.5 + 0.5) * FRAC_PI_2;
            let x = m[i] as f64;
            out_l[i] += (x * rad.cos()) as f32;
            out_r[i] += (x * rad.sin()) as f32;
        }
    }
}
