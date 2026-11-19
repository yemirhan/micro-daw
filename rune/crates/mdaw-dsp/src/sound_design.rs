//! The Sound Designer's synth: renders a [`SoundRecipe`] (oscillator layers with pitch
//! envelopes, FM, AHDSR amp envelope, state-variable filter with its own envelope, drive, bit
//! crush and a click) to audio.
//!
//! [`Voice`] is the whole engine: a plain struct, no heap, no locks, one sample at a time, so the
//! live preview (a few voices on the audio thread) and the offline [`render`] run the exact same
//! code. Every random source is seeded at [`Voice::start`], so a render is a pure function of the
//! recipe, the transpose and the velocity, however it is split into blocks.
//!
//! A sound is a one-shot: it plays for `recipe.length` seconds, the amp envelope releasing
//! `amp.release` seconds before the end. The offline render can then cut the silent tail
//! ([`trim_tail`]).

use std::f64::consts::PI;

use mdaw_model::sound_design::{
    FM_MAX_INDEX, FilterMode, LAYERS, OscLayer, RETRIG_SPACING, SoundRecipe, Wave,
};

use crate::buffer::AudioBuffer;

/// Level under which the tail counts as silent (−60 dBFS).
pub const TAIL_THRESHOLD: f32 = 0.001;
/// Fade applied at a trimmed end.
pub const TAIL_FADE: f64 = 0.005;
/// Shortest trimmed sound.
pub const MIN_TRIMMED: f64 = 0.01;
/// How long the click layer lasts.
const CLICK_TIME: f64 = 0.012;
/// e-folding times in a "decay" (the time to fall to ~1 %).
const DECAY_TC: f64 = 4.6;

/// `1 − (1 − p)^k`: `k > 1` rises fast then slowly, `k < 1` slowly then fast.
#[inline]
fn shape(p: f64, k: f64) -> f64 {
    let p = p.clamp(0.0, 1.0);
    1.0 - (1.0 - p).powf(k)
}

/// xorshift64* → [−1, 1).
#[inline]
fn noise(state: &mut u64) -> f64 {
    let mut x = *state;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    *state = x;
    let r = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
    (r >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
}

/// PolyBLEP residual at phase `t` for a step of `dt`.
#[inline]
fn blep(t: f64, dt: f64) -> f64 {
    if dt <= 0.0 {
        0.0
    } else if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

#[inline]
fn wrap(p: f64) -> f64 {
    p - p.floor()
}

#[inline]
fn midi_hz(note: f64) -> f64 {
    440.0 * 2f64.powf((note - 69.0) / 12.0)
}

/// The amplitude envelope (a pure function of time; see [`AmpEnv`](mdaw_model::sound_design::AmpEnv)).
#[derive(Debug, Clone, Copy)]
struct Env {
    attack: f64,
    hold: f64,
    decay: f64,
    sustain: f64,
    release: f64,
    k: f64,
    retrig: f64,
    /// When the release starts and the level there.
    gate: f64,
    gate_level: f64,
}

impl Env {
    fn new(r: &SoundRecipe) -> Self {
        let a = r.amp;
        let mut e = Env {
            attack: a.attack,
            hold: a.hold,
            decay: a.decay,
            sustain: a.sustain,
            release: a.release,
            k: 4f64.powf(a.curve),
            retrig: a.retrig as f64,
            gate: (r.length - a.release).max(0.0),
            gate_level: 0.0,
        };
        e.gate_level = e.held(e.gate);
        e
    }

    /// Attack → hold → decay → sustain from time `tt` of one attack.
    fn ahds(&self, tt: f64) -> f64 {
        if tt < self.attack {
            return shape(tt / self.attack, self.k);
        }
        let t = tt - self.attack;
        if t < self.hold {
            return 1.0;
        }
        let t = t - self.hold;
        if t < self.decay {
            return 1.0 - (1.0 - self.sustain) * shape(t / self.decay, self.k);
        }
        self.sustain
    }

    /// The level while the note is held (the retrig attacks first, 11 ms apart).
    fn held(&self, t: f64) -> f64 {
        let lead = self.retrig * RETRIG_SPACING;
        if t < lead {
            self.ahds(t % RETRIG_SPACING)
        } else {
            self.ahds(t - lead)
        }
    }

    fn at(&self, t: f64) -> f64 {
        if t < self.gate {
            self.held(t)
        } else {
            self.gate_level * (1.0 - shape((t - self.gate) / self.release, self.k))
        }
    }
}

/// One playing sound. See the module docs.
#[derive(Debug, Clone)]
pub struct Voice {
    r: SoundRecipe,
    sr: f64,
    transpose: f64,
    gain: f64,
    env: Env,
    n: u64,
    total: u64,
    active: bool,
    phase: [f64; LAYERS],
    rng: [u64; LAYERS],
    pink: [[f64; 3]; LAYERS],
    click_rng: u64,
    ic1: f64,
    ic2: f64,
    filter_on: bool,
}

impl Voice {
    /// An idle voice at `sample_rate`.
    pub fn new(sample_rate: f64) -> Self {
        let r = SoundRecipe::default();
        Self {
            env: Env::new(&r),
            r,
            sr: sample_rate.max(1.0),
            transpose: 0.0,
            gain: 0.0,
            n: 0,
            total: 0,
            active: false,
            phase: [0.0; LAYERS],
            rng: [0; LAYERS],
            pink: [[0.0; 3]; LAYERS],
            click_rng: 0,
            ic1: 0.0,
            ic2: 0.0,
            filter_on: false,
        }
    }

    /// Starts `recipe` from its beginning, `transpose` semitones from its own pitch, at
    /// `velocity` (0..1, a linear gain).
    pub fn start(&mut self, recipe: &SoundRecipe, transpose: f64, velocity: f64) {
        let r = recipe.sanitized();
        let f = r.filter;
        self.filter_on =
            !(f.mode == FilterMode::Lowpass && f.cutoff >= 19_999.0 && f.env_amount >= 0.0);
        self.env = Env::new(&r);
        self.gain = 10f64.powf(r.gain_db / 20.0) * velocity.clamp(0.0, 1.0);
        self.total = (r.length * self.sr).round() as u64;
        self.transpose = if transpose.is_finite() {
            transpose
        } else {
            0.0
        };
        self.r = r;
        self.n = 0;
        self.active = true;
        self.phase = [0.0; LAYERS];
        for (i, s) in self.rng.iter_mut().enumerate() {
            *s = 0x9E37_79B9_7F4A_7C15 ^ (i as u64 + 1).wrapping_mul(0xD1B5_4A32_D192_ED03);
        }
        self.pink = [[0.0; 3]; LAYERS];
        self.click_rng = 0xA076_1D64_78BD_642F;
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Stops at once.
    pub fn stop(&mut self) {
        self.active = false;
    }

    /// Samples this voice renders in all (`length × rate`).
    pub fn total_frames(&self) -> u64 {
        self.total
    }

    fn osc(&mut self, i: usize, l: &OscLayer, t: f64, pm: f64) -> f64 {
        match l.wave {
            Wave::Noise => return noise(&mut self.rng[i]),
            Wave::Pink => {
                let w = noise(&mut self.rng[i]);
                let b = &mut self.pink[i];
                b[0] = 0.99765 * b[0] + w * 0.099_046;
                b[1] = 0.963 * b[1] + w * 0.296_516_4;
                b[2] = 0.57 * b[2] + w * 1.052_691_3;
                return (b[0] + b[1] + b[2] + w * 0.1848) * 0.25;
            }
            _ => {}
        }
        let pe = if l.pitch_amount != 0.0 {
            l.pitch_amount * (-t * DECAY_TC / l.pitch_decay).exp()
        } else {
            0.0
        };
        let note = self.r.note + self.transpose + l.coarse + l.fine / 100.0 + pe;
        let dt = (midi_hz(note) / self.sr).min(0.45);
        let p = wrap(self.phase[i] + pm);
        let x = match l.wave {
            Wave::Sine => (2.0 * PI * p).sin(),
            Wave::Triangle => 1.0 - 4.0 * (p - 0.5).abs(),
            Wave::Saw => 2.0 * p - 1.0 - blep(p, dt),
            Wave::Pulse => {
                let w = l.width;
                let mut y = if p < w { 1.0 } else { -1.0 };
                y += blep(p, dt);
                y -= blep(wrap(p - w + 1.0), dt);
                // Keep the DC of an uneven pulse out.
                y - (2.0 * w - 1.0)
            }
            Wave::Noise | Wave::Pink => 0.0,
        };
        self.phase[i] = wrap(self.phase[i] + dt);
        x
    }

    fn filter(&mut self, x: f64, t: f64) -> f64 {
        if !self.filter_on {
            return x;
        }
        let f = self.r.filter;
        let oct = if f.env_amount != 0.0 {
            f.env_amount * (-t * DECAY_TC / f.env_decay).exp()
        } else {
            0.0
        };
        let fc = (f.cutoff * 2f64.powf(oct)).clamp(20.0, self.sr * 0.45);
        let g = (PI * fc / self.sr).tan();
        let k = 2.0 - 1.9 * f.resonance;
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        let v3 = x - self.ic2;
        let v1 = a1 * self.ic1 + a2 * v3;
        let v2 = self.ic2 + a2 * self.ic1 + a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        // No denormals in the decaying states.
        if self.ic1.abs() < 1e-20 {
            self.ic1 = 0.0;
        }
        if self.ic2.abs() < 1e-20 {
            self.ic2 = 0.0;
        }
        match f.mode {
            FilterMode::Lowpass => v2,
            FilterMode::Bandpass => v1,
            FilterMode::Highpass => x - k * v1 - v2,
        }
    }

    /// The next sample (0 once the sound has ended).
    #[inline]
    pub fn next_sample(&mut self) -> f32 {
        if !self.active {
            return 0.0;
        }
        if self.n >= self.total {
            self.active = false;
            return 0.0;
        }
        let t = self.n as f64 / self.sr;
        self.n += 1;
        let layers = self.r.layers;
        let fm = self.r.fm;
        let (from, to) = (fm.from as usize, fm.to as usize);
        let fm_on = fm.amount > 0.0 && from != to && layers[from].on && layers[to].on;
        let mut raw = [0.0; LAYERS];
        let mut mix = 0.0;
        // The modulator first, so its sample of this frame drives the carrier.
        let mut order = [0usize, 1, 2];
        if fm_on {
            order = [from, (from + 1) % LAYERS, (from + 2) % LAYERS];
        }
        for i in order {
            let l = layers[i];
            if !l.on {
                continue;
            }
            let pm = if fm_on && i == to {
                raw[from] * fm.amount * FM_MAX_INDEX / (2.0 * PI)
            } else {
                0.0
            };
            raw[i] = self.osc(i, &l, t, pm);
            mix += raw[i] * l.level;
        }
        let y = self.filter(mix, t) * self.env.at(t);
        let mut y = y + if self.r.click > 0.0 && t < CLICK_TIME {
            self.r.click * noise(&mut self.click_rng) * (-t / 0.0015).exp()
        } else {
            0.0
        };
        if self.r.drive > 0.0 {
            let g = 1.0 + self.r.drive * 15.0;
            y = (g * y).tanh() / g.tanh();
        }
        if self.r.crush >= 2.0 {
            let q = 2f64.powf(self.r.crush - 1.0);
            y = (y * q).round() / q;
        }
        let out = (y * self.gain) as f32;
        if out.is_finite() { out } else { 0.0 }
    }

    /// Fills `out` (overwriting it).
    pub fn process(&mut self, out: &mut [f32]) {
        for o in out {
            *o = self.next_sample();
        }
    }
}

/// The voice's whole output, untrimmed (`length × rate` samples).
pub fn render_raw(
    recipe: &SoundRecipe,
    sample_rate: u32,
    transpose: f64,
    velocity: f64,
) -> Vec<f32> {
    let mut v = Voice::new(sample_rate as f64);
    v.start(recipe, transpose, velocity);
    let mut out = vec![0.0; v.total_frames() as usize];
    v.process(&mut out);
    out
}

/// Cuts the tail after the last sample above [`TAIL_THRESHOLD`] (keeping [`TAIL_FADE`] past it,
/// at least [`MIN_TRIMMED`]) and fades the new end out over [`TAIL_FADE`]. Nothing changes when
/// the sound is loud up to its end.
pub fn trim_tail(samples: &mut Vec<f32>, sample_rate: u32) {
    let sr = sample_rate as f64;
    let fade = (TAIL_FADE * sr).round() as usize;
    let last = samples
        .iter()
        .rposition(|x| x.abs() > TAIL_THRESHOLD)
        .map_or(0, |i| i + 1);
    let keep = (last + fade).max((MIN_TRIMMED * sr).round() as usize);
    if keep >= samples.len() {
        return;
    }
    samples.truncate(keep);
    let n = fade.min(keep);
    for k in 0..n {
        let i = keep - n + k;
        samples[i] *= 1.0 - (k + 1) as f32 / n as f32;
    }
}

/// The sound as a mono buffer at `sample_rate` (what pads and the Song play), its silent tail
/// cut when the recipe asks for it.
pub fn render(recipe: &SoundRecipe, sample_rate: u32) -> AudioBuffer {
    let mut s = render_raw(recipe, sample_rate, 0.0, 1.0);
    if recipe.trim_tail {
        trim_tail(&mut s, sample_rate);
    }
    if s.is_empty() {
        s.push(0.0);
    }
    AudioBuffer::mono(sample_rate, s)
}

#[cfg(test)]
mod tests;
