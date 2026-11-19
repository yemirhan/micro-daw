//! `PluckVoice` (`src/audio/voices/PluckVoice.ts`): one `Tone.PluckSynth` per voice
//! (Karplus-Strong) with velocity → level and velocity → brightness (loop dampening).
//!
//! `Tone.PluckSynth` = pink `Tone.Noise` → `LowpassCombFilter` = `OnePoleFilter` (an
//! `IIRFilterNode([a0, 0], [1, a0 − 1])`, `a0 = 1 / (sr / (2π·dampening))`, re-created on every
//! `dampening` change, i.e. every attack: fresh state, f64 history) → `FeedbackCombFilter` (Tone's
//! AudioWorklet: per channel a `DelayLine` of `sr` Float32 frames whose write head wraps after
//! `sr + 1` pushes; each frame reads `buffer[w − floor(delayTime·sr)]` (+`sr` when negative), then
//! pushes `input + delayed·feedback`, output = the delayed sample; `delayTime` and `feedback` are
//! k-rate, i.e. one value per quantum).
//!
//! An attack sets `delayTime = 1/f`, plays the noise for `attackNoise / f` seconds and sets the
//! feedback (`resonance`); a release ramps the feedback to 0 over `release` (k-rate). Output gain
//! = `velocityGain(v, velocity) · 3.5`. The noise is stereo, so the voice is stereo.
//!
//! **Safety deviation (stability).** The OnePole is `y = a0·x − (a0 − 1)·y[n−1]`: its pole
//! `1 − a0` leaves the unit circle once `a0 ≥ 2`, i.e. a dampening ≥ `sr/π` (15279 Hz at 48 kHz,
//! 14037 Hz at 44.1 kHz). Tone has the same formula and clamps the velocity-scaled dampening to
//! 16 kHz only, so a bright pluck (harp, dampening 12000, velocity 1 → 16000 Hz) diverges in
//! Chromium and here alike (non-finite output after ~1360 frames at 48 kHz). The effective
//! dampening is clamped to [`max_stable_dampening`] (`a0 ≤ 1.999`, pole magnitude ≤ 0.999):
//! every setting below that renders exactly as before; above it the voice plays the brightest
//! stable lowpass instead of blowing up.

use std::sync::Arc;

use super::common::{
    Ctx, HARD_STOP_FADE, MIN_GATE, ModBus, MonotonicClock, Voice, clamp, midi_to_freq,
    velocity_gain,
};
use super::presets::PluckPreset;
use super::sources::{NoiseScratch, NoiseSource};
use super::wa::noise::{NoiseSeeds, NoiseType, noise_buffer};
use super::wa::param::{AutoParam, Clock};

/// Largest OnePole `a0` the voice uses (pole `1 − a0` at magnitude 0.999; see the module docs).
const MAX_A0: f64 = 1.999;

/// The brightest dampening the voice plays at `sr` (`a0 = MAX_A0`): 15271.2 Hz at 48 kHz,
/// 14030.5 Hz at 44.1 kHz.
pub(crate) fn max_stable_dampening(sr: f64) -> f64 {
    MAX_A0 * sr / (2.0 * std::f64::consts::PI)
}

/// Karplus-Strong output is ~11 dB quieter than the other engines at the same settings.
const PLUCK_MAKEUP: f64 = 3.5;
const Q: usize = 128;

#[derive(Debug, Clone)]
struct Scratch {
    l: [f32; Q],
    r: [f32; Q],
    noise: NoiseScratch,
    gain: [f32; Q],
}

impl Default for Scratch {
    fn default() -> Self {
        Self {
            l: [0.0; Q],
            r: [0.0; Q],
            noise: NoiseScratch::default(),
            gain: [0.0; Q],
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PluckVoice {
    p: Arc<PluckPreset>,
    sr: f64,
    noise: NoiseSource,
    /// OnePole coefficients (a0, feedback b1) and per-channel `y[n−1]` (f64).
    a0: f64,
    b1: f64,
    y1: [f64; 2],
    delay_time: AutoParam,
    feedback: AutoParam,
    lines: [Vec<f32>; 2],
    write_head: [usize; 2],
    /// PluckSynth fields.
    resonance: f64,
    attack_noise: f64,
    release: f64,
    out_gain: AutoParam,
    clock: MonotonicClock,
    s: Box<Scratch>,
    /// The dampening of the last attack.
    last_damp: f64,
    /// Export (`Tone.Offline`): the comb's lowpass is the voice's LAST one for the whole render
    /// (Tone disconnects every replaced filter on the next clock tick, and the whole clock runs
    /// before the render), so attacks neither change nor reset it.
    fixed_damp: bool,
    /// Export pre-pass: every attack time of this voice (recorded on a scratch instrument).
    attack_log: Option<Vec<f64>>,
    /// Export: this voice's attack times over the whole render, and how many it has played.
    attacks: Vec<f64>,
    played: usize,
    /// Export: Tone's clock time at which the comb worklet's params are swapped in (see
    /// `offline_built`), applied after the voice's first attack.
    swap_at: Option<f64>,
}

impl PluckVoice {
    /// Control side (allocates the comb lines).
    pub fn new(p: &Arc<PluckPreset>, sr: f64, seeds: NoiseSeeds, cx: Clock) -> Self {
        let size = sr as usize;
        let resonance = clamp(p.resonance, 0.0, 0.999);
        let mut v = Self {
            p: p.clone(),
            sr,
            noise: NoiseSource::new(noise_buffer(NoiseType::Pink, seeds.pink), sr, cx),
            a0: 0.0,
            b1: 0.0,
            y1: [0.0; 2],
            // LowpassCombFilter defaults: delayTime 0.1 over a gain param (default 1); feedback
            // param range [0, 0.9999], delayTime [0, 1].
            delay_time: AutoParam::new(1.0, 0.1, 0.0, 1.0, cx),
            feedback: AutoParam::new(1.0, resonance, 0.0, 0.9999, cx),
            lines: [vec![0.0; size], vec![0.0; size]],
            write_head: [0; 2],
            resonance,
            attack_noise: p.attack_noise,
            release: p.release,
            out_gain: AutoParam::gain(0.0, cx),
            clock: MonotonicClock::default(),
            s: Box::default(),
            last_damp: p.dampening,
            fixed_damp: false,
            attack_log: None,
            attacks: Vec::new(),
            played: 0,
            swap_at: None,
        };
        v.set_dampening(p.dampening);
        v
    }

    /// `OnePoleFilter.frequency = f`: a new IIR filter (fresh state). `f` is clamped to
    /// [`max_stable_dampening`] (safety deviation, see the module docs).
    fn set_dampening(&mut self, f: f64) {
        let f = f.min(max_stable_dampening(self.sr));
        let t = 1.0 / (2.0 * std::f64::consts::PI * f);
        self.a0 = 1.0 / (t * self.sr);
        self.b1 = self.a0 - 1.0;
        self.y1 = [0.0; 2];
    }

    /// The dampening of the voice's last attack.
    pub(crate) fn last_dampening(&self) -> f64 {
        self.last_damp
    }

    /// Export only: this voice's lowpass is `damp` for the whole render (see `fixed_damp`).
    pub(crate) fn fix_dampening(&mut self, damp: f64) {
        self.set_dampening(damp);
        self.fixed_damp = true;
    }

    /// Export pre-pass (scratch instrument): record the attack times.
    pub(crate) fn record_attacks(&mut self) {
        self.attack_log = Some(Vec::new());
    }

    pub(crate) fn recorded_attacks(&self) -> &[f64] {
        self.attack_log.as_deref().unwrap_or(&[])
    }

    /// Export only: the voice's attack times over the whole render. `Tone.Offline` schedules
    /// everything before rendering, so an attack's `resonance.cancelScheduledValues(t)` removes
    /// the previous note's release ramp (an event at its END time) when the ramp ends after `t`:
    /// that note then rings at full feedback until the attack instead of fading out.
    pub(crate) fn set_offline_attacks(&mut self, attacks: Vec<f64>) {
        self.attacks = attacks;
    }

    /// Export: the voice's next attack comes at or before `end` (it cancels a ramp ending then).
    fn next_attack_before(&self, end: f64) -> bool {
        self.attacks
            .get(self.played)
            .is_some_and(|&next| next <= end)
    }

    /// `PluckSynth.triggerAttack(freq, time)`; Err when the noise restart throws.
    fn trigger_attack(&mut self, freq: f64, time: f64, ctx: &mut Ctx) {
        let cx = ctx.cx;
        let delay = 1.0 / freq;
        self.delay_time.set_value_at_time(delay, time, cx);
        self.noise.start(time, ctx.rng, cx);
        self.noise.stop(time + delay * self.attack_noise, cx);
        self.feedback.cancel_scheduled_values(time, cx);
        self.feedback.set_value_at_time(self.resonance, time, cx);
    }

    /// Export: the comb worklet node is created right after the voice's first attack (Tone's
    /// `addAudioWorkletModule().then`, a microtask in the app), and `Param.setParam` →
    /// `apply` gives its native params only `setValueAtTime(value at now, now)` (+ events after
    /// now). Until Tone's clock time `now` they hold the worklet's defaults (delayTime 0.1 s,
    /// feedback 0.5); both are k-rate, so when `now·sr` lands a hair above the quantum start the
    /// whole first quantum runs on them (no echo of a note shorter than 128 frames there).
    fn swap_comb_params(&mut self, now: f64, cx: Clock) {
        for (param, default) in [(&mut self.delay_time, 0.1), (&mut self.feedback, 0.5)] {
            let value = param.get_value_at_time(now);
            param.cancel_scheduled_values(0.0, cx);
            param.set_value_at_time(default, 0.0, cx);
            param.set_value_at_time(value, now, cx);
        }
    }

    fn trigger_release(&mut self, time: f64, cx: Clock) {
        // export: a ramp the voice's next attack cancels never plays (see set_offline_attacks)
        if self.next_attack_before(time + self.release) {
            self.feedback.set_ramp_point(time, cx);
            return;
        }
        self.feedback.linear_ramp_to(0.0, self.release, time, cx);
    }
}

impl Voice for PluckVoice {
    type Preset = PluckPreset;
    type Prep = ();

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
        let v = clamp(velocity, 0.0, 1.0);
        let damp = clamp(
            p.dampening * 2f64.powf(p.velocity * (v - 0.7) * 2.0),
            300.0,
            16000.0,
        );
        self.resonance = clamp(p.resonance, 0.0, 0.999);
        let gain = velocity_gain(v, p.velocity) * PLUCK_MAKEUP;
        self.last_damp = damp;
        self.played += 1;
        if let Some(log) = self.attack_log.as_mut() {
            log.push(t);
        }
        if !self.fixed_damp {
            self.set_dampening(damp);
        }
        self.out_gain.cancel_scheduled_values(t, cx);
        self.out_gain.set_value_at_time(gain, t, cx);
        self.trigger_attack(midi_to_freq(midi as f64), t, ctx);
        if let Some(now) = self.swap_at.take() {
            self.swap_comb_params(now, cx);
        }
    }

    fn glide(&mut self, _midi: i32, _time: f64, _ctx: &mut Ctx) {}

    fn release(&mut self, time: f64, ctx: &mut Ctx) -> f64 {
        let t = time.max(self.clock.last_time() + MIN_GATE);
        self.trigger_release(t, ctx.cx);
        t + self.release_duration()
    }

    fn hard_stop(&mut self, time: f64, ctx: &mut Ctx) -> f64 {
        let cx = ctx.cx;
        let t = time.max(self.clock.last_time() + 0.003);
        self.out_gain.cancel_and_hold_at_time(t, cx);
        // export: the fade ramp is cancelled by an attack before its end (a steal's attack
        // comes 9 ms later), so it never plays (see set_offline_attacks)
        if !self.next_attack_before(t + HARD_STOP_FADE) {
            self.out_gain
                .linear_ramp_to_value_at_time(0.0, t + HARD_STOP_FADE, cx);
        }
        let rel = self.release;
        self.release = HARD_STOP_FADE;
        self.trigger_release(t, cx);
        self.release = rel;
        t + HARD_STOP_FADE
    }

    fn release_duration(&self) -> f64 {
        let r = if self.p.release.is_finite() && self.p.release != 0.0 {
            self.p.release
        } else {
            0.5
        };
        r.max(0.01)
    }

    fn update(&mut self, preset: &Arc<PluckPreset>, _prep: &(), _ctx: &mut Ctx) {
        // The instrument holds the previous preset too: this drop only decrements.
        self.p = preset.clone();
        self.attack_noise = clamp(preset.attack_noise, 0.1, 20.0);
        self.release = preset.release;
    }

    fn is_idle(&self) -> bool {
        // The comb filter worklet runs every quantum (its write head position matters).
        false
    }

    fn render(&mut self, frame: u64, _m: &ModBus, l: &mut [f32], r: &mut [f32]) {
        let n = l.len();
        let sr = self.sr;
        let s = &mut *self.s;
        // k-rate params: one value per quantum.
        let mut one = [0f32; 1];
        self.delay_time.render(frame, sr, true, &mut one);
        let delay = (one[0] as f64 * sr).floor() as usize;
        self.feedback.render(frame, sr, true, &mut one);
        let fb = one[0] as f64;
        if self.noise.connected {
            self.noise
                .render(frame, &mut s.noise, &mut s.l[..n], &mut s.r[..n]);
        } else {
            s.l[..n].fill(0.0);
            s.r[..n].fill(0.0);
        }
        self.out_gain.render(frame, sr, false, &mut s.gain[..n]);
        let size = self.lines[0].len();
        for (c, buf) in [&mut s.l, &mut s.r].into_iter().enumerate() {
            let line = &mut self.lines[c];
            let mut w = self.write_head[c];
            let mut y1 = self.y1[c];
            for x in buf[..n].iter_mut() {
                // OnePole IIR (f64 history, f32 output).
                let yn = self.a0 * *x as f64 - self.b1 * y1;
                y1 = yn;
                let input = yn as f32;
                // FeedbackCombFilter worklet.
                let mut read = w as isize - delay as isize;
                if read < 0 {
                    read += size as isize;
                }
                let delayed = line.get(read as usize).copied().unwrap_or(f32::NAN);
                w += 1;
                if w > size {
                    w = 0;
                }
                let value = (input as f64 + delayed as f64 * fb) as f32;
                if let Some(slot) = line.get_mut(w) {
                    *slot = value;
                }
                *x = delayed;
            }
            self.write_head[c] = w;
            self.y1[c] = y1;
        }
        for k in 0..n {
            l[k] += s.l[k] * s.gain[k];
            r[k] += s.r[k] * s.gain[k];
        }
    }

    fn born(&mut self, _frame: u64, _sr: f64) {}

    fn clear_non_finite(&mut self) {
        let bad = |v: f64| !v.is_finite();
        if self.y1.iter().any(|&y| bad(y)) || self.lines.iter().flatten().any(|&x| !x.is_finite()) {
            self.y1 = [0.0; 2];
            for line in &mut self.lines {
                line.fill(0.0);
            }
        }
    }

    fn offline_built(&mut self, t: f64, _cx: Clock) {
        self.swap_at = Some(t);
    }

    /// Export: the comb worklet has run (on silence) since frame 0, so its write heads stand at
    /// `frame mod (sr + 1)` (they wrap after `sr + 1` pushes, which shifts the loop by a frame
    /// at each wrap: where a voice's wraps fall depends on it).
    fn offline_start(&mut self, frame: u64) {
        let period = self.lines[0].len() as u64 + 1;
        self.write_head = [(frame % period) as usize; 2];
    }

    fn prune(&mut self, now: f64) {
        self.delay_time.prune(now);
        self.feedback.prune(now);
        self.out_gain.prune(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dampening_clamp_only_touches_unstable_settings() {
        let cx = Clock::at(0.0, 48000.0);
        let p = Arc::new(crate::synth::bank::synth_presets()[29].clone());
        let crate::synth::presets::SynthPreset::Pluck(harp) = &*p else {
            panic!("preset 29 is the harp");
        };
        let mut v = PluckVoice::new(&Arc::new(harp.clone()), 48000.0, NoiseSeeds::default(), cx);
        // below the limit: Tone's formula, bit for bit
        for f in [300.0, 4000.0, 12000.0, 15000.0] {
            v.set_dampening(f);
            let t = 1.0 / (2.0 * std::f64::consts::PI * f);
            assert_eq!(v.a0, 1.0 / (t * 48000.0), "{f} Hz");
        }
        // the harp's velocity-1 dampening (16 kHz) is clamped to a stable pole
        v.set_dampening(16000.0);
        assert!((v.a0 - MAX_A0).abs() < 1e-12 && v.b1.abs() < 1.0);
        assert!((max_stable_dampening(48000.0) - 15271.2).abs() < 0.1);
        assert!((max_stable_dampening(44100.0) - 14030.5).abs() < 0.1);
    }
}
