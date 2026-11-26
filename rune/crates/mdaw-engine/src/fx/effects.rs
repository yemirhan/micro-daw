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

/// Distortion with a dry / wet mix. Two voicings:
///
/// - **classic**: `Tone.Distortion({ distortion, wet, oversample: '2x' })`, as the Electron
///   app had it (the live synth bus keeps it; a track's strip runs it only while its
///   distortion is off, where all that reaches the output is its −56 dB leak).
/// - **smooth** (a track's distortion that is on, 2026-11-26): the classic one hissed. Its
///   curve gained quiet signal by 11 to 21 dB while pulling peaks down to a third, had a dead
///   zone around zero, aliased (a hard curve at 2×), had no tone control, and its wet path
///   came out 128 frames late, so any mix below 100 % combed against the dry signal. This one
///   is a tanh drive (0 to 42 dB) with antiderivative anti-aliasing, level-matched at −9 dBFS
///   so turning the drive up adds grit more than noise floor, followed by a 12 dB / octave
///   low-pass that closes from 18 kHz to 9 kHz with the drive. For bite (the first version was
///   too mild, user 2026-11-26): the upper mids are pushed into the clipper (+9 dB above
///   ~700 Hz, growing with the drive) and the curve is biased, so it clips one side first
///   (even harmonics); a 20 Hz high-pass takes the bias's DC out. No latency: the mix stays in
///   phase.
#[derive(Clone)]
pub struct Distortion {
    pub xfade: CrossFade,
    amount: f64,
    curve: [f32; CURVE_LEN],
    shaper: [Oversampled2x; 2],
    sr: f64,
    /// Use the smooth voicing while the amount is above 0.
    smooth: bool,
    drive: [SmoothDrive; 2],
}

/// One channel of the smooth voicing's state.
#[derive(Clone, Copy, Default)]
struct SmoothDrive {
    /// The previous driven input (ADAA).
    u1: f64,
    /// The two poles of the tone filter.
    lp: [f64; 2],
    /// The pre-emphasis's low-pass (what is below it is not boosted).
    pre: f64,
    /// The DC blocker's last input and output.
    dc: [f64; 2],
}

impl SmoothDrive {
    /// At rest on silence (the ADAA's previous input is the bias, so there is no step in).
    fn fresh() -> Self {
        Self {
            u1: DRIVE_BIAS,
            ..Self::default()
        }
    }
}

/// The curve's bias (it clips the positive side first).
const DRIVE_BIAS: f64 = 0.35;

#[cfg(test)]
thread_local! {
    /// Strips built on this thread keep the classic distortion (the reference-render tests).
    pub static CLASSIC_FOR_REFERENCE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The level the smooth voicing keeps unchanged as the drive rises (−9 dBFS).
const DRIVE_REF: f64 = 0.35;

/// `ln(cosh(u))`, the antiderivative of tanh, without overflow.
#[inline]
fn ln_cosh(u: f64) -> f64 {
    let a = u.abs();
    a + (-2.0 * a).exp().ln_1p() - std::f64::consts::LN_2
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
            smooth: false,
            drive: [SmoothDrive::fresh(); 2],
        }
    }

    /// The smooth voicing whenever the amount is above 0 (see the type's docs).
    pub fn smooth(mut self) -> Self {
        // The Chromium reference renders were made with the classic voicing: the tests that
        // compare against them ask for it, so they keep checking everything around it.
        #[cfg(test)]
        if CLASSIC_FOR_REFERENCE.with(std::cell::Cell::get) {
            return self;
        }
        self.smooth = true;
        self
    }

    pub fn amount(&self) -> f64 {
        self.amount
    }

    /// The WaveShaper's oversampler state is re-initialized (see [`super::live`]).
    pub fn reset_state(&mut self) {
        for s in &mut self.shaper {
            s.reset();
        }
        self.drive = [SmoothDrive::fresh(); 2];
    }

    /// `distortion.distortion = amount` (a new curve).
    pub fn set_amount(&mut self, amount: f64) {
        self.amount = amount;
        distortion_curve(amount, &mut self.curve);
    }

    pub fn process(&mut self, frame: u64, l: &mut [f32; Q], r: &mut [f32; Q]) {
        self.xfade.update(frame, self.sr);
        let smooth = self.smooth && self.amount > 0.0;
        // Drive 0..42 dB; the makeup keeps a −9 dBFS signal where it was.
        let amount = self.amount.clamp(0.0, 1.0);
        let gain = 10f64.powf(amount * 42.0 / 20.0);
        let makeup = DRIVE_REF / (gain * DRIVE_REF).tanh();
        let cutoff = (18_000.0 * (9_000.0f64 / 18_000.0).powf(amount)).min(0.45 * self.sr);
        let pole = 1.0 - (-2.0 * std::f64::consts::PI * cutoff / self.sr).exp();
        // Pre-emphasis: up to +9 dB above ~700 Hz, more of it as the drive rises.
        let pre_pole = 1.0 - (-2.0 * std::f64::consts::PI * 700.0 / self.sr).exp();
        let pre_boost = 1.8 * (0.35 + 0.65 * amount);
        let dc_r = 1.0 - 2.0 * std::f64::consts::PI * 20.0 / self.sr;
        let bias_out = DRIVE_BIAS.tanh();
        for (ch, io) in [l, r].into_iter().enumerate() {
            let mut wet = *io;
            if smooth {
                let st = &mut self.drive[ch];
                for v in wet.iter_mut() {
                    let x = *v as f64;
                    st.pre += pre_pole * (x - st.pre);
                    let u = gain * (x + pre_boost * (x - st.pre)) + DRIVE_BIAS;
                    let d = u - st.u1;
                    // First-order ADAA: the mean of tanh between the two samples.
                    let y = if d.abs() > 1e-6 {
                        (ln_cosh(u) - ln_cosh(st.u1)) / d
                    } else {
                        (0.5 * (u + st.u1)).tanh()
                    } - bias_out;
                    st.u1 = u;
                    // DC blocker (the bias moves the clipped wave off centre).
                    let hp = y - st.dc[0] + dc_r * st.dc[1];
                    st.dc = [y, hp];
                    st.lp[0] += pole * (hp * makeup - st.lp[0]);
                    st.lp[1] += pole * (st.lp[0] - st.lp[1]);
                    *v = st.lp[1] as f32;
                }
            } else {
                self.shaper[ch].process(&self.curve, &mut wet);
            }
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

#[cfg(test)]
mod smooth_tests {
    use super::*;

    const SR: f64 = 48_000.0;

    fn clock() -> Clock {
        Clock { now: 0.0, sr: SR }
    }

    /// A sine of `amp` at `hz` through the distortion at 100 % wet; the last half second.
    fn render(mut d: Distortion, amp: f64, hz: f64) -> Vec<f32> {
        let mut out = Vec::new();
        for q in 0..375u64 {
            let mut l = [0f32; Q];
            for (k, v) in l.iter_mut().enumerate() {
                let n = (q as usize * Q + k) as f64;
                *v = (amp * (2.0 * std::f64::consts::PI * hz * n / SR).sin()) as f32;
            }
            let mut r = l;
            d.process(q * Q as u64, &mut l, &mut r);
            if q >= 188 {
                out.extend_from_slice(&l);
            }
        }
        out
    }

    fn rms(x: &[f32]) -> f64 {
        (x.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / x.len() as f64).sqrt()
    }

    fn db(a: f64, b: f64) -> f64 {
        20.0 * (a / b).log10()
    }

    /// What made the classic voicing hiss: at the default drive it lifts a quiet signal by
    /// about 11 dB. The smooth one lifts it by under 5 dB, and still clips a loud one.
    #[test]
    fn the_smooth_voicing_does_not_lift_the_noise_floor_at_the_default_drive() {
        let quiet = 0.003; // −50 dBFS
        let dry = quiet / 2f64.sqrt();
        let classic = rms(&render(Distortion::new(0.3, 1.0, clock()), quiet, 220.0));
        let smooth = rms(&render(
            Distortion::new(0.3, 1.0, clock()).smooth(),
            quiet,
            220.0,
        ));
        assert!(
            db(classic, dry) > 9.0,
            "classic: {:.1} dB",
            db(classic, dry)
        );
        assert!(db(smooth, dry) < 5.0, "smooth: {:.1} dB", db(smooth, dry));
        eprintln!(
            "quiet lift: classic {:.1} dB, smooth {:.1} dB",
            db(classic, dry),
            db(smooth, dry)
        );
        // A loud signal is clipped: its peak stays under full scale and its shape changes.
        let loud = render(Distortion::new(0.8, 1.0, clock()).smooth(), 0.9, 220.0);
        let peak = loud.iter().fold(0f32, |m, v| m.max(v.abs()));
        assert!(peak < 0.8 && peak > 0.1, "{peak}");
        // Bite: a good part of a clipped 220 Hz tone is above its fundamental.
        let fund = {
            let (mut re, mut im) = (0.0, 0.0);
            for (n, v) in loud.iter().enumerate() {
                let w = 2.0 * std::f64::consts::PI * 220.0 * n as f64 / SR;
                re += *v as f64 * w.cos();
                im += *v as f64 * w.sin();
            }
            (re * re + im * im).sqrt() * 2.0 / loud.len() as f64 / 2f64.sqrt()
        };
        let total = rms(&loud);
        let harmonics = (total * total - fund * fund).max(0.0).sqrt();
        eprintln!(
            "harmonics {:.2} of the level, peak {peak:.2}",
            harmonics / total
        );
        assert!(
            harmonics / total > 0.4,
            "harmonics {:.2}",
            harmonics / total
        );
        // No DC left by the biased curve.
        let mean = loud.iter().map(|v| *v as f64).sum::<f64>() / loud.len() as f64;
        assert!(mean.abs() < 0.01, "{mean}");
        assert!(loud.iter().all(|v| v.is_finite()));
    }

    /// No latency: at a 50 % mix the wet signal adds to the dry one instead of combing with it
    /// (the classic wet path is 128 frames late: a 187.5 Hz sine comes back inverted).
    #[test]
    fn the_smooth_voicing_stays_in_phase_with_the_dry_signal() {
        let hz = SR / 256.0;
        let level = |d: Distortion| rms(&render(d, 0.05, hz));
        let dry = 0.05 / 2f64.sqrt();
        let classic = level(Distortion::new(0.05, 0.5, clock()));
        let smooth = level(Distortion::new(0.05, 0.5, clock()).smooth());
        assert!(db(smooth, dry) > 1.0, "smooth: {:.1} dB", db(smooth, dry));
        assert!(
            db(classic, dry) < db(smooth, dry) - 3.0,
            "classic: {:.1} dB",
            db(classic, dry)
        );
    }

    /// Off (amount 0) the track's distortion is still the classic one: what leaks through at
    /// −56 dB is what the reference renders have.
    #[test]
    fn an_amount_of_zero_keeps_the_classic_path() {
        let a = render(Distortion::new(0.0, 0.0, clock()), 0.5, 440.0);
        let b = render(Distortion::new(0.0, 0.0, clock()).smooth(), 0.5, 440.0);
        assert_eq!(a, b);
    }
}
