//! The synthesized `DrumKit` of `src/audio/drumKit.ts`, one hit rendered offline.
//!
//! Each voice is the Tone/Web Audio graph of the TS kit evaluated sample by sample: Tone params
//! become [`Param`] timelines (same calls, same order), oscillators are Chromium wavetable
//! oscillators, filters are Web Audio biquads, shapers are Web Audio WaveShapers. Only the nodes
//! that can be non-zero for the voice are computed (every other amp of the kit sits at gain 0).
//! Noise paths are stereo (Tone's noise buffer is), mono paths are up-mixed, and the result is
//! down-mixed `0.5·(L + R)` into the 1-channel offline destination.

use super::automation::Param;
use super::biquad::{BiquadKind, tone_filter};
use super::osc::{OscType, render_oscillator};
use crate::shaping::{kick_curve_shared, snare_curve_shared, waveshape};

/// `DrumVoice` of drumKit.ts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DrumVoice {
    Kick,
    KickSub,
    Snare,
    Rim,
    Chh,
    Phh,
    Ohh,
    Clap,
    Tom,
    Crash,
    Ride,
    Shaker,
    Perc,
    Snap,
}

/// 808 metallic oscillator bank (Hz).
pub const METAL_FREQS: [f64; 6] = [205.3, 304.4, 369.6, 522.7, 540.0, 800.0];

/// GM note → tom pitch (Hz); anything else → 118.
pub fn tom_pitch(note: Option<u8>) -> f64 {
    match note.unwrap_or(45) {
        41 => 82.0,
        43 => 98.0,
        45 => 118.0,
        47 => 138.0,
        48 => 158.0,
        50 => 185.0,
        _ => 118.0,
    }
}

/// `normVelocity` of voices/common.ts.
pub fn norm_velocity(velocity: f64) -> f64 {
    if !velocity.is_finite() {
        return 0.8;
    }
    if velocity > 1.0 {
        return (velocity / 127.0).clamp(0.0, 1.0);
    }
    velocity.clamp(0.0, 1.0)
}

/// `velocityGain(v, sensitivity)`.
pub fn velocity_gain(velocity: f64, sensitivity: f64) -> f64 {
    let v = velocity.clamp(0.0, 1.0);
    let s = sensitivity.clamp(0.0, 1.0);
    1.0 - s + s * v.powf(1.7)
}

/// `pulse()` of drumKit.ts on a gain param at time `t`.
fn pulse(param: &mut Param, t: f64, peak: f64, tau: f64, hold: f64, attack: f64) {
    param.cancel_and_hold_at_time(t);
    param.linear_ramp_to_value_at_time(peak, t + attack);
    param.set_target_at_time(0.0, t + attack + hold, tau);
}

fn pulse_default(param: &mut Param, t: f64, peak: f64, tau: f64, hold: f64) {
    pulse(param, t, peak, tau, hold, 0.0008);
}

type Stereo = [Vec<f32>; 2];

struct Ctx<'a> {
    sr: u32,
    frames: usize,
    noise: &'a Stereo,
}

impl Ctx<'_> {
    fn auto(&self, p: &Param) -> Vec<f32> {
        p.render(self.sr, self.frames)
    }

    /// `Tone.Oscillator` started at 0 (phase 0) with frequency timeline `freq`, stopped at `stop`
    /// (Tone's OneShotSource sets its output gain to 0 at the stop time).
    fn osc(&self, shape: OscType, freq: &Param, stop: Option<f64>) -> Vec<f32> {
        let f = self.auto(freq);
        let mut y = render_oscillator(shape, &f, 0, self.sr);
        // OneShotSource output gain: Gain(0) → setValueAtTime(1, start) → at stop:
        // cancelAndHoldAtTime(stop) + setValueAtTime(0, stop).
        let mut g = Param::gain(0.0);
        g.set_value_at_time(1.0, 0.0);
        if let Some(s) = stop {
            g.cancel_and_hold_at_time(s);
            g.set_value_at_time(0.0, s);
        }
        mul(&mut y, &self.auto(&g));
        y
    }

    /// Noise through a Tone.Filter chain, per channel.
    fn noise_filtered(&self, stages: &[(BiquadKind, f64, f64, i32)]) -> Stereo {
        let mut out = self.noise.clone();
        for ch in &mut out {
            for &(kind, f, q, rolloff) in stages {
                tone_filter(kind, f, q, rolloff, self.sr, ch);
            }
        }
        out
    }

    /// The metal bank: 6 squares (never stopped) summed into a Gain(1/6).
    fn metal(&self) -> Vec<f32> {
        let mut sum = vec![0f32; self.frames];
        for f in METAL_FREQS {
            let y = self.osc(OscType::Square, &Param::signal(f), None);
            for (s, v) in sum.iter_mut().zip(y) {
                *s += v;
            }
        }
        let g = (1.0 / METAL_FREQS.len() as f64) as f32;
        for s in &mut sum {
            *s *= g;
        }
        sum
    }
}

fn mul(a: &mut [f32], g: &[f32]) {
    for (x, y) in a.iter_mut().zip(g) {
        *x *= *y;
    }
}

fn scale(a: &mut [f32], g: f64) {
    let g = g as f32;
    for x in a {
        *x *= g;
    }
}

fn add(a: &mut [f32], b: &[f32]) {
    for (x, y) in a.iter_mut().zip(b) {
        *x += *y;
    }
}

fn downmix(st: Stereo) -> Vec<f32> {
    let [l, r] = st;
    l.iter().zip(&r).map(|(a, b)| 0.5f32 * (a + b)).collect()
}

/// Mono `m` up-mixed and added to both channels of a stereo signal.
fn upmix_add(st: &mut Stereo, m: &[f32]) {
    for ch in st.iter_mut() {
        add(ch, m);
    }
}

/// Render one hit of `voice` (velocity as given to `DrumKit.play`, `note` for toms) at time 0
/// into `frames` mono frames. `noise` is the kit's stereo noise for those frames.
pub fn render_voice(
    voice: DrumVoice,
    velocity: f64,
    note: Option<u8>,
    sample_rate: u32,
    frames: usize,
    noise: &Stereo,
) -> Vec<f32> {
    let vel = norm_velocity(velocity);
    let v = velocity_gain(vel, 0.85);
    let c = Ctx {
        sr: sample_rate,
        frames,
        noise,
    };
    match voice {
        DrumVoice::Kick => kick(&c, v, false),
        DrumVoice::KickSub => kick(&c, v, true),
        DrumVoice::Snare => snare(&c, v, vel, 1.0),
        DrumVoice::Rim => snare(&c, v * 0.7, vel, 0.35),
        DrumVoice::Clap => clap(&c, v),
        DrumVoice::Chh => hat(&c, v, false),
        DrumVoice::Phh => hat(&c, v * 0.6, false),
        DrumVoice::Ohh => hat(&c, v, true),
        DrumVoice::Tom => tom(&c, v, tom_pitch(note)),
        DrumVoice::Crash => crash(&c, v),
        DrumVoice::Ride => ride(&c, v),
        DrumVoice::Shaker => shaker(&c, v),
        DrumVoice::Perc => perc(&c, v),
        DrumVoice::Snap => snap(&c, v),
    }
}

fn kick(c: &Ctx, v: f64, sub: bool) -> Vec<f32> {
    let t = 0.0;
    let mut f = Param::signal(50.0);
    f.cancel_scheduled_values(t);
    if sub {
        f.set_value_at_time(140.0, t);
        f.exponential_ramp_to_value_at_time(52.0, t + 0.07);
        f.exponential_ramp_to_value_at_time(41.0, t + 1.2);
    } else {
        f.set_value_at_time(190.0, t);
        f.exponential_ramp_to_value_at_time(56.0, t + 0.045);
        f.exponential_ramp_to_value_at_time(46.0, t + 0.35);
    }
    let mut body = c.osc(OscType::Sine, &f, Some(t + if sub { 2.0 } else { 0.9 }));
    let mut amp = Param::gain(0.0);
    pulse_default(
        &mut amp,
        t,
        (if sub { 0.85 } else { 0.8 }) * v,
        if sub { 0.4 } else { 0.1 },
        if sub { 0.06 } else { 0.03 },
    );
    let mut click_amp = Param::gain(0.0);
    pulse_default(
        &mut click_amp,
        t,
        (if sub { 0.2 } else { 0.45 }) * v,
        0.004,
        0.001,
    );
    mul(&mut body, &c.auto(&amp));
    let mut bus = c.noise_filtered(&[(BiquadKind::Bandpass, 3200.0, 0.8, -12)]);
    let ca = c.auto(&click_amp);
    for ch in &mut bus {
        mul(ch, &ca);
        add(ch, &body);
    }
    let curve = kick_curve_shared();
    for ch in &mut bus {
        for x in ch.iter_mut() {
            *x = waveshape(curve, *x);
        }
    }
    downmix(bus)
}

fn snare(c: &Ctx, v: f64, vel: f64, noise_amount: f64) -> Vec<f32> {
    let t = 0.0;
    let mut body = vec![0f32; c.frames];
    for (shape, f0) in [(OscType::Triangle, 185.0), (OscType::Sine, 330.0)] {
        let mut f = Param::signal(f0);
        f.cancel_scheduled_values(t);
        f.set_value_at_time(f0 * 1.35, t);
        f.exponential_ramp_to_value_at_time(f0, t + 0.03);
        add(&mut body, &c.osc(shape, &f, Some(t + 0.5)));
    }
    let mut body_amp = Param::gain(0.0);
    pulse_default(&mut body_amp, t, 0.75 * v, 0.045, 0.004);
    mul(&mut body, &c.auto(&body_amp));
    // snareNoiseBP is a lowpass whose frequency is set at the hit: 5000 + 4000·vel.
    let mut lp_freq = Param::signal(7500.0);
    lp_freq.set_value_at_time(5000.0 + 4000.0 * vel, t);
    let lp = lp_freq.value_at(t);
    let mut bus = c.noise_filtered(&[
        (BiquadKind::Highpass, 1400.0, 1.0, -12),
        (BiquadKind::Lowpass, lp, 1.0, -12),
    ]);
    let mut noise_amp = Param::gain(0.0);
    pulse_default(&mut noise_amp, t, 0.85 * v * noise_amount, 0.075, 0.006);
    let na = c.auto(&noise_amp);
    for ch in &mut bus {
        mul(ch, &na);
    }
    upmix_add(&mut bus, &body);
    let curve = snare_curve_shared();
    for ch in &mut bus {
        for x in ch.iter_mut() {
            *x = waveshape(curve, *x);
        }
    }
    downmix(bus)
}

fn clap(c: &Ctx, v: f64) -> Vec<f32> {
    let t = 0.0;
    let mut g = Param::gain(0.0);
    g.cancel_and_hold_at_time(t);
    for b in [0.0, 0.011, 0.022] {
        g.set_value_at_time(2.6 * v, t + b);
        g.set_target_at_time(0.02, t + b + 0.0005, 0.003);
    }
    g.set_value_at_time(2.8 * v, t + 0.033);
    g.set_target_at_time(0.0, t + 0.034, 0.07);
    let mut st = c.noise_filtered(&[(BiquadKind::Bandpass, 1150.0, 1.5, -12)]);
    let ga = c.auto(&g);
    for ch in &mut st {
        mul(ch, &ga);
    }
    downmix(st)
}

fn hat(c: &Ctx, v: f64, open: bool) -> Vec<f32> {
    let t = 0.0;
    let mut metal = c.metal();
    tone_filter(BiquadKind::Bandpass, 10000.0, 0.9, -12, c.sr, &mut metal);
    tone_filter(BiquadKind::Highpass, 7000.0, 1.0, -24, c.sr, &mut metal);
    let mut st = c.noise_filtered(&[(BiquadKind::Highpass, 8500.0, 1.0, -12)]);
    for ch in &mut st {
        scale(ch, 0.3);
    }
    upmix_add(&mut st, &metal);
    let mut amp = Param::gain(0.0);
    if open {
        pulse_default(&mut amp, t, 1.25 * v, 0.14, 0.01);
    } else {
        // (the open amp is choked: it was at 0 and stays at 0)
        pulse_default(&mut amp, t, 1.5 * v, 0.016, 0.002);
    }
    let a = c.auto(&amp);
    for ch in &mut st {
        mul(ch, &a);
    }
    downmix(st)
}

fn tom(c: &Ctx, v: f64, f: f64) -> Vec<f32> {
    let t = 0.0;
    let mut fr = Param::signal(118.0);
    fr.cancel_scheduled_values(t);
    fr.set_value_at_time(f * 1.6, t);
    fr.exponential_ramp_to_value_at_time(f, t + 0.05);
    fr.exponential_ramp_to_value_at_time(f * 0.9, t + 0.5);
    let mut body = c.osc(OscType::Sine, &fr, Some(t + 1.2));
    let tau = (0.2 * (118.0 / f).sqrt()).clamp(0.12, 0.3);
    let mut amp = Param::gain(0.0);
    pulse_default(&mut amp, t, 0.62 * v, tau, 0.01);
    mul(&mut body, &c.auto(&amp));
    let mut bp_freq = Param::signal(500.0);
    bp_freq.set_value_at_time(f * 5.0, t);
    let mut st = c.noise_filtered(&[(BiquadKind::Bandpass, bp_freq.value_at(t), 1.0, -12)]);
    let mut namp = Param::gain(0.0);
    pulse_default(&mut namp, t, 0.25 * v, 0.015, 0.001);
    let na = c.auto(&namp);
    for ch in &mut st {
        mul(ch, &na);
    }
    upmix_add(&mut st, &body);
    downmix(st)
}

fn crash(c: &Ctx, v: f64) -> Vec<f32> {
    let t = 0.0;
    let mut metal = c.metal();
    tone_filter(BiquadKind::Bandpass, 5200.0, 0.35, -12, c.sr, &mut metal);
    let mut st = c.noise_filtered(&[(BiquadKind::Highpass, 4500.0, 1.0, -12)]);
    upmix_add(&mut st, &metal);
    let mut g = Param::gain(0.0);
    g.cancel_and_hold_at_time(t);
    g.linear_ramp_to_value_at_time(0.42 * v, t + 0.003);
    g.set_target_at_time(0.19 * v, t + 0.003, 0.06);
    g.set_target_at_time(0.0, t + 0.15, 0.6);
    let a = c.auto(&g);
    for ch in &mut st {
        mul(ch, &a);
    }
    downmix(st)
}

fn ride(c: &Ctx, v: f64) -> Vec<f32> {
    let t = 0.0;
    let mut metal = c.metal();
    tone_filter(BiquadKind::Bandpass, 5000.0, 2.5, -12, c.sr, &mut metal);
    let mut st = c.noise_filtered(&[(BiquadKind::Highpass, 7500.0, 1.0, -12)]);
    for ch in &mut st {
        scale(ch, 0.25);
    }
    upmix_add(&mut st, &metal);
    let mut g = Param::gain(0.0);
    pulse_default(&mut g, t, 1.05 * v, 0.28, 0.003);
    let a = c.auto(&g);
    for ch in &mut st {
        mul(ch, &a);
    }
    downmix(st)
}

fn shaker(c: &Ctx, v: f64) -> Vec<f32> {
    let t = 0.0;
    let mut st = c.noise_filtered(&[
        (BiquadKind::Highpass, 5200.0, 1.0, -12),
        (BiquadKind::Bandpass, 8500.0, 0.7, -12),
    ]);
    let mut g = Param::gain(0.0);
    pulse(&mut g, t, 1.1 * v, 0.03, 0.006, 0.012);
    let a = c.auto(&g);
    for ch in &mut st {
        mul(ch, &a);
    }
    downmix(st)
}

fn perc(c: &Ctx, v: f64) -> Vec<f32> {
    let t = 0.0;
    let mut sum = c.osc(OscType::Square, &Param::signal(587.0), Some(t + 0.7));
    add(
        &mut sum,
        &c.osc(OscType::Square, &Param::signal(845.0), Some(t + 0.7)),
    );
    tone_filter(BiquadKind::Bandpass, 1200.0, 0.9, -12, c.sr, &mut sum);
    let mut g = Param::gain(0.0);
    g.cancel_and_hold_at_time(t);
    g.linear_ramp_to_value_at_time(0.7 * v, t + 0.001);
    g.set_target_at_time(0.25 * v, t + 0.002, 0.012);
    g.set_target_at_time(0.0, t + 0.03, 0.09);
    mul(&mut sum, &c.auto(&g));
    sum
}

fn snap(c: &Ctx, v: f64) -> Vec<f32> {
    let t = 0.0;
    let mut g = Param::gain(0.0);
    g.cancel_and_hold_at_time(t);
    g.set_value_at_time(2.2 * v, t);
    g.set_target_at_time(0.1 * v, t + 0.0005, 0.002);
    g.set_value_at_time(3.0 * v, t + 0.007);
    g.set_target_at_time(0.0, t + 0.0075, 0.03);
    let mut st = c.noise_filtered(&[(BiquadKind::Bandpass, 2400.0, 2.5, -12)]);
    let a = c.auto(&g);
    for ch in &mut st {
        mul(ch, &a);
    }
    downmix(st)
}
