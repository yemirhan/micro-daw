//! Tone effects built on [`CrossFade`]: `Distortion`, `Chorus`, `FeedbackDelay`, `PingPongDelay`,
//! and `RoomReverb` (src/audio/RoomReverb.ts). Each processes a stereo quantum in place.
//!
//! Feedback cycles (FeedbackDelay, PingPongDelay) read the previous quantum of the cycle's
//! output, as Chromium does (see `fx/delay.rs`). The chorus feedback gains are 0 (Tone's default
//! `feedback: 0`) and contribute nothing, so that cycle is left out.

use mdaw_dsp::factory::osc::OscType;
use mdaw_dsp::reverb_ir::{
    ReverbOptions, SEND_LOW_CUT_HZ, convolver_normalization_scale, create_impulse_response,
};
use mdaw_model::types::FilterType;

use super::Q;
use super::biquad::{Coefs, State};
use super::convolver::Convolver;
use super::delay::DelayLine;
use super::lfo::Lfo;
use super::osc::Tables;
use super::tone::{ToneParam, Units};
use super::waveshaper::{CURVE_LEN, Oversampled2x, distortion_curve};
use super::xfade::CrossFade;
use crate::sampler::param::Clock;

/// `Tone.Distortion({ distortion, wet, oversample: '2x' })`.
#[derive(Clone)]
pub struct Distortion {
    pub xfade: CrossFade,
    amount: f64,
    curve: [f32; CURVE_LEN],
    shaper: [Oversampled2x; 2],
    sr: f64,
}

impl Distortion {
    pub fn new(amount: f64, wet: f64, cx: Clock) -> Self {
        let mut curve = [0f32; CURVE_LEN];
        distortion_curve(amount, &mut curve);
        Self {
            xfade: CrossFade::new(wet, cx),
            amount,
            curve,
            shaper: [Oversampled2x::new(), Oversampled2x::new()],
            sr: cx.sr,
        }
    }

    pub fn amount(&self) -> f64 {
        self.amount
    }

    /// The WaveShaper's oversampler state is re-initialized (see [`super::live`]).
    pub fn reset_state(&mut self) {
        for s in &mut self.shaper {
            s.reset();
        }
    }

    /// `distortion.distortion = amount` (a new curve).
    pub fn set_amount(&mut self, amount: f64) {
        self.amount = amount;
        distortion_curve(amount, &mut self.curve);
    }

    pub fn process(&mut self, frame: u64, l: &mut [f32; Q], r: &mut [f32; Q]) {
        self.xfade.update(frame, self.sr);
        for (ch, io) in [l, r].into_iter().enumerate() {
            let mut wet = *io;
            self.shaper[ch].process(&self.curve, &mut wet);
            let dry = *io;
            self.xfade.mix(&dry, &wet, io);
        }
    }
}

/// `Tone.Chorus({ frequency, delayTime (ms), spread: 180, depth, wet }).start()`.
#[derive(Clone)]
pub struct Chorus {
    pub xfade: CrossFade,
    depth: f64,
    delay_time: f64,
    lfo: [Lfo; 2],
    delay: [DelayLine; 2],
    d: [f32; Q],
    sr: f64,
}

impl Chorus {
    pub fn new(
        tables: &Tables,
        frequency: f64,
        delay_ms: f64,
        depth: f64,
        wet: f64,
        cx: Clock,
    ) -> Self {
        let delay_time = delay_ms / 1000.0;
        let (min, max) = Self::range(delay_time, depth);
        Self {
            xfade: CrossFade::new(wet, cx),
            depth,
            delay_time,
            lfo: [
                Lfo::new(
                    tables.clone(),
                    OscType::Sine,
                    false,
                    frequency,
                    min,
                    max,
                    cx,
                ),
                Lfo::new(tables.clone(), OscType::Sine, true, frequency, min, max, cx),
            ],
            delay: [DelayLine::new(1.0, cx.sr), DelayLine::new(1.0, cx.sr)],
            d: [0.0; Q],
            sr: cx.sr,
        }
    }

    fn range(delay_time: f64, depth: f64) -> (f64, f64) {
        let dev = delay_time * depth;
        ((delay_time - dev).max(0.0), delay_time + dev)
    }

    pub fn depth(&self) -> f64 {
        self.depth
    }

    /// `chorus.depth = d`: both LFOs' min then max (`.value =` at now).
    pub fn set_depth(&mut self, depth: f64, cx: Clock) {
        self.depth = depth;
        let (min, max) = Self::range(self.delay_time, depth);
        for l in &mut self.lfo {
            l.set_min(min, cx);
            l.set_max(max, cx);
        }
    }

    pub fn process(&mut self, frame: u64, l: &mut [f32; Q], r: &mut [f32; Q]) {
        self.process_with(frame, l, r, false);
    }

    /// `r_silent`: the effect path's right input is silent for this quantum (the first one after
    /// the effect is wired back in, see [`super::live`]); the dry path is not affected.
    pub fn process_with(&mut self, frame: u64, l: &mut [f32; Q], r: &mut [f32; Q], r_silent: bool) {
        self.xfade.update(frame, self.sr);
        for (ch, io) in [l, r].into_iter().enumerate() {
            // The right LFO's frequency is the left one's signal (identical values).
            self.lfo[ch].render(frame, self.sr, &mut self.d);
            let mut wet = if ch == 1 && r_silent { [0.0; Q] } else { *io };
            // the LFO is connected to delayTime: always a-rate
            self.delay[ch].process(&mut wet, &self.d, self.sr, true);
            let dry = *io;
            self.xfade.mix(&dry, &wet, io);
        }
    }
}

/// `Tone.FeedbackDelay({ delayTime, feedback, wet })` (maxDelay 1 s).
#[derive(Clone)]
pub struct FeedbackDelay {
    pub xfade: CrossFade,
    pub delay_time: ToneParam,
    pub feedback: ToneParam,
    delay: [DelayLine; 2],
    ret: [[f32; Q]; 2],
    d: [f32; Q],
    fb: [f32; Q],
    sr: f64,
}

impl FeedbackDelay {
    pub fn new(delay_time: f64, feedback: f64, wet: f64, cx: Clock) -> Self {
        Self {
            xfade: CrossFade::new(wet, cx),
            delay_time: ToneParam::new(0.0, delay_time, Units::Linear, (0.0, 1.0), cx),
            feedback: ToneParam::gain(feedback, Units::Linear, cx),
            delay: [DelayLine::new(1.0, cx.sr), DelayLine::new(1.0, cx.sr)],
            ret: [[0.0; Q]; 2],
            d: [0.0; Q],
            fb: [0.0; Q],
            sr: cx.sr,
        }
    }

    pub fn process(&mut self, frame: u64, l: &mut [f32; Q], r: &mut [f32; Q]) {
        self.xfade.update(frame, self.sr);
        let a_rate = self.delay_time.render(frame, self.sr, &mut self.d);
        self.feedback.render(frame, self.sr, &mut self.fb);
        for (ch, io) in [l, r].into_iter().enumerate() {
            let mut send = [0f32; Q];
            for k in 0..Q {
                send[k] = io[k] + self.ret[ch][k] * self.fb[k];
            }
            self.delay[ch].process(&mut send, &self.d, self.sr, a_rate);
            self.ret[ch] = send;
            let dry = *io;
            self.xfade.mix(&dry, &send, io);
        }
    }
}

/// `Tone.PingPongDelay({ delayTime, feedback, wet })` (maxDelay 1 s).
#[derive(Clone)]
pub struct PingPongDelay {
    pub xfade: CrossFade,
    pub delay_time: ToneParam,
    pub feedback: ToneParam,
    left: DelayLine,
    right_pre: DelayLine,
    right: DelayLine,
    /// The merge's previous quantum (L, R).
    prev: [[f32; Q]; 2],
    d: [f32; Q],
    fb: [f32; Q],
    sr: f64,
}

impl PingPongDelay {
    pub fn new(delay_time: f64, feedback: f64, wet: f64, cx: Clock) -> Self {
        Self {
            xfade: CrossFade::new(wet, cx),
            delay_time: ToneParam::signal(delay_time, Units::Linear, cx),
            feedback: ToneParam::signal(feedback, Units::Linear, cx),
            left: DelayLine::new(1.0, cx.sr),
            right_pre: DelayLine::new(1.0, cx.sr),
            right: DelayLine::new(1.0, cx.sr),
            prev: [[0.0; Q]; 2],
            d: [0.0; Q],
            fb: [0.0; Q],
            sr: cx.sr,
        }
    }

    pub fn process(&mut self, frame: u64, l: &mut [f32; Q], r: &mut [f32; Q]) {
        self.process_with(frame, l, r, false);
    }

    /// See [`Chorus::process_with`].
    pub fn process_with(&mut self, frame: u64, l: &mut [f32; Q], r: &mut [f32; Q], r_silent: bool) {
        self.xfade.update(frame, self.sr);
        self.delay_time.render(frame, self.sr, &mut self.d);
        self.feedback.render(frame, self.sr, &mut self.fb);
        // (the delayTime Signal is connected to the three delays: a-rate)
        // left: delay(inL + fb·R_prev)
        let mut wl = [0f32; Q];
        for k in 0..Q {
            wl[k] = l[k] + self.prev[1][k] * self.fb[k];
        }
        self.left.process(&mut wl, &self.d, self.sr, true);
        // right: delay(delay(inR) + fb·L_prev)
        let mut wr = if r_silent { [0.0; Q] } else { *r };
        self.right_pre.process(&mut wr, &self.d, self.sr, true);
        for ((w, p), fb) in wr.iter_mut().zip(&self.prev[0]).zip(&self.fb) {
            *w += p * fb;
        }
        self.right.process(&mut wr, &self.d, self.sr, true);
        self.prev = [wl, wr];
        let (dl, dr) = (*l, *r);
        self.xfade.mix(&dl, &wl, l);
        self.xfade.mix(&dr, &wr, r);
    }
}

/// `RoomReverb` (src/audio/RoomReverb.ts): `in → xfade.a`, `in → HP 200 Hz → Convolver(IR,
/// normalize) → xfade.b`.
pub struct RoomReverb {
    pub xfade: CrossFade,
    hp: Coefs,
    hp_st: [State; 2],
    conv: Box<Convolver>,
    sr: f64,
}

impl RoomReverb {
    /// Builds the IR and the convolver (control side).
    pub fn new(opts: &ReverbOptions, cx: Clock) -> Self {
        let ir = create_impulse_response(cx.sr as u32, opts);
        let scale = convolver_normalization_scale(&ir) as f32;
        Self {
            xfade: CrossFade::new(opts.wet.clamp(0.0, 1.0), cx),
            hp: Coefs::compute(FilterType::Highpass, SEND_LOW_CUT_HZ as f32, 1.0, cx.sr),
            hp_st: [State::default(); 2],
            conv: Box::new(Convolver::new(&ir, scale)),
            sr: cx.sr,
        }
    }

    /// The send's highpass (a BiquadFilterNode) is re-initialized (see [`super::live`]).
    pub fn reset_state(&mut self) {
        self.hp_st = [State::default(); 2];
    }

    pub fn process(&mut self, frame: u64, l: &mut [f32; Q], r: &mut [f32; Q]) {
        self.xfade.update(frame, self.sr);
        let mut sl = *l;
        let mut sr_ = *r;
        for k in 0..Q {
            sl[k] = self.hp_st[0].tick(&self.hp, sl[k]);
            sr_[k] = self.hp_st[1].tick(&self.hp, sr_[k]);
        }
        self.hp_st[0].flush();
        self.hp_st[1].flush();
        let mut wl = [0f32; Q];
        let mut wr = [0f32; Q];
        self.conv.process(&sl, &sr_, &mut wl, &mut wr);
        let (dl, dr) = (*l, *r);
        self.xfade.mix(&dl, &wl, l);
        self.xfade.mix(&dr, &wr, r);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Finding 1, measured on a raw Chromium `DelayNode` cycle (480 frames, feedback 0.5): echoes
    /// at 480, 1088, 1696, 2304, ... (each pass through the cycle adds a render quantum).
    #[test]
    fn a_feedback_cycle_repeats_every_delay_plus_one_quantum() {
        let sr = 48_000.0;
        let cx = Clock { now: 0.0, sr };
        let mut d = FeedbackDelay::new(480.0 / sr, 0.5, 1.0, cx);
        let mut out = Vec::new();
        for q in 0..24 {
            let (mut l, mut r) = ([0f32; Q], [0f32; Q]);
            if q == 0 {
                l[0] = 1.0;
                r[0] = 1.0;
            }
            d.process((q * Q) as u64, &mut l, &mut r);
            out.extend_from_slice(&l);
        }
        let peaks: Vec<(usize, f32)> = out
            .iter()
            .enumerate()
            .filter(|(_, v)| v.abs() > 1e-3)
            .map(|(i, v)| (i, *v))
            .collect();
        let at: Vec<usize> = peaks.iter().map(|p| p.0).collect();
        assert_eq!(at, [480, 1088, 1696, 2304, 2912], "{peaks:?}");
        assert!((peaks[1].1 - 0.5).abs() < 1e-4 && (peaks[2].1 - 0.25).abs() < 1e-4);
    }
}
