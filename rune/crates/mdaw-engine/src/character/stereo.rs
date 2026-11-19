//! Tone's stereo effects as CharacterChain uses them: `Tremolo` and `Chorus` (both
//! `StereoEffect`s: stereo input → split → per-channel effect → merge → `CrossFade(wet)` with
//! the dry input).
//!
//! - `CrossFade`: `fade` → `GainToAudio` WaveShaper (`|x|·2 − 1`, 1024 points) → the pan of a
//!   `StereoPannerNode` fed a constant 1 → per frame `dry gain = cos θ`, `wet gain = sin θ`,
//!   `θ = (pan·0.5 + 0.5)·π/2` (f64, stored f32). Output = `dry·gd + wet·gw` in f32.
//! - `Tremolo` (frequency, depth, spread 140): two LFOs `min 1, max 0` at phases
//!   `90 − spread/2` (L) and `90 + spread/2` (R), amplitude = depth, each the gain of its
//!   channel; wet 1. Its `frequency` / `depth` are Signals (`rampTo`: exponential / linear).
//! - `Chorus` (0.6 Hz, delay 3.2 ms, depth, spread 180, feedback 0): two LFOs at phases 0 (L)
//!   and 180 (R) between `max(dt − dt·depth, 0)` and `dt + dt·depth` driving the delayTime of a
//!   `DelayNode` per channel (a-rate, linear interpolation). `depth` is set at once, `wet` ramps
//!   linearly. The `StereoFeedbackEffect` loop is silent (feedback gain 0).

use std::sync::LazyLock;

use mdaw_dsp::shaping::{tone_curve, waveshape};

use super::lfo::Lfo;
use super::param::{Automated, Curve};

/// Tone's `rampTo` duration in CharacterChain.update.
pub const RAMP: f64 = 0.05;

static G2A: LazyLock<Vec<f32>> = LazyLock::new(|| tone_curve(1024, |x| x.abs() * 2.0 - 1.0));

/// `(dry, wet)` gains of a Tone `CrossFade` at `fade`.
#[inline]
pub fn crossfade_gains(fade: f32) -> (f32, f32) {
    // (the curve is built on first use: constructors call this, never the audio thread first)
    let pan = waveshape(&G2A, fade) as f64;
    let rad = (pan.clamp(-1.0, 1.0) * 0.5 + 0.5) * std::f64::consts::FRAC_PI_2;
    (rad.cos() as f32, rad.sin() as f32)
}

#[derive(Debug, Clone)]
pub struct Tremolo {
    lfo: [Lfo; 2],
    pub frequency: Automated,
    pub depth: Automated,
    wet: (f32, f32),
}

impl Tremolo {
    pub const SPREAD: f64 = 140.0;

    /// `new Tone.Tremolo({ frequency, depth, spread: 140 }).start(now)`.
    pub fn new(sr: u32, frequency: f64, depth: f64, start_frame: u64) -> Self {
        let l = Lfo::new(sr, 90.0 - Self::SPREAD / 2.0, 1.0, 0.0, start_frame);
        let r = Lfo::new(sr, 90.0 + Self::SPREAD / 2.0, 1.0, 0.0, start_frame);
        Self {
            lfo: [l, r],
            frequency: Automated::new(frequency),
            depth: Automated::new(depth),
            wet: crossfade_gains(1.0),
        }
    }

    pub fn reset(&mut self, frequency: f64, depth: f64, start_frame: u64) {
        for l in &mut self.lfo {
            l.restart(start_frame);
        }
        self.frequency = Automated::new(frequency);
        self.depth = Automated::new(depth);
    }

    /// `depth.rampTo(d, 0.05)` and `frequency.rampTo(f, 0.05)` at `now`.
    pub fn ramp(&mut self, depth: f64, frequency: f64, now: f64) {
        self.depth.ramp_to(depth, RAMP, now, Curve::Linear);
        self.frequency
            .ramp_to(frequency, RAMP, now, Curve::Exponential);
    }

    pub fn process(&mut self, frame: u64, sr: f64, l: &mut [f32], r: &mut [f32]) {
        let (gd, gw) = self.wet;
        for i in 0..l.len() {
            let f = frame + i as u64;
            let t = f as f64 / sr;
            let freq = self.frequency.at(t);
            let depth = self.depth.at(t);
            let gl = self.lfo[0].tick(f, freq, depth);
            let gr = self.lfo[1].tick(f, freq, depth);
            l[i] = l[i] * gd + (l[i] * gl) * gw;
            r[i] = r[i] * gd + (r[i] * gr) * gw;
        }
    }
}

/// A Chromium `DelayNode` (`new Tone.Delay()`: maxDelay 1 s) with an a-rate delay time, as
/// Blink's NEON `Delay::ProcessARateVector` computes it (the capture machine is arm64; the x86
/// SSE path is the same arithmetic): the whole quantum is written into a circular buffer of
/// `128 + ceil(1 s · sr)` frames first, then per frame, all in f32,
/// `pos = w + (len − max(d, 0)·sr)` (wrapped once), `i = trunc(pos)`, `f = pos − i`,
/// `y = s[i] + f·(s[i+1] − s[i])`. The f32 read position (≈ 0.004 frame steps at 48 kHz) is
/// audible in the error budget, so it is reproduced exactly.
#[derive(Debug, Clone)]
struct DelayLine {
    buf: Vec<f32>,
    w: usize,
}

impl DelayLine {
    fn new(sr: u32) -> Self {
        Self {
            buf: vec![0.0; 128 + sr as usize],
            w: 0,
        }
    }

    fn reset(&mut self) {
        self.buf.fill(0.0);
        self.w = 0;
    }

    /// Write `x` (the quantum is written before any read, so reading a delay below one frame
    /// sees this quantum's later input like Chromium does: only matters for delays < 1 frame).
    #[inline]
    fn write(&mut self, k: usize, x: f32) {
        let len = self.buf.len();
        self.buf[(self.w + k) % len] = x;
    }

    #[inline]
    fn read(&self, k: usize, delay: f32, sr: f32) -> f32 {
        let len = self.buf.len();
        let len_f = len as f32;
        let w = ((self.w + k) % len) as f32;
        let mut pos = w + (len_f - delay.max(0.0) * sr);
        if pos >= len_f {
            pos -= len_f;
        }
        let mut i1 = pos as i32 as usize;
        if i1 >= len {
            i1 -= len;
        }
        let i2 = if i1 + 1 >= len { i1 + 1 - len } else { i1 + 1 };
        let f = pos - i1 as f32;
        let (s1, s2) = (self.buf[i1], self.buf[i2]);
        s1 + f * (s2 - s1)
    }

    fn advance(&mut self, n: usize) {
        self.w = (self.w + n) % self.buf.len();
    }
}

#[derive(Debug, Clone)]
pub struct Chorus {
    lfo: [Lfo; 2],
    delay: [DelayLine; 2],
    pub wet: Automated,
    depth: f64,
    sr: f32,
    /// The right delay kernel does not run in the quantum the chorus is created in (captured
    /// Chromium behaviour: its output is silent and that quantum's input never enters the line;
    /// the left one runs). Frames before this are skipped.
    right_from: u64,
}

impl Chorus {
    pub const FREQUENCY: f32 = 0.6;
    /// `delayTime: 3.2` ms.
    pub const DELAY: f64 = 3.2 / 1000.0;

    /// `new Tone.Chorus({ frequency: 0.6, delayTime: 3.2, depth, spread: 180, wet }).start(now)`.
    pub fn new(sr: u32, depth: f64, wet: f64, start_frame: u64) -> Self {
        let mut c = Self {
            lfo: [
                Lfo::new(sr, 0.0, 0.0, 1.0, start_frame),
                Lfo::new(sr, 180.0, 0.0, 1.0, start_frame),
            ],
            delay: [DelayLine::new(sr), DelayLine::new(sr)],
            wet: Automated::new(wet),
            depth,
            sr: sr as f32,
            right_from: next_quantum(start_frame),
        };
        c.set_depth(depth);
        c
    }

    pub fn reset(&mut self, depth: f64, wet: f64, start_frame: u64) {
        for l in &mut self.lfo {
            l.restart(start_frame);
        }
        for d in &mut self.delay {
            d.reset();
        }
        self.right_from = next_quantum(start_frame);
        self.wet = Automated::new(wet);
        self.set_depth(depth);
    }

    /// `chorus.depth = depth` (LFO ranges set at once).
    pub fn set_depth(&mut self, depth: f64) {
        self.depth = depth;
        let dt = Self::DELAY;
        let dev = dt * depth;
        for l in &mut self.lfo {
            l.set_range((dt - dev).max(0.0), dt + dev);
        }
    }

    pub fn depth(&self) -> f64 {
        self.depth
    }

    pub fn process(&mut self, frame: u64, sr: f64, l: &mut [f32], r: &mut [f32]) {
        let n = l.len();
        let right_on = frame >= self.right_from;
        for i in 0..n {
            self.delay[0].write(i, l[i]);
            if right_on {
                self.delay[1].write(i, r[i]);
            }
        }
        let mut gains = crossfade_gains(self.wet.at(frame as f64 / sr));
        let ramping = !self.wet.constant_from(frame as f64 / sr);
        for i in 0..n {
            let f = frame + i as u64;
            if ramping {
                gains = crossfade_gains(self.wet.at(f as f64 / sr));
            }
            let (gd, gw) = gains;
            let dl = self.lfo[0].tick(f, Self::FREQUENCY, 1.0);
            let dr = self.lfo[1].tick(f, Self::FREQUENCY, 1.0);
            let wl = self.delay[0].read(i, dl, self.sr);
            let wr = if right_on {
                self.delay[1].read(i, dr, self.sr)
            } else {
                0.0
            };
            l[i] = l[i] * gd + wl * gw;
            r[i] = r[i] * gd + wr * gw;
        }
        self.delay[0].advance(n);
        if right_on {
            self.delay[1].advance(n);
        }
    }
}

/// The first quantum boundary after `frame`.
fn next_quantum(frame: u64) -> u64 {
    (frame / 128 + 1) * 128
}
