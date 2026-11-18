//! The live synth bus of `AudioEngine` (src/services/AudioEngine.ts): what the on-screen piano /
//! MIDI keyboard instrument plays through.
//!
//! ```text
//! instrument → Gain(1) → [Distortion 2×] → Filter (+LFO) → [EQ3] → [Compressor knee 6] → [Chorus]
//!            → [PingPongDelay] → [RoomReverb(decay 1.8, pre 0.022, damping 0.55)] → Volume(−12 dB)
//! ```
//!
//! Bracketed stages are only wired in while they do something (`rewire`, AudioEngine.ts:388-418):
//! distortion `wet > 0 && amount > 0`, EQ any band ≠ 0, compressor `ratio > 1.01`, chorus
//! `depth > 0`, delay `wet > 0`, reverb `wet > 0`. A stage taken out is disconnected on both
//! sides, so Chromium no longer pulls it: its state (delay lines, reverb tail, compressor
//! envelope, LFO phase) freezes and resumes where it stopped when it is wired back in. Here an
//! unwired stage is simply not processed; its params keep their timelines (they are evaluated by
//! frame), like AudioParams whose node is not pulled.
//!
//! Setters ([`LiveOp`], one per `AudioEngine.setX`) keep the TS ramps: 30 ms (`rampTo`) for the
//! wets, EQ bands and delay feedback, 20 ms for cutoff and Q, 50 ms for the delay time; the
//! compressor's threshold / ratio / attack / release are set at once. `rampTo` is exponential on
//! dB / Hz params (EQ bands, cutoff, volume), linear otherwise.
//!
//! Kept TS quirks:
//! - the filter LFO is built with Tone's default range (0..1 Hz) and only gets its real range
//!   from `updateLfoRange` (at start / on a cutoff or depth change);
//! - while the LFO runs, `setFilterCutoff` only re-ranges it;
//! - once the LFO has been connected, `filter.frequency` stays overridden, so turning it off
//!   (`setFilterLfoDepth(0)`: `frequency.value = cutoff`) writes 0 Hz: a lowpass then goes silent
//!   (see `fx/track.rs`).
//!
//! Deviation: mute is a flag here, so `setVolume` while muted does not un-mute (Tone's
//! `volume.rampTo` replaces the −∞ of `mute = true`, so the TS un-mutes).

use mdaw_dsp::reverb_ir::ReverbOptions;
use mdaw_model::types::{EffectParams, FilterType, LfoWaveform};

use super::Q;
use super::compressor::{Compressor, CompressorParams};
use super::effects::{Chorus, Distortion, PingPongDelay, RoomReverb};
use super::filter::{Eq3, ToneFilter};
use super::osc::{Tables, osc_type};
use super::state::lfo_range;
use super::tone::{ToneParam, Units};
use crate::sampler::param::Clock;

/// `DEFAULT_VOLUME` (constants.ts).
pub const LIVE_DEFAULT_VOLUME_DB: f64 = -12.0;

/// One `AudioEngine` setter (applied at the command's time).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LiveOp {
    ReverbWet(f64),
    ChorusDepth(f64),
    FilterCutoff(f64),
    FilterResonance(f64),
    FilterType(FilterType),
    FilterLfoRate(f64),
    FilterLfoDepth(f64),
    FilterLfoWave(LfoWaveform),
    DelayTime(f64),
    DelayFeedback(f64),
    DelayWet(f64),
    DistortionAmount(f64),
    DistortionWet(f64),
    EqLow(f64),
    EqMid(f64),
    EqHigh(f64),
    CompThreshold(f64),
    CompRatio(f64),
    CompAttack(f64),
    CompRelease(f64),
    /// `setVolume(db)`: 30 ms ramp.
    Volume(f64),
    /// `mute()` / `unmute()`.
    Mute(bool),
}

impl LiveOp {
    /// The params after this setter (`effectParams.x = v`); `None` for volume / mute.
    pub fn update(self, p: &mut EffectParams) -> bool {
        match self {
            LiveOp::ReverbWet(v) => p.reverb_wet = v,
            LiveOp::ChorusDepth(v) => p.chorus_depth = v,
            LiveOp::FilterCutoff(v) => p.filter_cutoff = v,
            LiveOp::FilterResonance(v) => p.filter_resonance = v,
            LiveOp::FilterType(v) => p.filter_type = v,
            LiveOp::FilterLfoRate(v) => p.filter_lfo_rate = v,
            LiveOp::FilterLfoDepth(v) => p.filter_lfo_depth = v,
            LiveOp::FilterLfoWave(v) => p.filter_lfo_wave = v,
            LiveOp::DelayTime(v) => p.delay_time = v,
            LiveOp::DelayFeedback(v) => p.delay_feedback = v,
            LiveOp::DelayWet(v) => p.delay_wet = v,
            LiveOp::DistortionAmount(v) => p.distortion_amount = v,
            LiveOp::DistortionWet(v) => p.distortion_wet = v,
            LiveOp::EqLow(v) => p.eq_low = v,
            LiveOp::EqMid(v) => p.eq_mid = v,
            LiveOp::EqHigh(v) => p.eq_high = v,
            LiveOp::CompThreshold(v) => p.comp_threshold = v,
            LiveOp::CompRatio(v) => p.comp_ratio = v,
            LiveOp::CompAttack(v) => p.comp_attack = v,
            LiveOp::CompRelease(v) => p.comp_release = v,
            LiveOp::Volume(_) | LiveOp::Mute(_) => return false,
        }
        true
    }
}

/// `chorusDepthFor(amount)`.
pub fn chorus_depth_for(amount: f64) -> f64 {
    amount.clamp(0.0, 1.0) * 0.6
}

/// `chorusWetFor(amount)`.
pub fn chorus_wet_for(amount: f64) -> f64 {
    if amount > 0.0 {
        0.15 + 0.35 * amount.min(1.0)
    } else {
        0.0
    }
}

/// Which stages `rewire` puts in the path: dist, eq, comp, chorus, delay, reverb.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stages {
    pub dist: bool,
    pub eq: bool,
    pub comp: bool,
    pub chorus: bool,
    pub delay: bool,
    pub reverb: bool,
}

impl Stages {
    pub fn of(p: &EffectParams) -> Self {
        Self {
            dist: p.distortion_wet > 0.0 && p.distortion_amount > 0.0,
            eq: p.eq_low != 0.0 || p.eq_mid != 0.0 || p.eq_high != 0.0,
            comp: p.comp_ratio > 1.01,
            chorus: p.chorus_depth > 0.0,
            delay: p.delay_wet > 0.0,
            reverb: p.reverb_wet > 0.0,
        }
    }
}

/// The live bus (see the module docs). Built on the control side (allocates), moved into the core.
pub struct LiveFx {
    sr: f64,
    tables: Tables,
    p: EffectParams,
    stages: Stages,
    dist: Distortion,
    filter: ToneFilter,
    eq: Eq3,
    comp: Compressor,
    threshold: ToneParam,
    ratio: ToneParam,
    attack: ToneParam,
    release: ToneParam,
    knee: ToneParam,
    chorus: Chorus,
    delay: PingPongDelay,
    reverb: RoomReverb,
    volume: ToneParam,
    muted: bool,
    vv: [f32; Q],
    /// The chorus / delay were wired in for the first time: their next quantum's right input is
    /// silent (see `rewire`).
    chorus_fresh: bool,
    delay_fresh: bool,
    /// The chorus / delay have been processed at least once.
    chorus_ran: bool,
    delay_ran: bool,
}

impl LiveFx {
    /// `AudioEngine.doStart()` with `params` (the effect params set before start), at context
    /// time `t`.
    pub fn new(p: &EffectParams, sample_rate: u32, t: f64) -> Self {
        let sr = sample_rate as f64;
        let c = Clock { now: t, sr };
        let tables = Tables::new(sample_rate);
        let cp = |native: f64, v: f64, units, range| ToneParam::new(native, v, units, range, c);
        let mut s = Self {
            sr,
            p: *p,
            stages: Stages::of(p),
            dist: Distortion::new(p.distortion_amount, p.distortion_wet, c),
            filter: ToneFilter::new(p.filter_type, p.filter_cutoff, p.filter_resonance, c),
            eq: Eq3::new(p.eq_low, p.eq_mid, p.eq_high, c),
            comp: Compressor::new(sample_rate as f32),
            threshold: cp(-24.0, p.comp_threshold, Units::RawDecibels, (-100.0, 0.0)),
            ratio: cp(12.0, p.comp_ratio, Units::Linear, (1.0, 20.0)),
            attack: cp(0.003, p.comp_attack, Units::Linear, (0.0, 1.0)),
            release: cp(0.25, p.comp_release, Units::Linear, (0.0, 1.0)),
            knee: cp(30.0, 6.0, Units::RawDecibels, (0.0, 40.0)),
            chorus: Chorus::new(
                &tables,
                0.7,
                3.5,
                chorus_depth_for(p.chorus_depth),
                chorus_wet_for(p.chorus_depth),
                c,
            ),
            delay: PingPongDelay::new(p.delay_time, p.delay_feedback, p.delay_wet, c),
            reverb: RoomReverb::new(
                &ReverbOptions {
                    decay: 1.8,
                    pre_delay: 0.022,
                    damping: 0.55,
                    wet: p.reverb_wet,
                },
                c,
            ),
            volume: ToneParam::gain(LIVE_DEFAULT_VOLUME_DB, Units::Decibels, c),
            muted: false,
            vv: [0.0; Q],
            chorus_fresh: false,
            delay_fresh: false,
            chorus_ran: false,
            delay_ran: false,
            tables,
        };
        if p.filter_lfo_depth > 0.0 {
            s.connect_lfo(c);
            s.update_lfo_range(c);
        }
        s
    }

    pub fn params(&self) -> &EffectParams {
        &self.p
    }

    pub fn stages(&self) -> Stages {
        self.stages
    }

    fn connect_lfo(&mut self, c: Clock) {
        if self.filter.lfo.is_none() {
            let shape = osc_type(self.p.filter_lfo_wave);
            // Tone.LFO defaults: min 0, max 1
            self.filter
                .connect_lfo(&self.tables, shape, self.p.filter_lfo_rate, 0.0, 1.0, c);
        }
    }

    fn disconnect_lfo(&mut self, c: Clock) {
        if self.filter.lfo.is_some() {
            self.filter.dispose_lfo();
            self.filter.frequency.set_value(self.p.filter_cutoff, c);
        }
    }

    fn update_lfo_range(&mut self, c: Clock) {
        if self.p.filter_lfo_depth == 0.0 {
            return;
        }
        let (min, max) = lfo_range(self.p.filter_cutoff, self.p.filter_lfo_depth);
        if let Some(lfo) = self.filter.lfo.as_mut() {
            lfo.set_min(min, c);
            lfo.set_max(max, c);
        }
    }

    /// `rewire()`: a stage put back into the path resets the nodes at its input whose channel
    /// count followed the disconnected input (BiquadFilter / WaveShaper kernels are re-initialized
    /// when it changes: the distortion's oversampler, the EQ crossover, the reverb's send
    /// highpass); everything else resumes from its frozen state. A stereo effect (chorus,
    /// ping-pong delay) wired in for the first time after the bus started (it was off at
    /// `start()`) gets a silent right input for that first quantum: its input's buses still have
    /// their initial single channel there, so the splitter's second output is silent (measured:
    /// `live_filter_*` vs `live_all` / `live_rewire` in fx_chains.json).
    fn rewire(&mut self) {
        let next = Stages::of(&self.p);
        let old = self.stages;
        self.chorus_fresh |= next.chorus && !old.chorus && !self.chorus_ran;
        self.delay_fresh |= next.delay && !old.delay && !self.delay_ran;
        if next.dist && !old.dist {
            self.dist.reset_state();
        }
        if next.eq && !old.eq {
            self.eq.reset_state();
        }
        if next.reverb && !old.reverb {
            self.reverb.reset_state();
        }
        self.stages = next;
    }

    /// Apply one setter at context time `t`.
    pub fn apply(&mut self, op: LiveOp, t: f64) {
        let c = Clock {
            now: t,
            sr: self.sr,
        };
        op.update(&mut self.p);
        match op {
            LiveOp::ReverbWet(v) => {
                self.reverb.xfade.fade.ramp_to(v, 0.03, c);
                self.rewire();
            }
            LiveOp::ChorusDepth(v) => {
                self.chorus.set_depth(chorus_depth_for(v), c);
                self.chorus.xfade.fade.ramp_to(chorus_wet_for(v), 0.03, c);
                self.rewire();
            }
            LiveOp::FilterCutoff(hz) => {
                if self.filter.lfo.is_none() {
                    self.filter.frequency.ramp_to(hz, 0.02, c);
                }
                self.update_lfo_range(c);
            }
            LiveOp::FilterResonance(q) => self.filter.q.ramp_to(q, 0.02, c),
            LiveOp::FilterType(k) => self.filter.set_kind(k),
            LiveOp::FilterLfoRate(r) => {
                if let Some(lfo) = self.filter.lfo.as_mut() {
                    lfo.frequency.set_value(r, c);
                }
            }
            LiveOp::FilterLfoDepth(d) => {
                if d == 0.0 {
                    self.disconnect_lfo(c);
                } else {
                    self.connect_lfo(c);
                    self.update_lfo_range(c);
                }
            }
            LiveOp::FilterLfoWave(w) => {
                if let Some(lfo) = self.filter.lfo.as_mut() {
                    lfo.set_type(osc_type(w));
                }
            }
            LiveOp::DelayTime(v) => self.delay.delay_time.ramp_to(v, 0.05, c),
            LiveOp::DelayFeedback(v) => self.delay.feedback.ramp_to(v, 0.03, c),
            LiveOp::DelayWet(v) => {
                self.delay.xfade.fade.ramp_to(v, 0.03, c);
                self.rewire();
            }
            LiveOp::DistortionAmount(v) => {
                self.dist.set_amount(v);
                self.rewire();
            }
            LiveOp::DistortionWet(v) => {
                self.dist.xfade.fade.ramp_to(v, 0.03, c);
                self.rewire();
            }
            LiveOp::EqLow(v) => {
                self.eq.low.ramp_to(v, 0.03, c);
                self.rewire();
            }
            LiveOp::EqMid(v) => {
                self.eq.mid.ramp_to(v, 0.03, c);
                self.rewire();
            }
            LiveOp::EqHigh(v) => {
                self.eq.high.ramp_to(v, 0.03, c);
                self.rewire();
            }
            LiveOp::CompThreshold(v) => self.threshold.set_value(v, c),
            LiveOp::CompRatio(v) => {
                self.ratio.set_value(v, c);
                self.rewire();
            }
            LiveOp::CompAttack(v) => self.attack.set_value(v, c),
            LiveOp::CompRelease(v) => self.release.set_value(v, c),
            LiveOp::Volume(db) => self.volume.ramp_to(db, 0.03, c),
            LiveOp::Mute(m) => self.muted = m,
        }
    }

    /// Run one quantum (`frame`) of the instrument's stereo output through the bus, in place.
    pub fn process(&mut self, frame: u64, l: &mut [f32; Q], r: &mut [f32; Q]) {
        let sr = self.sr;
        let st = self.stages;
        // A stage out of the path is not pulled: its state freezes (see the module docs).
        macro_rules! stage {
            ($on:expr, $run:expr) => {
                if $on {
                    $run(&mut *l, &mut *r);
                }
            };
        }
        stage!(st.dist, |a: &mut [f32; Q], b: &mut [f32; Q]| self
            .dist
            .process(frame, a, b));
        self.filter.process(frame, l, r);
        stage!(st.eq, |a: &mut [f32; Q], b: &mut [f32; Q]| self
            .eq
            .process(frame, a, b));
        let p = CompressorParams {
            threshold: self.threshold.k_value(frame, sr),
            knee: self.knee.k_value(frame, sr),
            ratio: self.ratio.k_value(frame, sr),
            attack: self.attack.k_value(frame, sr),
            release: self.release.k_value(frame, sr),
        };
        stage!(st.comp, |a: &mut [f32; Q], b: &mut [f32; Q]| self
            .comp
            .process(a, b, &p));
        if st.chorus {
            let fresh = std::mem::take(&mut self.chorus_fresh);
            self.chorus.process_with(frame, l, r, fresh);
            self.chorus_ran = true;
        }
        if st.delay {
            let fresh = std::mem::take(&mut self.delay_fresh);
            self.delay.process_with(frame, l, r, fresh);
            self.delay_ran = true;
        }
        stage!(st.reverb, |a: &mut [f32; Q], b: &mut [f32; Q]| self
            .reverb
            .process(frame, a, b));
        self.volume.render(frame, sr, &mut self.vv);
        let m = if self.muted { 0.0 } else { 1.0 };
        for k in 0..Q {
            l[k] *= self.vv[k] * m;
            r[k] *= self.vv[k] * m;
        }
    }
}

/// The control side of the live bus: the params `getEffectParams` returns, kept in step with the
/// ops sent to the core.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiveFxState {
    pub params: EffectParams,
    pub volume_db: f64,
    pub muted: bool,
}

impl Default for LiveFxState {
    fn default() -> Self {
        Self {
            params: EffectParams::default(),
            volume_db: LIVE_DEFAULT_VOLUME_DB,
            muted: false,
        }
    }
}

impl LiveFxState {
    /// Record `op` (as `AudioEngine` does before touching its nodes).
    pub fn record(&mut self, op: LiveOp) {
        match op {
            LiveOp::Volume(db) => self.volume_db = db,
            LiveOp::Mute(m) => self.muted = m,
            _ => {
                op.update(&mut self.params);
            }
        }
    }

    /// `getVolume()`.
    pub fn volume(&self) -> f64 {
        if self.volume_db.is_finite() {
            self.volume_db
        } else {
            LIVE_DEFAULT_VOLUME_DB
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    fn run(fx: &mut LiveFx, input: &[f32]) -> (Vec<f32>, Vec<f32>) {
        let (mut ol, mut or) = (Vec::new(), Vec::new());
        for (q, chunk) in input.chunks(Q).enumerate() {
            let mut l = [0f32; Q];
            l[..chunk.len()].copy_from_slice(chunk);
            let mut r = l;
            fx.process((q * Q) as u64, &mut l, &mut r);
            ol.extend_from_slice(&l);
            or.extend_from_slice(&r);
        }
        (ol, or)
    }

    /// AudioEngine.test.ts "keeps the global effect setters working".
    #[test]
    fn every_setter_applies_and_is_recorded() {
        let mut fx = LiveFx::new(&EffectParams::default(), SR, 0.0);
        let mut st = LiveFxState::default();
        let ops = [
            LiveOp::ReverbWet(0.4),
            LiveOp::ChorusDepth(0.5),
            LiveOp::FilterCutoff(1000.0),
            LiveOp::FilterResonance(4.0),
            LiveOp::FilterType(FilterType::Highpass),
            LiveOp::FilterLfoDepth(0.5),
            LiveOp::FilterLfoRate(3.0),
            LiveOp::FilterLfoWave(LfoWaveform::Square),
            LiveOp::FilterLfoDepth(0.0),
            LiveOp::DelayTime(0.3),
            LiveOp::DelayFeedback(0.4),
            LiveOp::DelayWet(0.2),
            LiveOp::DistortionAmount(0.3),
            LiveOp::DistortionWet(0.5),
            LiveOp::EqLow(3.0),
            LiveOp::EqMid(-2.0),
            LiveOp::EqHigh(1.0),
            LiveOp::CompThreshold(-20.0),
            LiveOp::CompRatio(3.0),
            LiveOp::CompAttack(0.01),
            LiveOp::CompRelease(0.2),
            LiveOp::Volume(-6.0),
        ];
        for (i, op) in ops.into_iter().enumerate() {
            fx.apply(op, i as f64 * 0.01);
            st.record(op);
        }
        run(&mut fx, &[0.1; 2048]);
        assert_eq!(fx.params().reverb_wet, 0.4);
        assert_eq!(fx.params().comp_ratio, 3.0);
        assert_eq!(fx.params().filter_type, FilterType::Highpass);
        assert_eq!(*fx.params(), st.params);
        assert_eq!(st.volume(), -6.0);
        assert_eq!(
            fx.stages(),
            Stages {
                dist: true,
                eq: true,
                comp: true,
                chorus: true,
                delay: true,
                reverb: true
            }
        );
    }

    #[test]
    fn stages_follow_the_rewire_rules() {
        let mut p = EffectParams::default();
        assert_eq!(
            Stages::of(&p),
            Stages {
                reverb: true,
                ..Stages::default()
            },
            "defaults: only the reverb (wet 0.12)"
        );
        p.distortion_wet = 0.5;
        assert!(!Stages::of(&p).dist, "amount 0: off");
        p.distortion_amount = 0.2;
        p.comp_ratio = 1.01;
        p.eq_mid = -0.5;
        let s = Stages::of(&p);
        assert!(s.dist && s.eq && !s.comp);
        p.comp_ratio = 1.02;
        assert!(Stages::of(&p).comp);
    }

    #[test]
    fn chorus_mapping() {
        assert_eq!(chorus_depth_for(0.5), 0.3);
        assert_eq!(chorus_depth_for(2.0), 0.6);
        assert_eq!(chorus_wet_for(0.0), 0.0);
        assert!((chorus_wet_for(1.0) - 0.5).abs() < 1e-12);
        assert!((chorus_wet_for(0.2) - 0.22).abs() < 1e-12);
    }

    #[test]
    fn default_bus_is_minus_12_db_with_the_reverb_wet_mixed_in() {
        let p = EffectParams {
            reverb_wet: 0.0,
            ..EffectParams::default()
        };
        let mut fx = LiveFx::new(&p, SR, 0.0);
        let input = vec![0.5f32; 4096];
        let (l, r) = run(&mut fx, &input);
        // the 18 kHz lowpass passes DC: 0.5 · 10^(−12/20)
        let want = 0.5 * 10f64.powf(-12.0 / 20.0);
        assert!((l[4000] as f64 - want).abs() < 1e-4, "{}", l[4000]);
        assert_eq!(l, r);
    }

    #[test]
    fn mute_silences_and_volume_ramps() {
        let mut fx = LiveFx::new(&EffectParams::default(), SR, 0.0);
        fx.apply(LiveOp::Mute(true), 0.0);
        let (l, _) = run(&mut fx, &[0.5; 1024]);
        assert!(l.iter().all(|&x| x == 0.0));
        let mut st = LiveFxState::default();
        st.record(LiveOp::Volume(-6.0));
        st.record(LiveOp::ReverbWet(0.4));
        assert_eq!((st.volume(), st.params.reverb_wet), (-6.0, 0.4));
    }

    #[test]
    fn lfo_off_writes_zero_hz_and_silences_a_lowpass() {
        let p = EffectParams {
            reverb_wet: 0.0,
            filter_cutoff: 2000.0,
            filter_lfo_depth: 0.5,
            ..EffectParams::default()
        };
        let mut fx = LiveFx::new(&p, SR, 0.0);
        let sine: Vec<f32> = (0..9600)
            .map(|i| (i as f32 * 2.0 * std::f32::consts::PI * 300.0 / 48000.0).sin() * 0.5)
            .collect();
        let (on, _) = run(&mut fx, &sine);
        assert!(on[4800..].iter().any(|x| x.abs() > 0.05));
        fx.apply(LiveOp::FilterLfoDepth(0.0), 9600.0 / 48000.0);
        let mut later = LiveFx::new(&p, SR, 0.0);
        std::mem::swap(&mut later, &mut fx);
        let (mut ol, mut or) = (Vec::new(), Vec::new());
        for q in 0..75 {
            let mut l = [0f32; Q];
            for (k, x) in l.iter_mut().enumerate() {
                *x = sine[(q * Q + k) % 9600];
            }
            let mut r = l;
            later.process(((75 + q) * Q) as u64, &mut l, &mut r);
            ol.extend_from_slice(&l);
            or.extend_from_slice(&r);
        }
        let tail = ol[2400..].iter().fold(0f32, |m, x| m.max(x.abs()));
        assert!(tail < 1e-3, "lowpass at 0 Hz: {tail}");
    }
}
