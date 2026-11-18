//! `FMVoice` (`src/audio/voices/FMVoice.ts`): 2-operator FM + a decaying "ping" partial.
//!
//! ```text
//!   modulator --> depth (Hz, per note) --> modGain (× modEnv 0..1) --+--> carrier.frequency
//!   carrier -----------------------------------------------------------+--> amp env --> out
//!   ping (sine, ratio) --> ping env (fast decay) ----------------------------^
//! ```
//!
//! Depth = `index · velIdx · keyIdx · f · ratio` with `velIdx = 1 − v2i + v2i·vel` and
//! `keyIdx = clamp(2^(−keyScale·(midi − 60)/24), .15, 2)`. The carrier frequency signal is its own
//! value plus the modulation (audio input), clamped by the oscillator to ±nyquist.

use super::common::{
    Ctx, HARD_STOP_FADE, MIN_GATE, ModBus, MonotonicClock, Voice, clamp, midi_to_freq,
    velocity_gain,
};
use super::presets::{CarrierWave, FmPreset};
use super::sources::{OscScratch, ToneOsc};
use super::wa::envelope::Envelope;
use super::wa::param::{AutoParam, Clock};
use super::wa::wave::BasicShape;

const Q: usize = 128;

fn carrier_shape(w: CarrierWave) -> BasicShape {
    match w {
        CarrierWave::Sine => BasicShape::Sine,
        CarrierWave::Triangle => BasicShape::Triangle,
        CarrierWave::Square => BasicShape::Square,
        CarrierWave::Sawtooth => BasicShape::Sawtooth,
    }
}

#[derive(Debug, Clone)]
struct Scratch {
    osc: OscScratch,
    cfreq: [f32; Q],
    mfreq: [f32; Q],
    pfreq: [f32; Q],
    modu: [f32; Q],
    depth: [f32; Q],
    env: [f32; Q],
    carrier: [f32; Q],
    ping: [f32; Q],
}

impl Default for Scratch {
    fn default() -> Self {
        Self {
            osc: OscScratch::default(),
            cfreq: [0.0; Q],
            mfreq: [0.0; Q],
            pfreq: [0.0; Q],
            modu: [0.0; Q],
            depth: [0.0; Q],
            env: [0.0; Q],
            carrier: [0.0; Q],
            ping: [0.0; Q],
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct FmVoice {
    p: FmPreset,
    sr: f64,
    carrier: ToneOsc,
    carrier_freq: AutoParam,
    modulator: ToneOsc,
    mod_freq: AutoParam,
    depth: AutoParam,
    mod_env: Envelope,
    ping: ToneOsc,
    ping_freq: AutoParam,
    ping_env: Envelope,
    amp: Envelope,
    clock: MonotonicClock,
    /// Quantum whose first frame the voice's signals skip (see `signals_skip_first_frame`).
    dead_frame: Option<u64>,
    s: Box<Scratch>,
}

impl FmVoice {
    pub fn new(p: &FmPreset, sr: f64, cx: Clock) -> Self {
        let a = Self::amp_options(p);
        let me = &p.mod_env;
        let mut v = Self {
            p: p.clone(),
            sr,
            carrier: ToneOsc::basic(carrier_shape(p.carrier.wave), sr, cx),
            carrier_freq: AutoParam::signal(440.0, cx),
            modulator: ToneOsc::basic(BasicShape::Sine, sr, cx),
            mod_freq: AutoParam::signal(440.0, cx),
            depth: AutoParam::gain(0.0, cx),
            mod_env: Envelope::new(me.attack, me.decay, me.sustain, me.release, cx),
            ping: ToneOsc::basic(BasicShape::Sine, sr, cx),
            ping_freq: AutoParam::signal(440.0, cx),
            ping_env: Envelope::new(0.001, 0.1, 0.0, 0.05, cx),
            amp: Envelope::new(a.0, a.1, a.2, a.3, cx),
            clock: MonotonicClock::default(),
            dead_frame: None,
            s: Box::default(),
        };
        v.apply_static(cx);
        v
    }

    fn amp_options(p: &FmPreset) -> (f64, f64, f64, f64) {
        (
            p.amp.attack.max(0.001),
            p.amp.decay.max(0.001),
            clamp(p.amp.sustain, 0.0, 1.0),
            p.amp.release.max(0.005),
        )
    }

    fn apply_static(&mut self, cx: Clock) {
        let p = &self.p;
        let ping_level = clamp(p.ping.level, 0.0, 1.0);
        self.carrier.set_level(1.0 / (1.0 + ping_level * 0.5), cx);
        self.ping.set_level(ping_level, cx);
        self.carrier.set_detune(p.detune, cx);
        let d = p.ping.decay.max(0.005);
        self.ping_env.set(0.001, d, 0.0, d.min(0.2));
    }

    fn stop_sources(&mut self, t: f64, cx: Clock) {
        self.carrier.stop(t, cx);
        self.modulator.stop(t, cx);
        self.ping.stop(t, cx);
    }
}

impl Voice for FmVoice {
    type Preset = FmPreset;

    fn attack(
        &mut self,
        midi: i32,
        time: f64,
        velocity: f64,
        _glide_from: Option<i32>,
        ctx: &mut Ctx,
    ) {
        let cx = ctx.cx;
        let t = self.clock.next(time);
        let p = &self.p;
        let f = midi_to_freq(midi as f64);
        let ratio = p.ratio.max(0.01);
        self.carrier_freq.cancel_scheduled_values(t, cx);
        self.carrier_freq.set_value_at_time(f, t, cx);
        self.mod_freq.set_value_at_time(f * ratio, t, cx);
        self.ping_freq
            .set_value_at_time((f * p.ping.ratio.max(0.5)).min(18000.0), t, cx);
        let vel_idx = 1.0 - p.velocity_to_index + p.velocity_to_index * clamp(velocity, 0.0, 1.0);
        let key_idx = clamp(
            2f64.powf((-p.key_scale * (midi - 60) as f64) / 24.0),
            0.15,
            2.0,
        );
        let index = p.index.max(0.0) * vel_idx * key_idx;
        self.depth.set_value_at_time(index * f * ratio, t, cx);
        let ping_on = p.ping.level > 0.0;
        let amp_vel = p.amp.velocity;
        self.carrier.start(t, cx);
        if index > 0.0 {
            self.modulator.start(t, cx);
        }
        if ping_on {
            self.ping.start(t, cx);
        }
        self.mod_env.trigger_attack(t, 1.0, cx);
        self.ping_env
            .trigger_attack(t, velocity_gain(velocity, 0.6), cx);
        self.amp
            .trigger_attack(t, velocity_gain(velocity, amp_vel), cx);
    }

    fn glide(&mut self, midi: i32, time: f64, ctx: &mut Ctx) {
        let cx = ctx.cx;
        let t = time.max(self.clock.last_time());
        let f = midi_to_freq(midi as f64);
        self.carrier_freq.exponential_ramp_to(f, 0.03, t, cx);
        let r = self.p.ratio.max(0.01);
        self.mod_freq.exponential_ramp_to(f * r, 0.03, t, cx);
    }

    fn release(&mut self, time: f64, ctx: &mut Ctx) -> f64 {
        let cx = ctx.cx;
        let t = time.max(self.clock.last_time() + MIN_GATE);
        self.amp.trigger_release(t, cx);
        self.mod_env.trigger_release(t, cx);
        self.ping_env.trigger_release(t, cx);
        let end = t + self.release_duration();
        self.stop_sources(end + 0.05, cx);
        end
    }

    fn hard_stop(&mut self, time: f64, ctx: &mut Ctx) -> f64 {
        let cx = ctx.cx;
        let t = time.max(self.clock.last_time() + 0.003);
        let rel = self.amp.release;
        self.amp.release = HARD_STOP_FADE;
        self.amp.trigger_release(t, cx);
        self.amp.release = rel;
        self.mod_env.trigger_release(t, cx);
        self.ping_env.trigger_release(t, cx);
        let end = t + HARD_STOP_FADE;
        self.stop_sources(end + 0.03, cx);
        end
    }

    fn release_duration(&self) -> f64 {
        self.p.amp.release.max(0.005)
    }

    fn update(&mut self, preset: &FmPreset, ctx: &mut Ctx) {
        let prev = std::mem::replace(&mut self.p, preset.clone());
        if prev.carrier.wave != preset.carrier.wave {
            self.carrier.set_basic(carrier_shape(preset.carrier.wave));
        }
        let me = &preset.mod_env;
        self.mod_env
            .set(me.attack, me.decay, me.sustain, me.release);
        let a = Self::amp_options(preset);
        self.amp.set(a.0, a.1, a.2, a.3);
        self.apply_static(ctx.cx);
    }

    fn is_idle(&self) -> bool {
        self.amp.sig.idle_value() == Some(0.0)
            && self.carrier.is_idle()
            && self.modulator.is_idle()
            && self.ping.is_idle()
    }

    fn render(&mut self, frame: u64, m: &ModBus, l: &mut [f32], r: &mut [f32]) {
        let n = l.len();
        let sr = self.sr;
        let s = &mut *self.s;
        // Modulator → depth → modGain (× modEnv).
        self.mod_freq.render(frame, sr, false, &mut s.mfreq[..n]);
        let dead = self.dead_frame == Some(frame);
        if dead {
            s.mfreq[0] = 0.0;
        }
        s.modu[..n].fill(0.0);
        self.modulator.render(
            frame,
            &s.mfreq[..n],
            m.vibrato,
            &mut s.osc,
            &mut s.modu[..n],
        );
        self.depth.render(frame, sr, false, &mut s.depth[..n]);
        self.mod_env.render(frame, sr, &mut s.env[..n]);
        // Carrier frequency signal = its value + the modulation.
        self.carrier_freq
            .render(frame, sr, false, &mut s.cfreq[..n]);
        for k in 0..n {
            s.cfreq[k] += s.modu[k] * s.depth[k] * s.env[k];
        }
        if dead {
            s.cfreq[0] = 0.0;
        }
        s.carrier[..n].fill(0.0);
        self.carrier.render(
            frame,
            &s.cfreq[..n],
            m.vibrato,
            &mut s.osc,
            &mut s.carrier[..n],
        );
        // Ping → its envelope; summed with the carrier into the amp envelope.
        self.ping_freq.render(frame, sr, false, &mut s.pfreq[..n]);
        if dead {
            s.pfreq[0] = 0.0;
        }
        s.ping[..n].fill(0.0);
        self.ping.render(
            frame,
            &s.pfreq[..n],
            m.vibrato,
            &mut s.osc,
            &mut s.ping[..n],
        );
        self.ping_env.render(frame, sr, &mut s.env[..n]);
        for k in 0..n {
            s.carrier[k] += s.ping[k] * s.env[k];
        }
        self.amp.render(frame, sr, &mut s.env[..n]);
        for k in 0..n {
            let y = s.carrier[k] * s.env[k];
            l[k] += y;
            r[k] += y;
        }
    }

    fn born(&mut self, frame: u64, sr: f64) {
        self.dead_frame = super::common::signals_skip_first_frame(frame, sr).then_some(frame);
    }

    fn prune(&mut self, now: f64) {
        self.carrier_freq.prune(now);
        self.mod_freq.prune(now);
        self.ping_freq.prune(now);
        self.depth.prune(now);
        self.mod_env.prune(now);
        self.ping_env.prune(now);
        self.amp.prune(now);
    }
}
