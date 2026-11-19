//! `src/services/SampleProcessor.ts`: the Samples view's offline processing (`processAudio`) and
//! test-signal generator (`generateSignal`), reproduced from what Tone.js makes Chromium do.
//!
//! **processAudio** (order of the TS, not of the UI list):
//! 1. reverse (plain copy, before the offline render);
//! 2. only when the filter, pitch shift, delay or reverb is on: an offline render of
//!    `Player(buffer) → Filter → PitchShift → FeedbackDelay → Reverb → destination` at the
//!    buffer's rate and channel count, `trunc((len/sr + tail)·sr)` frames, `tail = min(decay +
//!    delay/(1 − feedback), 3)` (each term only when its stage is on);
//! 3. normalize (global peak → 1, skipped at peak 0 or exactly 1).
//!
//! The stages reuse the engine's Chromium models: `Tone.Filter` (one biquad, rolloff −12),
//! `Tone.FeedbackDelay` (DelayNode ring + feedback cycle), the ConvolverNode, `Tone.CrossFade`.
//! New here:
//! - [`PitchShift`]: `Tone.PitchShift` (window 0.1 s, delay 0, feedback 0, wet 1): two 1-second
//!   DelayNodes whose delay times follow sawtooth LFOs (phase 0 and 180°) between 0 and the
//!   window, cross-faded by a triangle LFO (phase 90°) through an equal-power `CrossFade`. The
//!   phase-shifted LFOs are Tone's custom `PeriodicWave`s (`_getRealImaginary`), built with
//!   [`mdaw_dsp::factory::osc::WaveTables::from_coefficients`]. The effect's own feedback delay
//!   (0 s, gain 0) is the identity.
//! - [`NoiseReverb`]: `Tone.Reverb` (preDelay 0.01): the impulse response is rendered like
//!   `Reverb.generate`: two `Tone.Noise` (white) sources each start at a random offset in Tone's
//!   shared 5-second stereo noise buffer ([`NoiseBank`], built on first use from `Math.random`),
//!   are folded to mono by the `Merge` inputs (`0.5·(L + R)`), and are shaped by a gain that is 0
//!   until preDelay, then 1, then `exponentialApproachValueAtTime(0, preDelay, decay)` (a
//!   `setTargetAtTime` with `τ = ln(decay + 1)/ln 200`, held at 90 % of the decay and ramped
//!   linearly to 0 at the end), evaluated as Chromium's AudioParam timeline does
//!   ([`reverb_envelope`]). The convolver normalizes the IR (`convolver_normalization_scale`).
//!
//! Mono buffers run the graph with `L = R` and are folded back by the 1-channel destination
//! (`0.5·(L + R)`, exact while the channels stay equal; the reverb's stereo IR is what makes them
//! differ). Buffers with more than two channels are processed in pairs (the app's decoder gives
//! one or two).
//!
//! **generateSignal**: white noise is `(Math.random()·2 − 1)·amp` over `ceil(sr·d)` samples; the
//! tones are Chromium's band-limited `OscillatorNode` at a constant frequency through
//! `Tone.Volume(gainToDb(amp))`, `trunc(sr·d)` samples, mono at the context rate.
//!
//! Randomness (`Math.random`) is a caller-supplied source so the parity test can replay the
//! Chromium page's seeded sequence; the app passes [`MathRandom`].

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use mdaw_dsp::buffer::AudioBuffer;
use mdaw_dsp::factory::osc::{OscType, WaveTables, periodic_wave_size, render_oscillator};
use mdaw_dsp::reverb_ir::convolver_normalization_scale;
use mdaw_model::types::FilterType;

use crate::fx::Q;
use crate::fx::convolver::Convolver;
use crate::fx::delay::DelayLine;
use crate::fx::effects::FeedbackDelay;
use crate::fx::filter::ToneFilter;
use crate::fx::lfo::Lfo;
use crate::fx::osc::{Osc, Tables};
use crate::fx::xfade::{CrossFade, g2a_curve, gains};
use crate::sampler::param::Clock;

/// The longest ring-out added after the source (seconds).
pub const TAIL_CAP: f64 = 3.0;
/// `Tone.Reverb` preDelay.
pub const REVERB_PRE_DELAY: f64 = 0.01;
/// `Tone.PitchShift` windowSize.
pub const PITCH_WINDOW: f64 = 0.1;
/// Tone's noise buffers: 5 s at 44.1 kHz frames, 2 channels.
pub const NOISE_FRAMES: usize = 44100 * 5;

/// `SampleFilterType`.
pub type SampleFilterType = FilterType;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FilterStage {
    pub enabled: bool,
    pub kind: FilterType,
    pub cutoff: f64,
    pub resonance: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PitchStage {
    pub enabled: bool,
    pub semitones: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReverbStage {
    pub enabled: bool,
    pub decay: f64,
    pub wet: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DelayStage {
    pub enabled: bool,
    pub time: f64,
    pub feedback: f64,
    pub wet: f64,
}

/// `SampleProcessingParams`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProcessParams {
    pub filter: FilterStage,
    pub pitch_shift: PitchStage,
    pub reverse: bool,
    pub normalize: bool,
    pub reverb: ReverbStage,
    pub delay: DelayStage,
}

/// `DEFAULT_PROCESSING`.
pub const DEFAULT_PROCESSING: ProcessParams = ProcessParams {
    filter: FilterStage {
        enabled: false,
        kind: FilterType::Lowpass,
        cutoff: 1000.0,
        resonance: 1.0,
    },
    pitch_shift: PitchStage {
        enabled: false,
        semitones: 0.0,
    },
    reverse: false,
    normalize: false,
    reverb: ReverbStage {
        enabled: false,
        decay: 2.5,
        wet: 0.3,
    },
    delay: DelayStage {
        enabled: false,
        time: 0.25,
        feedback: 0.3,
        wet: 0.3,
    },
};

impl Default for ProcessParams {
    fn default() -> Self {
        DEFAULT_PROCESSING
    }
}

impl ProcessParams {
    /// Some stage is on (the "Apply to new sample" button's condition).
    pub fn any_enabled(&self) -> bool {
        self.filter.enabled
            || self.pitch_shift.enabled
            || self.reverse
            || self.normalize
            || self.reverb.enabled
            || self.delay.enabled
    }

    /// `needsTone`: the stages that need the offline graph.
    pub fn needs_render(&self) -> bool {
        self.filter.enabled || self.pitch_shift.enabled || self.reverb.enabled || self.delay.enabled
    }

    /// The ring-out added after the source: reverb decay + delay echoes, capped at 3 s.
    pub fn tail_seconds(&self) -> f64 {
        let mut tail = 0.0;
        if self.reverb.enabled {
            tail += self.reverb.decay;
        }
        if self.delay.enabled {
            tail += self.delay.time * (1.0 / (1.0 - self.delay.feedback));
        }
        tail.min(TAIL_CAP)
    }
}

/// `SignalType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalType {
    Sine,
    Square,
    Sawtooth,
    Triangle,
    WhiteNoise,
}

impl SignalType {
    /// The TS string (`'sine'`, … `'white-noise'`).
    pub fn id(self) -> &'static str {
        match self {
            Self::Sine => "sine",
            Self::Square => "square",
            Self::Sawtooth => "sawtooth",
            Self::Triangle => "triangle",
            Self::WhiteNoise => "white-noise",
        }
    }

    fn osc(self) -> Option<OscType> {
        match self {
            Self::Sine => Some(OscType::Sine),
            Self::Square => Some(OscType::Square),
            Self::Sawtooth => Some(OscType::Sawtooth),
            Self::Triangle => Some(OscType::Triangle),
            Self::WhiteNoise => None,
        }
    }
}

/// `SignalGeneratorParams`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SignalParams {
    pub kind: SignalType,
    pub frequency: f64,
    pub duration: f64,
    pub amplitude: f64,
}

/// `DEFAULT_SIGNAL_PARAMS`.
pub const DEFAULT_SIGNAL: SignalParams = SignalParams {
    kind: SignalType::Sine,
    frequency: 440.0,
    duration: 1.0,
    amplitude: 0.8,
};

impl Default for SignalParams {
    fn default() -> Self {
        DEFAULT_SIGNAL
    }
}

/// JS `String(number)` for the values the generator names use (integers and short decimals).
pub fn js_number(x: f64) -> String {
    if x.is_finite() && x.fract() == 0.0 && x.abs() < 1e21 {
        format!("{}", x as i64)
    } else {
        format!("{x}")
    }
}

/// The generated sample's name: `white-noise ${d}s` or `${type} ${f}Hz ${d}s`.
pub fn signal_name(p: &SignalParams) -> String {
    match p.kind {
        SignalType::WhiteNoise => format!("white-noise {}s", js_number(p.duration)),
        k => format!(
            "{} {}Hz {}s",
            k.id(),
            js_number(p.frequency),
            js_number(p.duration)
        ),
    }
}

/// `Math.random()` for the app: xorshift64* seeded from the clock (any uniform [0, 1) source
/// will do: the TS uses the browser's).
pub struct MathRandom(u64);

impl MathRandom {
    pub fn new() -> Self {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0x9e37_79b9_7f4a_7c15, |d| d.as_nanos() as u64);
        Self(t | 1)
    }

    pub fn next_f64(&mut self) -> f64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        (x.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64 / (1u64 << 53) as f64
    }
}

impl Default for MathRandom {
    fn default() -> Self {
        Self::new()
    }
}

/// Tone's cached white-noise buffer (`_noiseBuffers.white`): 2 × 220,500 samples of
/// `Math.random()·2 − 1` (channel 0 first), created at the rate of the context current at its
/// first use. The app keeps one per process like Tone; if a later render runs at another rate the
/// buffer is rebuilt at that rate (Tone would resample the cached one; noise either way).
#[derive(Default)]
pub struct NoiseBank {
    white: Option<(u32, Arc<[Vec<f32>; 2]>)>,
}

impl NoiseBank {
    pub fn new() -> Self {
        Self::default()
    }

    /// A noise bank whose white buffer is `chans` at `rate` (what Tone cached earlier).
    pub fn with_white(rate: u32, chans: [Vec<f32>; 2]) -> Self {
        Self {
            white: Some((rate, Arc::new(chans))),
        }
    }

    /// The white buffer at `rate` (built from `rnd` when missing).
    pub fn white(&mut self, rate: u32, rnd: &mut dyn FnMut() -> f64) -> (u32, Arc<[Vec<f32>; 2]>) {
        if let Some((r, b)) = &self.white
            && *r == rate
        {
            return (*r, b.clone());
        }
        let mut chans: [Vec<f32>; 2] = [Vec::new(), Vec::new()];
        for ch in &mut chans {
            *ch = (0..NOISE_FRAMES)
                .map(|_| (rnd() * 2.0 - 1.0) as f32)
                .collect();
        }
        let b = Arc::new(chans);
        self.white = Some((rate, b.clone()));
        (rate, b)
    }
}

/// `reverseBuffer`.
pub fn reverse_buffer(b: &AudioBuffer) -> AudioBuffer {
    AudioBuffer {
        sample_rate: b.sample_rate,
        channels: b
            .channels
            .iter()
            .map(|c| c.iter().rev().copied().collect())
            .collect(),
    }
}

/// `normalizeBuffer` (in place): global peak across channels; scaled by `1/peak` (f64, like JS
/// numbers on a `Float32Array`) when the peak is neither 0 nor 1.
pub fn normalize_buffer(b: &mut AudioBuffer) {
    let peak = b
        .channels
        .iter()
        .flatten()
        .fold(0f32, |m, &x| if x.abs() > m { x.abs() } else { m });
    if peak > 0.0 && peak != 1.0 {
        let scale = 1.0 / peak as f64;
        for x in b.channels.iter_mut().flatten() {
            *x = (*x as f64 * scale) as f32;
        }
    }
}

// ------------------------------------------------------------------------ pitch shift

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum PhaseWave {
    /// sawtooth, phase 180°.
    Saw180,
    /// triangle, phase 90°.
    Tri90,
}

/// Tone's `_getRealImaginary(type, phase)` coefficients (f64, stored as f32), as a normalized
/// periodic wave of Chromium's table size for `sr`. Built once per (wave, size).
fn phase_wave(w: PhaseWave, sr: u32) -> Arc<WaveTables> {
    type Cache = Mutex<HashMap<(PhaseWave, usize), Arc<WaveTables>>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let size = periodic_wave_size(sr);
    let cache = CACHE.get_or_init(Default::default);
    if let Some(t) = cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&(w, size))
    {
        return t.clone();
    }
    let degrees = match w {
        PhaseWave::Saw180 => 180.0,
        PhaseWave::Tri90 => 90.0,
    };
    let phase = (degrees * std::f64::consts::PI) / 180.0;
    const PERIODIC_WAVE_SIZE: usize = 4096 / 2;
    let mut real = vec![0f32; PERIODIC_WAVE_SIZE];
    let mut imag = vec![0f32; PERIODIC_WAVE_SIZE];
    for n in 1..PERIODIC_WAVE_SIZE {
        let pi_factor = 2.0 / (n as f64 * std::f64::consts::PI);
        let b = match w {
            PhaseWave::Saw180 => pi_factor * if n & 1 == 1 { 1.0 } else { -1.0 },
            PhaseWave::Tri90 => {
                if n & 1 == 1 {
                    2.0 * (pi_factor * pi_factor) * if ((n - 1) >> 1) & 1 == 1 { -1.0 } else { 1.0 }
                } else {
                    0.0
                }
            }
        };
        if b != 0.0 {
            real[n] = (-b * (phase * n as f64).sin()) as f32;
            imag[n] = (b * (phase * n as f64).cos()) as f32;
        }
    }
    let t = Arc::new(WaveTables::from_coefficients(&real, &imag, size));
    cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert((w, size), t.clone());
    t
}

/// `Tone.PitchShift({ pitch })` with the defaults (window 0.1, delayTime 0, feedback 0, wet 1).
pub struct PitchShift {
    dry_wet: CrossFade,
    lfo_a: Lfo,
    lfo_b: Lfo,
    lfo_fade: Lfo,
    delay_a: [DelayLine; 2],
    delay_b: [DelayLine; 2],
    va: [f32; Q],
    vb: [f32; Q],
    vf: [f32; Q],
    sr: f64,
}

impl PitchShift {
    /// The LFO rate (`_frequency`): `factor·1.2/window`.
    pub fn lfo_rate(semitones: f64) -> f64 {
        let ratio = |i: f64| 2f64.powf(i / 12.0);
        let factor = if semitones < 0.0 {
            ratio(semitones - 1.0) + 1.0
        } else {
            ratio(semitones) - 1.0
        };
        factor * (1.2 / PITCH_WINDOW)
    }

    pub fn new(semitones: f64, cx: Clock) -> Self {
        let sr_u = cx.sr as u32;
        let tables = Tables::new(sr_u);
        let f = Self::lfo_rate(semitones);
        // pitch < 0: min 0, max window; else min window, max 0.
        let (min, max) = if semitones < 0.0 {
            (0.0, PITCH_WINDOW)
        } else {
            (PITCH_WINDOW, 0.0)
        };
        let lfo_a = Lfo::new(tables.clone(), OscType::Sawtooth, false, f, min, max, cx);
        let lfo_b = Lfo::with_osc(
            Osc::with_wave(tables.clone(), phase_wave(PhaseWave::Saw180, sr_u), cx.sr),
            f,
            min,
            max,
            cx,
        );
        let lfo_fade = Lfo::with_osc(
            Osc::with_wave(tables, phase_wave(PhaseWave::Tri90, sr_u), cx.sr),
            f,
            0.0,
            1.0,
            cx,
        );
        Self {
            dry_wet: CrossFade::new(1.0, cx),
            lfo_a,
            lfo_b,
            lfo_fade,
            delay_a: [DelayLine::new(1.0, cx.sr), DelayLine::new(1.0, cx.sr)],
            delay_b: [DelayLine::new(1.0, cx.sr), DelayLine::new(1.0, cx.sr)],
            va: [0.0; Q],
            vb: [0.0; Q],
            vf: [0.0; Q],
            sr: cx.sr,
        }
    }

    pub fn process(&mut self, frame: u64, l: &mut [f32; Q], r: &mut [f32; Q]) {
        self.dry_wet.update(frame, self.sr);
        self.lfo_a.render(frame, self.sr, &mut self.va);
        self.lfo_b.render(frame, self.sr, &mut self.vb);
        self.lfo_fade.render(frame, self.sr, &mut self.vf);
        let curve = g2a_curve();
        for (ch, io) in [l, r].into_iter().enumerate() {
            let mut a = *io;
            let mut b = *io;
            self.delay_a[ch].process(&mut a, &self.va, self.sr, true);
            self.delay_b[ch].process(&mut b, &self.vb, self.sr, true);
            let mut wet = [0f32; Q];
            for k in 0..Q {
                let (ga, gb) = gains(curve, self.vf[k]);
                wet[k] = a[k] * ga + b[k] * gb;
            }
            let dry = *io;
            self.dry_wet.mix(&dry, &wet, io);
        }
    }
}

// ------------------------------------------------------------------------ reverb

/// The IR gain of `Tone.Reverb.generate` per frame (`len` frames at `sr`): Chromium's timeline
/// for `setValueAtTime(0, 0)`, `setValueAtTime(1, P)`, `setTargetAtTime(0, P, τ)`,
/// `setValueAtTime(v, P + 0.9·D)` (Tone's `cancelAndHoldAtTime`) and
/// `linearRampToValueAtTime(0, P + D)`.
pub fn reverb_envelope(decay: f64, sr: f64, len: usize) -> Vec<f32> {
    let p = REVERB_PRE_DELAY;
    let hold_t = p + decay * 0.9;
    let end_t = p + decay;
    let tc = (decay + 1.0).ln() / 200f64.ln();
    // Tone's getValueAtTime(hold): the approach from 1 (the setValue before the target).
    let hold_v = (0.0 + (1.0 - 0.0) * (-(hold_t - p) / tc).exp()) as f32;
    let dtc = (1.0 - (-1.0 / (sr * tc)).exp()) as f32;
    let p_frame = (p * sr).ceil() as usize;
    let hold_frame = (hold_t * sr).ceil() as usize;
    let end_frame = (end_t * sr).ceil() as usize;
    let inv = (1.0 / (end_t - hold_t)) as f32;
    let mut out = vec![0f32; len];
    let mut v = 1f32;
    for (k, o) in out.iter_mut().enumerate() {
        *o = if k < p_frame {
            0.0
        } else if k < hold_frame {
            let start = p * sr;
            if start <= k as f64 && (k as f64) < start + 1.0 {
                v = (0.0 + (v as f64 - 0.0) * (-(k as f64 / sr - p) / tc).exp()) as f32;
            } else {
                v += (0.0 - v) * dtc;
            }
            v
        } else if k < end_frame {
            let x = ((k as f64 / sr - hold_t) * inv as f64) as f32;
            hold_v + x * (0.0 - hold_v)
        } else {
            0.0
        };
    }
    out
}

/// `Tone.Reverb.generate`'s impulse response (2 channels, `trunc((decay + 0.01)·sr)` frames).
pub fn noise_reverb_ir(
    decay: f64,
    sr: u32,
    rnd: &mut dyn FnMut() -> f64,
    noise: &mut NoiseBank,
) -> AudioBuffer {
    let srf = sr as f64;
    let len = ((decay + REVERB_PRE_DELAY) * srf) as usize;
    let (nrate, buf) = noise.white(sr, rnd);
    let dur = NOISE_FRAMES as f64 / nrate as f64;
    let env = reverb_envelope(decay, srf, len);
    let mut chans = Vec::with_capacity(2);
    for _ in 0..2 {
        // Noise.start: `Math.random()·(duration − 0.001)`, offset snapped to a frame (rate 1).
        let off = rnd() * (dur - 0.001);
        let start = (off * nrate as f64).round() as usize % NOISE_FRAMES;
        let mut c = vec![0f32; len];
        for (k, o) in c.iter_mut().enumerate() {
            let i = (start + k) % NOISE_FRAMES;
            let mono = (buf[0][i] + buf[1][i]) * 0.5;
            *o = mono * env[k];
        }
        chans.push(c);
    }
    AudioBuffer {
        sample_rate: sr,
        channels: chans,
    }
}

/// `Tone.Reverb({ decay, wet })` once its IR is ready.
pub struct NoiseReverb {
    dry_wet: CrossFade,
    conv: Box<Convolver>,
    sr: f64,
}

impl NoiseReverb {
    pub fn new(ir: &AudioBuffer, wet: f64, cx: Clock) -> Self {
        let scale = convolver_normalization_scale(ir) as f32;
        Self {
            dry_wet: CrossFade::new(wet, cx),
            conv: Box::new(Convolver::new(ir, scale)),
            sr: cx.sr,
        }
    }

    pub fn process(&mut self, frame: u64, l: &mut [f32; Q], r: &mut [f32; Q]) {
        self.dry_wet.update(frame, self.sr);
        let (mut wl, mut wr) = ([0f32; Q], [0f32; Q]);
        self.conv.process(l, r, &mut wl, &mut wr);
        let (dl, dr) = (*l, *r);
        self.dry_wet.mix(&dl, &wl, l);
        self.dry_wet.mix(&dr, &wr, r);
    }
}

// ------------------------------------------------------------------------ processAudio

/// The offline graph of one channel pair.
struct Graph {
    filter: Option<ToneFilter>,
    pitch: Option<PitchShift>,
    delay: Option<FeedbackDelay>,
    reverb: Option<NoiseReverb>,
}

/// `processAudio(buffer, params)`. `rnd` is `Math.random` (the reverb's noise), `noise` Tone's
/// shared noise buffer.
pub fn process_audio(
    source: &AudioBuffer,
    p: &ProcessParams,
    rnd: &mut dyn FnMut() -> f64,
    noise: &mut NoiseBank,
) -> AudioBuffer {
    let mut buffer = if p.reverse {
        reverse_buffer(source)
    } else {
        source.clone()
    };
    if p.needs_render() {
        buffer = render(&buffer, p, rnd, noise);
    }
    if p.normalize {
        normalize_buffer(&mut buffer);
    }
    buffer
}

fn render(
    src: &AudioBuffer,
    p: &ProcessParams,
    rnd: &mut dyn FnMut() -> f64,
    noise: &mut NoiseBank,
) -> AudioBuffer {
    let sr = src.sample_rate;
    let srf = sr as f64;
    let in_frames = src.frames();
    let duration = in_frames as f64 / srf + p.tail_seconds();
    let frames = (duration * srf) as usize;
    let cx = Clock { now: 0.0, sr: srf };
    let channels = src.channel_count().max(1);
    // The reverb's IR is built once (one Tone.Reverb in the graph).
    let ir = p
        .reverb
        .enabled
        .then(|| noise_reverb_ir(p.reverb.decay, sr, rnd, noise));
    let mut out = vec![vec![0f32; frames]; channels];
    let mut pair = 0;
    while pair < channels {
        let li = pair;
        let ri = (pair + 1).min(channels - 1);
        let stereo = ri != li;
        let mut g = Graph {
            filter: p
                .filter
                .enabled
                .then(|| ToneFilter::new(p.filter.kind, p.filter.cutoff, p.filter.resonance, cx)),
            pitch: p
                .pitch_shift
                .enabled
                .then(|| PitchShift::new(p.pitch_shift.semitones, cx)),
            delay: p
                .delay
                .enabled
                .then(|| FeedbackDelay::new(p.delay.time, p.delay.feedback, p.delay.wet, cx)),
            reverb: ir.as_ref().map(|ir| NoiseReverb::new(ir, p.reverb.wet, cx)),
        };
        let mut f = 0usize;
        while f < frames {
            let n = Q.min(frames - f);
            let (mut l, mut r) = ([0f32; Q], [0f32; Q]);
            for k in 0..n {
                let i = f + k;
                if i < in_frames {
                    l[k] = src.channels[li][i];
                    r[k] = src.channels[ri][i];
                }
            }
            let frame = f as u64;
            if let Some(s) = g.filter.as_mut() {
                s.process(frame, &mut l, &mut r);
            }
            if let Some(s) = g.pitch.as_mut() {
                s.process(frame, &mut l, &mut r);
            }
            if let Some(s) = g.delay.as_mut() {
                s.process(frame, &mut l, &mut r);
            }
            if let Some(s) = g.reverb.as_mut() {
                s.process(frame, &mut l, &mut r);
            }
            for k in 0..n {
                if stereo {
                    out[li][f + k] = l[k];
                    out[ri][f + k] = r[k];
                } else if l[k] == r[k] {
                    out[li][f + k] = l[k];
                } else {
                    // 1-channel destination: speakers down-mix.
                    out[li][f + k] = (l[k] + r[k]) * 0.5;
                }
            }
            f += n;
        }
        pair += 2;
    }
    AudioBuffer {
        sample_rate: sr,
        channels: out,
    }
}

// ------------------------------------------------------------------------ generateSignal

/// Tone's `gainToDb` / `dbToGain` round trip (`Tone.Volume(gainToDb(amp))`).
fn volume_gain(amp: f64) -> f32 {
    let db = 20.0 * (amp.ln() / std::f64::consts::LN_10);
    10f64.powf(db / 20.0) as f32
}

/// `generateSignal(params)` at the context rate `sample_rate` (mono).
pub fn generate_signal(
    p: &SignalParams,
    sample_rate: u32,
    rnd: &mut dyn FnMut() -> f64,
) -> AudioBuffer {
    let sr = sample_rate as f64;
    let data = match p.kind.osc() {
        None => {
            let len = (sr * p.duration).ceil().max(0.0) as usize;
            (0..len)
                .map(|_| ((rnd() * 2.0 - 1.0) * p.amplitude) as f32)
                .collect()
        }
        Some(shape) => {
            let frames = (p.duration * sr).max(0.0) as usize;
            let freq = vec![p.frequency as f32; frames];
            let mut y = render_oscillator(shape, &freq, 0, sample_rate);
            let g = volume_gain(p.amplitude);
            for x in &mut y {
                *x *= g;
            }
            y
        }
    };
    AudioBuffer::mono(sample_rate, data)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod parity_tests;
