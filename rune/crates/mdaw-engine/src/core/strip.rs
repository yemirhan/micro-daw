//! The per-track strip after the instrument (`createTrackAudioNodes`):
//!
//! ```text
//! instrument → Filter → Distortion → EQ3 → [Compressor] → Chorus → FeedbackDelay → RoomReverb
//!            → Panner(pan) → Volume(dB) → Meter → Destination
//! ```
//!
//! Phase 3 implements the chain **with every effect disabled** (the state of every new track and
//! of every track whose effects were never switched on), which is not a plain pass-through:
//! - `Tone.Filter` lowpass at 18 kHz, Q 1 (−12 dB/oct: one biquad, Q in dB as in Chromium);
//! - `Tone.EQ3` at 0/0/0 dB is a crossover (`MultibandSplit`): low = LP 400 Hz, mid = HP 400 →
//!   LP 2500, high = HP 2500 (Q 1 each), summed — flat in level, not in phase;
//! - the compressor is bypassed (not in the path);
//! - Distortion, Chorus, FeedbackDelay and RoomReverb at wet 0 are NOT exactly dry: Tone's
//!   equal-power `CrossFade` turns `fade` into the pan of a `StereoPannerNode` through a
//!   1024-point `GainToAudio` WaveShaper, whose output at 0 is −0.998045 (the curve has no point
//!   at 0), not −1. So every "disabled" effect passes its dry signal × cos(0.0015355) (−1e−5 dB)
//!   and LEAKS its wet signal × sin(0.0015355) = 0.0015355 (−56.3 dB). Modelled here: the chorus
//!   wet (each channel delayed by `f32(0.0035)·sr` frames: depth 0, feedback 0), the feedback
//!   delay wet (0.25 s, feedback 0.3: the disabled-delay defaults), and the reverb send
//!   (HP 200 Hz → the RoomReverb IR); the reverb convolution itself is only rendered offline
//!   (export: [`super::Core::render_offline_with_reverb_send`] + a post-render FFT convolution),
//!   live it is left out (an 87k-tap convolution per track for a −56 dB leak). The distortion's
//!   wet is its amount-0 curve (`x/3`, 0 near 0) delayed by the 2× oversampler's latency
//!   ([`DIST_LATENCY_FRAMES`]; its half-band filters are taken as flat).
//! - `Tone.Panner` is a `StereoPannerNode` with `channelCount = 1, 'explicit'`: the stereo input
//!   is down-mixed to mono (`0.5·(L+R)`) and panned equal-power (`cos/sin((pan+1)·π/4)`).
//!
//! The FX parameters themselves (enabled effects, automation) are phase 4; this is where they
//! go. Volume and mute are applied by the core after the strip.

use mdaw_dsp::factory::biquad::{Biquad, BiquadKind};
use mdaw_dsp::reverb_ir::{SEND_LOW_CUT_HZ, SEND_LOW_CUT_Q_DB};
use mdaw_dsp::shaping::{tone_curve, waveshape};

use crate::contract::RENDER_QUANTUM;

const Q: usize = RENDER_QUANTUM;
/// `Tone.Filter` defaults of a disabled track filter.
pub const TRACK_FILTER_HZ: f64 = 18_000.0;
/// `Tone.EQ3` crossover frequencies (Tone defaults).
pub const EQ_LOW_HZ: f64 = 400.0;
pub const EQ_HIGH_HZ: f64 = 2_500.0;
/// The chorus delay (`delayTime: 3.5` ms, depth 0).
pub const CHORUS_DELAY_SEC: f32 = 0.0035;
/// The disabled FeedbackDelay's settings (`DEFAULT_TRACK_EFFECTS.delay`).
pub const DELAY_SEC: f32 = 0.25;
pub const DELAY_FEEDBACK: f32 = 0.3;

/// Latency (context frames) of a WaveShaper with `oversample = '2x'` in Chromium: the
/// UpSampler's 128-tap kernel (64) + the DownSampler's reduced 128-tap kernel (64).
pub const DIST_LATENCY_FRAMES: f64 = 128.0;

/// `Tone.Distortion` at amount 0: `setMap(x ↦ |x| < 0.001 ? 0 : x/3)` (1024 points: `setMap`'s
/// default length, not the 4096 of the constructor).
fn distortion_curve() -> &'static [f32] {
    static CURVE: std::sync::LazyLock<Vec<f32>> = std::sync::LazyLock::new(|| {
        let deg = std::f64::consts::PI / 180.0;
        tone_curve(1024, |x| {
            if x.abs() < 0.001 {
                0.0
            } else {
                (3.0 * x * 20.0 * deg) / std::f64::consts::PI
            }
        })
    });
    &CURVE
}

/// (dry, wet) gains of a Tone `CrossFade` at `fade = 0` (see the module docs).
pub fn disabled_crossfade_gains() -> (f32, f32) {
    let curve = tone_curve(1024, |x| x.abs() * 2.0 - 1.0);
    let pan = waveshape(&curve, 0.0) as f64;
    let rad = (pan.clamp(-1.0, 1.0) * 0.5 + 0.5) * std::f64::consts::FRAC_PI_2;
    (rad.cos() as f32, rad.sin() as f32)
}

/// A Chromium `DelayNode` with a constant delay (linear interpolation).
#[derive(Clone)]
struct DelayLine {
    buf: Vec<f32>,
    w: usize,
    frames: f64,
}

impl DelayLine {
    fn new(frames: f64) -> Self {
        Self {
            buf: vec![0.0; frames.ceil() as usize + 2],
            w: 0,
            frames,
        }
    }

    /// Write `x`, read the delayed sample (Chromium writes first, then reads).
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let len = self.buf.len();
        self.buf[self.w] = x;
        let out = self.read();
        self.w = (self.w + 1) % len;
        out
    }

    /// The delayed sample at the current write position (before writing it).
    #[inline]
    fn read(&self) -> f32 {
        let len = self.buf.len() as f64;
        let mut pos = self.w as f64 + len - self.frames;
        if pos >= len {
            pos -= len;
        }
        let i1 = pos as usize;
        let i2 = (i1 + 1) % self.buf.len();
        let f = pos - i1 as f64;
        ((1.0 - f) * self.buf[i1] as f64 + f * self.buf[i2] as f64) as f32
    }

    #[inline]
    fn write(&mut self, x: f32) {
        self.buf[self.w] = x;
        self.w = (self.w + 1) % self.buf.len();
    }
}

#[derive(Clone)]
struct Chain {
    filter: Biquad,
    low: Biquad,
    low_mid: Biquad,
    mid: Biquad,
    high: Biquad,
    chorus: DelayLine,
    delay: DelayLine,
    send_hp: Biquad,
    dist: DelayLine,
    /// [`distortion_curve`], resolved off the audio thread (its first use allocates).
    dist_curve: &'static [f32],
}

impl Chain {
    fn new(sr: u32) -> Self {
        let srf = sr as f64;
        Self {
            filter: Biquad::new(BiquadKind::Lowpass, TRACK_FILTER_HZ, 1.0, sr),
            low: Biquad::new(BiquadKind::Lowpass, EQ_LOW_HZ, 1.0, sr),
            low_mid: Biquad::new(BiquadKind::Highpass, EQ_LOW_HZ, 1.0, sr),
            mid: Biquad::new(BiquadKind::Lowpass, EQ_HIGH_HZ, 1.0, sr),
            high: Biquad::new(BiquadKind::Highpass, EQ_HIGH_HZ, 1.0, sr),
            chorus: DelayLine::new(CHORUS_DELAY_SEC as f64 * srf),
            delay: DelayLine::new(DELAY_SEC as f64 * srf),
            send_hp: Biquad::new(BiquadKind::Highpass, SEND_LOW_CUT_HZ, SEND_LOW_CUT_Q_DB, sr),
            dist: DelayLine::new(DIST_LATENCY_FRAMES),
            dist_curve: distortion_curve(),
        }
    }

    /// One sample through the chain up to the panner; returns (output, reverb send).
    #[inline]
    fn tick(&mut self, x: f32, dry: f32, wet: f32) -> (f32, f32) {
        let y0 = self.filter.tick(x);
        let y1 = y0 * dry + waveshape(self.dist_curve, self.dist.tick(y0)) * wet;
        let y2 = {
            let low = self.low.tick(y1);
            let mid = self.mid.tick(self.low_mid.tick(y1));
            let high = self.high.tick(y1);
            low + mid + high
        };
        let y3 = y2 * dry + self.chorus.tick(y2) * wet;
        // FeedbackEffect: send = input + feedback·return; return = delay(send).
        let echo = self.delay.read();
        self.delay.write(y3 + DELAY_FEEDBACK * echo);
        let y4 = y3 * dry + echo * wet;
        let send = self.send_hp.tick(y4);
        (y4 * dry, send)
    }
}

/// Filter + EQ3 crossover + disabled-effect leaks of both channels, and the panner. Built on the
/// control side (allocates its delay lines) and moved into the core.
pub struct Strip {
    ch: [Chain; 2],
    dry: f32,
    wet: f32,
    /// The last quantum's reverb sends (`wet · HP200(reverb input)`), per channel.
    pub(crate) send: [[f32; Q]; 2],
}

impl Strip {
    pub fn new(sr: u32) -> Self {
        let c = Chain::new(sr);
        let (dry, wet) = disabled_crossfade_gains();
        Self {
            ch: [c.clone(), c],
            dry,
            wet,
            send: [[0.0; Q]; 2],
        }
    }

    /// Run `l` / `r` (the instrument's stereo output) through the chain and the panner, in place.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32], pan: f32) {
        let (gl, gr) = pan_gains(pan);
        let (dry, wet) = (self.dry, self.wet);
        for (k, (a, b)) in l.iter_mut().zip(r.iter_mut()).enumerate() {
            let (el, sl) = self.ch[0].tick(*a, dry, wet);
            let (er, sr) = self.ch[1].tick(*b, dry, wet);
            if k < Q {
                self.send[0][k] = sl * wet;
                self.send[1][k] = sr * wet;
            }
            let mono = 0.5 * (el + er);
            *a = (mono as f64 * gl) as f32;
            *b = (mono as f64 * gr) as f32;
        }
    }
}

/// Chromium `StereoPanner` gains for a mono input.
pub fn pan_gains(pan: f32) -> (f64, f64) {
    let p = (pan as f64).clamp(-1.0, 1.0);
    let rad = (p * 0.5 + 0.5) * std::f64::consts::FRAC_PI_2;
    (rad.cos(), rad.sin())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centre_pan_is_equal_power_and_the_chain_is_flat_at_dc() {
        let (l, r) = pan_gains(0.0);
        assert!((l - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-15 && (l - r).abs() < 1e-15);
        assert_eq!(pan_gains(-1.0), (1.0, 0.0));
        // constant in → 0.7071 × (dry⁴ + the delay leak's echoes) out
        let mut s = Strip::new(48_000);
        let mut a = vec![0.5f32; 6_000];
        let mut b = a.clone();
        s.process(&mut a, &mut b, 0.0);
        let want = 0.5 * std::f64::consts::FRAC_1_SQRT_2;
        // + the chorus / distortion leaks (≈ +0.2 %)
        assert!((a[5_999] as f64 / want - 1.0).abs() < 5e-3 && a[5_999] == b[5_999]);
    }

    #[test]
    fn a_disabled_crossfade_leaks_its_wet_signal_at_minus_56_db() {
        let (dry, wet) = disabled_crossfade_gains();
        assert!((20.0 * (wet as f64).log10() + 56.27).abs() < 0.05, "{wet}");
        assert!(1.0 - dry < 2e-6);
        // an impulse comes back from the chorus 168 frames later and from the delay at 12000
        let mut s = Strip::new(48_000);
        let mut a = vec![0f32; 12_400];
        a[0] = 1.0;
        let mut b = a.clone();
        s.process(&mut a, &mut b, 0.0);
        let near = |i: usize| {
            a[i - 4..i + 40]
                .iter()
                .map(|x| x.abs())
                .fold(0f32, f32::max)
        };
        assert!(near(168 + 4) > 1e-5 && near(12_004) > 1e-5);
        assert!(a[8_000..11_990].iter().all(|x| x.abs() < 1e-6));
    }
}
