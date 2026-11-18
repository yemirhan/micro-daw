//! `SubtractiveVoice` (`src/audio/voices/SubtractiveVoice.ts`):
//!
//! ```text
//!   oscA (unison) --+
//!   oscB (unison) --+--> mixer --> filter (cutoff + envelope/LFO via detune) --> amp env --> out
//!   sub osc --------+
//!   noise -> env ---+
//! ```
//!
//! Pitch is one frequency signal shared by all oscillators (glide / pitch envelope act on it);
//! the filter envelope is applied in cents on the filter's detune. Note (parity): the Electron
//! voice never applies `oscA/oscB.semi`/`fine` at construction (`omniOptions` has no detune);
//! only a later live `update()` does. This port does the same.

use mdaw_dsp::factory::Mulberry32;

use super::common::{
    Ctx, HARD_STOP_FADE, MIN_GATE, ModBus, MonotonicClock, Voice, clamp, midi_to_freq,
    velocity_gain,
};
use super::presets::{SubWave, SubtractivePreset};
use super::sources::{NoiseScratch, NoiseSource, OscScratch, ToneOsc};
use super::wa::biquad::ToneFilter;
use super::wa::envelope::Envelope;
use super::wa::noise::{NoiseSeeds, NoiseType, noise_buffer};
use super::wa::param::{AutoParam, Clock};
use super::wa::wave::BasicShape;

const NOISE_TRIM: f64 = 0.45;
const MAX_CUTOFF: f64 = 18000.0;
const Q: usize = 128;

pub(crate) fn noise_seed(seeds: &NoiseSeeds, kind: NoiseType) -> u32 {
    match kind {
        NoiseType::White => seeds.white,
        NoiseType::Pink => seeds.pink,
        NoiseType::Brown => seeds.brown,
    }
}

fn sub_shape(w: SubWave) -> BasicShape {
    match w {
        SubWave::Sine => BasicShape::Sine,
        SubWave::Triangle => BasicShape::Triangle,
        SubWave::Square => BasicShape::Square,
    }
}

#[derive(Debug, Clone)]
struct Scratch {
    osc: OscScratch,
    freq: [f32; Q],
    mix_l: [f32; Q],
    mix_r: [f32; Q],
    noise_l: [f32; Q],
    noise_r: [f32; Q],
    noise: NoiseScratch,
    env: [f32; Q],
    ffreq: [f32; Q],
    fdet: [f32; Q],
    fq: [f32; Q],
    depth: [f32; Q],
}

impl Default for Scratch {
    fn default() -> Self {
        Self {
            osc: OscScratch::default(),
            freq: [0.0; Q],
            mix_l: [0.0; Q],
            mix_r: [0.0; Q],
            noise_l: [0.0; Q],
            noise_r: [0.0; Q],
            noise: NoiseScratch::default(),
            env: [0.0; Q],
            ffreq: [0.0; Q],
            fdet: [0.0; Q],
            fq: [0.0; Q],
            depth: [0.0; Q],
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SubtractiveVoice {
    p: SubtractivePreset,
    sr: f64,
    seeds: NoiseSeeds,
    freq: AutoParam,
    osc_a: ToneOsc,
    osc_b: ToneOsc,
    sub: ToneOsc,
    noise: NoiseSource,
    noise_env: Envelope,
    filter: ToneFilter,
    filter_freq: AutoParam,
    q: AutoParam,
    filter_env: Envelope,
    env_depth: AutoParam,
    amp: Envelope,
    clock: MonotonicClock,
    note: i32,
    velocity: f64,
    stereo: bool,
    /// Quantum whose first frame the voice's signals skip (see `signals_skip_first_frame`).
    dead_frame: Option<u64>,
    s: Box<Scratch>,
}

impl SubtractiveVoice {
    pub fn new(p: &SubtractivePreset, sr: f64, seeds: NoiseSeeds, cx: Clock) -> Self {
        let a = Self::amp_options(p);
        let mut v = Self {
            p: p.clone(),
            sr,
            seeds,
            freq: AutoParam::signal(261.63, cx),
            osc_a: ToneOsc::omni(&p.osc_a, sr, cx),
            osc_b: ToneOsc::omni(&p.osc_b, sr, cx),
            sub: ToneOsc::basic(sub_shape(p.sub.wave), sr, cx),
            noise: NoiseSource::new(
                noise_buffer(p.noise.kind, noise_seed(&seeds, p.noise.kind)),
                sr,
                cx,
            ),
            noise_env: Envelope::new(0.001, 0.05, 1.0, 0.05, cx),
            filter: ToneFilter::new(p.filter.kind, p.filter.rolloff, sr),
            filter_freq: AutoParam::signal(p.filter.cutoff, cx),
            q: AutoParam::signal(p.filter.resonance, cx),
            filter_env: Envelope::new(
                p.filter_env.attack,
                p.filter_env.decay,
                p.filter_env.sustain,
                p.filter_env.release,
                cx,
            ),
            env_depth: AutoParam::gain(0.0, cx),
            amp: Envelope::new(a.0, a.1, a.2, a.3, cx),
            clock: MonotonicClock::default(),
            note: 60,
            velocity: 0.8,
            stereo: false,
            dead_frame: None,
            s: Box::default(),
        };
        v.apply_static(cx);
        v
    }

    fn amp_options(p: &SubtractivePreset) -> (f64, f64, f64, f64) {
        (
            p.amp.attack.max(0.001),
            p.amp.decay.max(0.001),
            clamp(p.amp.sustain, 0.0, 1.0),
            p.amp.release.max(0.005),
        )
    }

    /// Levels, tuning, resonance: everything that is a plain value.
    fn apply_static(&mut self, cx: Clock) {
        let p = &self.p;
        let sustained = p.noise.decay <= 0.0;
        let sum = p.osc_a.level
            + p.osc_b.level
            + p.sub.level
            + if sustained {
                p.noise.level * NOISE_TRIM
            } else {
                0.0
            };
        let norm = 1.0 / sum.max(1.0);
        self.osc_a.set_level(p.osc_a.level * norm, cx);
        self.osc_b.set_level(p.osc_b.level * norm, cx);
        self.sub.set_level(p.sub.level * norm, cx);
        self.noise.set_level(p.noise.level * norm * NOISE_TRIM, cx);
        let octave = clamp(p.sub.octave, -2.0, -1.0).round() * 1200.0;
        self.sub.set_detune(octave, cx);
        self.q.set_value(clamp(p.filter.resonance, 0.1, 30.0), cx);
        if sustained {
            self.noise_env
                .set(0.001, 0.01, 1.0, p.amp.release.max(0.005));
        } else {
            self.noise_env.set(0.001, p.noise.decay, 0.0, p.noise.decay);
        }
    }

    fn cutoff_for(&self, midi: i32, velocity: f64) -> f64 {
        let f = &self.p.filter;
        let keyed = f.cutoff * 2f64.powf((f.key_track * (midi - 60) as f64) / 12.0);
        let vel = 2f64.powf(f.velocity * (velocity - 0.75) * 1.5);
        clamp(keyed * vel, 20.0, MAX_CUTOFF)
    }

    fn env_cents(&self, midi: i32, velocity: f64) -> f64 {
        let f = &self.p.filter;
        let octaves = f.env_amount * (1.0 - f.velocity + f.velocity * velocity);
        if octaves <= 0.0 {
            return octaves.max(-8.0) * 1200.0;
        }
        let base = self.cutoff_for(midi, velocity);
        let headroom = (20000.0 / base).log2().max(0.0);
        octaves.min(headroom) * 1200.0
    }

    fn start_sources(&mut self, t: f64, rng: &mut Mulberry32, cx: Clock) {
        let p = &self.p;
        let (a, b, s, n) = (p.osc_a.level, p.osc_b.level, p.sub.level, p.noise.level);
        if a > 0.0 {
            self.osc_a.start(t, cx);
        }
        if b > 0.0 {
            self.osc_b.start(t, cx);
        }
        if s > 0.0 {
            self.sub.start(t, cx);
        }
        if n > 0.0 {
            self.noise.start(t, rng, cx);
        }
    }

    fn stop_sources(&mut self, t: f64, cx: Clock) {
        self.osc_a.stop(t, cx);
        self.osc_b.stop(t, cx);
        self.sub.stop(t, cx);
        self.noise.stop(t, cx);
    }
}

impl Voice for SubtractiveVoice {
    type Preset = SubtractivePreset;

    fn attack(
        &mut self,
        midi: i32,
        time: f64,
        velocity: f64,
        glide_from: Option<i32>,
        ctx: &mut Ctx,
    ) {
        let cx = ctx.cx;
        let t = self.clock.next(time);
        self.note = midi;
        self.velocity = velocity;
        let f = midi_to_freq(midi as f64);
        let pe = &self.p.pitch_env;
        if pe.amount != 0.0 && pe.decay > 0.0 {
            self.freq.cancel_scheduled_values(t, cx);
            self.freq
                .set_value_at_time(f * 2f64.powf(pe.amount / 12.0), t, cx);
            self.freq
                .exponential_ramp_to_value_at_time(f, t + pe.decay, cx);
        } else if glide_from.is_some() && self.p.glide > 0.001 {
            self.freq.exponential_ramp_to(f, self.p.glide, t, cx);
        } else {
            self.freq.cancel_scheduled_values(t, cx);
            self.freq.set_value_at_time(f, t, cx);
        }
        self.start_sources(t, ctx.rng, cx);
        let cutoff = self.cutoff_for(midi, velocity);
        self.filter_freq.set_value_at_time(cutoff, t, cx);
        let cents = self.env_cents(midi, velocity);
        self.env_depth.set_value_at_time(cents, t, cx);
        self.filter_env.trigger_attack(t, 1.0, cx);
        self.noise_env.trigger_attack(t, 1.0, cx);
        self.amp
            .trigger_attack(t, velocity_gain(velocity, self.p.amp.velocity), cx);
    }

    fn glide(&mut self, midi: i32, time: f64, ctx: &mut Ctx) {
        let cx = ctx.cx;
        let t = time.max(self.clock.last_time());
        self.note = midi;
        let g = self.p.glide.max(0.005);
        self.freq
            .exponential_ramp_to(midi_to_freq(midi as f64), g, t, cx);
        let cutoff = self.cutoff_for(midi, self.velocity);
        self.filter_freq.linear_ramp_to(cutoff, g, t, cx);
    }

    fn release(&mut self, time: f64, ctx: &mut Ctx) -> f64 {
        let cx = ctx.cx;
        let t = time.max(self.clock.last_time() + MIN_GATE);
        self.amp.trigger_release(t, cx);
        self.filter_env.trigger_release(t, cx);
        self.noise_env.trigger_release(t, cx);
        let end = t + self.release_duration();
        self.stop_sources(end + 0.05, cx);
        end
    }

    fn hard_stop(&mut self, time: f64, ctx: &mut Ctx) -> f64 {
        let cx = ctx.cx;
        let t = time.max(self.clock.last_time() + 0.003);
        let rel = self.amp.release;
        let n_rel = self.noise_env.release;
        self.amp.release = HARD_STOP_FADE;
        self.noise_env.release = HARD_STOP_FADE;
        self.amp.trigger_release(t, cx);
        self.noise_env.trigger_release(t, cx);
        self.filter_env.trigger_release(t, cx);
        self.amp.release = rel;
        self.noise_env.release = n_rel;
        let end = t + HARD_STOP_FADE;
        self.stop_sources(end + 0.03, cx);
        end
    }

    fn release_duration(&self) -> f64 {
        self.p.amp.release.max(0.005)
    }

    fn update(&mut self, preset: &SubtractivePreset, ctx: &mut Ctx) {
        let cx = ctx.cx;
        let prev = std::mem::replace(&mut self.p, preset.clone());
        let p = preset;
        self.osc_a.update(&p.osc_a, cx);
        self.osc_b.update(&p.osc_b, cx);
        if prev.sub.wave != p.sub.wave {
            self.sub.set_basic(sub_shape(p.sub.wave));
        }
        if prev.noise.kind != p.noise.kind {
            let buffer = noise_buffer(p.noise.kind, noise_seed(&self.seeds, p.noise.kind));
            self.noise.set_buffer(buffer, ctx.rng, cx);
        }
        if prev.filter.kind != p.filter.kind {
            self.filter.set_kind(p.filter.kind);
        }
        if prev.filter.rolloff != p.filter.rolloff {
            self.filter.set_rolloff(p.filter.rolloff);
        }
        let fe = &p.filter_env;
        self.filter_env
            .set(fe.attack, fe.decay, fe.sustain, fe.release);
        let a = Self::amp_options(p);
        self.amp.set(a.0, a.1, a.2, a.3);
        self.apply_static(cx);
        let now = cx.now;
        let cutoff = self.cutoff_for(self.note, self.velocity);
        self.filter_freq.linear_ramp_to(cutoff, 0.03, now, cx);
        let cents = self.env_cents(self.note, self.velocity);
        self.env_depth.linear_ramp_to(cents, 0.03, now, cx);
    }

    fn is_idle(&self) -> bool {
        self.filter.is_frozen()
            && self.amp.sig.idle_value() == Some(0.0)
            && self.osc_a.is_idle()
            && self.osc_b.is_idle()
            && self.sub.is_idle()
            && self.noise.is_idle()
    }

    fn render(&mut self, frame: u64, m: &ModBus, l: &mut [f32], r: &mut [f32]) {
        let n = l.len();
        let sr = self.sr;
        let s = &mut *self.s;
        self.freq.render(frame, sr, false, &mut s.freq[..n]);
        if self.dead_frame == Some(frame) {
            s.freq[0] = 0.0;
        }
        s.mix_l[..n].fill(0.0);
        let freq = &s.freq[..n];
        let mut silent = self
            .osc_a
            .render(frame, freq, m.vibrato, &mut s.osc, &mut s.mix_l[..n]);
        silent &= self
            .osc_b
            .render(frame, freq, m.vibrato, &mut s.osc, &mut s.mix_l[..n]);
        silent &= self
            .sub
            .render(frame, freq, m.vibrato, &mut s.osc, &mut s.mix_l[..n]);
        if self.noise.connected && !self.stereo {
            // The noise buffer source makes the mixer stereo: Chromium re-creates the filter kernels.
            self.stereo = true;
            self.filter.reset();
        }
        self.noise_env.render(frame, sr, &mut s.env[..n]);
        if self.stereo {
            silent &= self.noise.render(
                frame,
                &mut s.noise,
                &mut s.noise_l[..n],
                &mut s.noise_r[..n],
            );
            for k in 0..n {
                let mono = s.mix_l[k];
                s.mix_l[k] = mono + s.noise_l[k] * s.env[k];
                s.mix_r[k] = mono + s.noise_r[k] * s.env[k];
            }
        }
        // Filter params: frequency (clamped to [0, nyquist]), detune = env depth + LFO, Q.
        self.filter_freq.render(frame, sr, false, &mut s.ffreq[..n]);
        let nyq = (sr / 2.0) as f32;
        for v in &mut s.ffreq[..n] {
            *v = v.clamp(0.0, nyq);
        }
        self.filter_env.render(frame, sr, &mut s.env[..n]);
        self.env_depth.render(frame, sr, false, &mut s.depth[..n]);
        for k in 0..n {
            s.fdet[k] = s.env[k] * s.depth[k] + m.filter[k];
        }
        self.q.render(frame, sr, false, &mut s.fq[..n]);
        let chans = if self.stereo { 2 } else { 1 };
        self.filter.process(
            frame,
            silent,
            &s.ffreq[..n],
            &s.fdet[..n],
            &s.fq[..n],
            [&mut s.mix_l[..n], &mut s.mix_r[..n]],
            chans,
        );
        self.amp.render(frame, sr, &mut s.env[..n]);
        if self.stereo {
            for k in 0..n {
                l[k] += s.mix_l[k] * s.env[k];
                r[k] += s.mix_r[k] * s.env[k];
            }
        } else {
            for k in 0..n {
                let y = s.mix_l[k] * s.env[k];
                l[k] += y;
                r[k] += y;
            }
        }
    }

    fn born(&mut self, frame: u64, sr: f64) {
        self.dead_frame = super::common::signals_skip_first_frame(frame, sr).then_some(frame);
    }

    fn offline_built(&mut self, t: f64, cx: Clock) {
        // `applyStatic` (the sub's octave detune) and the fat stacks' spread, at `t`
        self.osc_a.offline_built(t, cx);
        self.osc_b.offline_built(t, cx);
        self.sub.offline_built(t, cx);
    }

    fn prune(&mut self, now: f64) {
        self.freq.prune(now);
        self.filter_freq.prune(now);
        self.env_depth.prune(now);
        self.filter_env.prune(now);
        self.noise_env.prune(now);
        self.amp.prune(now);
    }
}
