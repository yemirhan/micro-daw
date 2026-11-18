//! The per-track strip of `createTrackAudioNodes` (src/utils/trackAudioFactory.ts):
//!
//! ```text
//! instrument → Filter → Distortion(2×) → EQ3 → [Compressor] → Chorus → FeedbackDelay → RoomReverb
//!            → Panner (mono down-mix, equal power) → Volume(dB) → Meter
//! ```
//!
//! Live parameter changes follow `ArrangementEngine.applyTrackEffect` / `resetTrackEffects` and
//! automation playback (`scheduleAutomation`), as [`StripOp`]s applied at a frame. Kept TS quirks:
//! - `resetTrackEffects` (every play start) uses `delay.time || 0.25`, `delay.feedback || 0.3`;
//! - an LFO that already exists is not re-ranged by `resetTrackEffects` (its rate / wave / range
//!   stay until `applyTrackEffect('filter')`);
//! - removing the LFO (`removeFilterLfo`) clears the override its connection set on
//!   `filter.frequency` and writes the static cutoff (Electron bug fixed in both apps, user
//!   decision 2026-11-18: Tone never clears the flag, so the TS used to write 0 Hz and silence a
//!   lowpass / bandpass track);
//! - a disabled effect still leaks its wet path at −56 dB (`fx/xfade.rs`).
//!
//! The compressor is in the path only while `compressorActive` (review round 2 #10). Bypassed,
//! its input is disconnected but its output still feeds the chorus, so Chromium keeps processing
//! it on silence (its detector relaxes; measured: modelling it as frozen put the next enable 7 dB
//! off, `fx_chains.json` `strip_apply`).

use mdaw_dsp::reverb_ir::REVERB_DEFAULTS;
use mdaw_model::types::AutomationParameter;

use super::Q;
use super::compressor::{Compressor, CompressorParams};
use super::effects::{Chorus, Distortion, FeedbackDelay, RoomReverb};
use super::filter::{Eq3, ToneFilter};
use super::osc::{Tables, osc_type};
use super::state::{FxState, compressor_active, lfo_range, or_default};
use super::tone::{ToneParam, Units};
use crate::sampler::param::Clock;

/// Which effect `applyTrackEffect` updates (the doc's effect names).
pub use mdaw_model::doc::EffectName as Effect;

/// A change to a strip, applied at a frame (time `t`, the "now" of Tone's `.value =`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StripOp {
    /// `applyTrackEffect(audio, effects, name)`.
    Apply(Effect, FxState),
    /// `resetTrackEffects(track, audio)` with the track's pan and volume (dB).
    Reset {
        fx: FxState,
        pan: f64,
        volume_db: f64,
    },
    /// `audio.panner.pan.value = pan`.
    Pan(f64),
    /// `audio.volume.volume.value = db`.
    Volume(f64),
    /// One automation point (`scheduleAutomation`'s transport callback at `t`): set `value`
    /// (real units) at `t`, then ramp linearly to `ramp.0` until `t + ramp.1` seconds.
    Automation {
        param: AutomationParameter,
        value: f64,
        ramp: Option<(f64, f64)>,
    },
    /// The automation pre-pass: `distortion.distortion = max(amount, 0.4)` for a distortionWet
    /// lane, default time / feedback (`|| 0.25`, `|| 0.3`) for a delayWet lane.
    ForceDistortionAmount(f64),
    ForceDelayTimes {
        time: f64,
        feedback: f64,
    },
}

/// See the module docs.
pub struct TrackStrip {
    sr: f64,
    tables: Tables,
    filter: ToneFilter,
    dist: Distortion,
    eq: Eq3,
    comp: Compressor,
    threshold: ToneParam,
    ratio: ToneParam,
    attack: ToneParam,
    release: ToneParam,
    knee: ToneParam,
    comp_inline: bool,
    chorus: Chorus,
    delay: FeedbackDelay,
    reverb: RoomReverb,
    pan: ToneParam,
    volume: ToneParam,
    pv: [f32; Q],
    vv: [f32; Q],
}

/// `clock` for a strip op applied at `t`.
fn cx(t: f64, sr: f64) -> Clock {
    Clock { now: t, sr }
}

impl TrackStrip {
    /// `createTrackAudioNodes(track)` (control side: allocates the delay lines and the reverb).
    /// `t`: the context time it is built at (its LFOs start there).
    pub fn new(fx: &FxState, pan: f64, volume_db: f64, sample_rate: u32, t: f64) -> Self {
        let sr = sample_rate as f64;
        let c = cx(t, sr);
        let tables = Tables::new(sample_rate);
        let mut filter = ToneFilter::new(
            fx.filter_type,
            if fx.filter_on { fx.cutoff } else { 18000.0 },
            if fx.filter_on { fx.resonance } else { 1.0 },
            c,
        );
        if fx.filter_on && fx.lfo_depth > 0.0 {
            let (min, max) = lfo_range(fx.cutoff, fx.lfo_depth);
            filter.connect_lfo(&tables, osc_type(fx.lfo_wave), fx.lfo_rate, min, max, c);
        }
        let cp = |native: f64, v: f64, units, range| ToneParam::new(native, v, units, range, c);
        Self {
            sr,
            filter,
            dist: Distortion::new(
                if fx.dist_on { fx.dist_amount } else { 0.0 },
                if fx.dist_on { fx.dist_wet } else { 0.0 },
                c,
            ),
            eq: {
                let g = |v: f64| if fx.eq_on { v } else { 0.0 };
                Eq3::new(g(fx.eq[0]), g(fx.eq[1]), g(fx.eq[2]), c)
            },
            comp: Compressor::new(sample_rate as f32),
            threshold: cp(
                -24.0,
                if fx.comp_on { fx.comp_threshold } else { 0.0 },
                Units::RawDecibels,
                (-100.0, 0.0),
            ),
            ratio: cp(
                12.0,
                if fx.comp_on { fx.comp_ratio } else { 1.0 },
                Units::Linear,
                (1.0, 20.0),
            ),
            attack: cp(0.003, fx.comp_attack, Units::Linear, (0.0, 1.0)),
            release: cp(0.25, fx.comp_release, Units::Linear, (0.0, 1.0)),
            knee: cp(30.0, 30.0, Units::RawDecibels, (0.0, 40.0)),
            comp_inline: compressor_active(fx.comp_on, fx.comp_ratio),
            chorus: Chorus::new(
                &tables,
                0.7,
                3.5,
                if fx.chorus_on { fx.chorus_depth } else { 0.0 },
                if fx.chorus_on { 0.5 } else { 0.0 },
                c,
            ),
            delay: FeedbackDelay::new(
                fx.delay_time,
                fx.delay_feedback,
                if fx.delay_on { fx.delay_wet } else { 0.0 },
                c,
            ),
            reverb: {
                let mut o = REVERB_DEFAULTS;
                o.wet = if fx.reverb_on { fx.reverb_wet } else { 0.0 };
                RoomReverb::new(&o, c)
            },
            pan: ToneParam::new(0.0, pan, Units::Linear, (-1.0, 1.0), c),
            volume: ToneParam::gain(volume_db, Units::Decibels, c),
            pv: [0.0; Q],
            vv: [0.0; Q],
            tables,
        }
    }

    /// Whether the compressor is in the signal path.
    pub fn compressor_inline(&self) -> bool {
        self.comp_inline
    }

    /// `setTrackCompressorEnabled(audio, enabled)` (idempotent).
    fn set_compressor_inline(&mut self, on: bool) {
        self.comp_inline = on;
    }

    /// Apply `op` at context time `t`.
    pub fn apply(&mut self, op: StripOp, t: f64) {
        let c = cx(t, self.sr);
        match op {
            StripOp::Apply(e, fx) => self.apply_effect(e, &fx, c),
            StripOp::Reset { fx, pan, volume_db } => self.reset(&fx, pan, volume_db, c),
            StripOp::Pan(p) => self.pan.set_value(p, c),
            StripOp::Volume(db) => self.volume.set_value(db, c),
            StripOp::Automation { param, value, ramp } => self.automate(param, value, ramp, c),
            StripOp::ForceDistortionAmount(a) => self.dist.set_amount(a),
            StripOp::ForceDelayTimes { time, feedback } => {
                self.delay.delay_time.set_value(time, c);
                self.delay.feedback.set_value(feedback, c);
            }
        }
    }

    /// `applyTrackEffect`.
    fn apply_effect(&mut self, e: Effect, fx: &FxState, c: Clock) {
        match e {
            Effect::Reverb => {
                let w = if fx.reverb_on { fx.reverb_wet } else { 0.0 };
                self.reverb.xfade.fade.set_value(w, c);
            }
            Effect::Delay => {
                let w = if fx.delay_on { fx.delay_wet } else { 0.0 };
                self.delay.xfade.fade.set_value(w, c);
                self.delay.delay_time.set_value(fx.delay_time, c);
                self.delay.feedback.set_value(fx.delay_feedback, c);
            }
            Effect::Chorus => self.set_chorus(fx, c),
            Effect::Distortion => self.set_distortion(fx, c),
            Effect::Eq => self.set_eq(fx, c),
            Effect::Compressor => {
                self.set_compressor_levels(fx, c);
                self.attack.set_value(fx.comp_attack, c);
                self.release.set_value(fx.comp_release, c);
                self.set_compressor_inline(compressor_active(fx.comp_on, fx.comp_ratio));
            }
            Effect::Filter => {
                self.set_filter_static(fx, c);
                let depth = if fx.filter_on { fx.lfo_depth } else { 0.0 };
                if depth > 0.0 {
                    let (min, max) = lfo_range(fx.cutoff, depth);
                    if let Some(lfo) = self.filter.lfo.as_mut() {
                        lfo.frequency.set_value(fx.lfo_rate, c);
                        lfo.set_type(osc_type(fx.lfo_wave));
                        lfo.set_min(min, c);
                        lfo.set_max(max, c);
                    } else {
                        let shape = osc_type(fx.lfo_wave);
                        self.filter
                            .connect_lfo(&self.tables, shape, fx.lfo_rate, min, max, c);
                    }
                } else {
                    let f = if fx.filter_on { fx.cutoff } else { 18000.0 };
                    self.filter.remove_lfo(f, c);
                }
            }
        }
    }

    fn set_chorus(&mut self, fx: &FxState, c: Clock) {
        self.chorus
            .set_depth(if fx.chorus_on { fx.chorus_depth } else { 0.0 }, c);
        self.chorus
            .xfade
            .fade
            .set_value(if fx.chorus_on { 0.5 } else { 0.0 }, c);
    }

    fn set_distortion(&mut self, fx: &FxState, c: Clock) {
        self.dist
            .set_amount(if fx.dist_on { fx.dist_amount } else { 0.0 });
        self.dist
            .xfade
            .fade
            .set_value(if fx.dist_on { fx.dist_wet } else { 0.0 }, c);
    }

    fn set_eq(&mut self, fx: &FxState, c: Clock) {
        let g = |v: f64| if fx.eq_on { v } else { 0.0 };
        self.eq.low.set_value(g(fx.eq[0]), c);
        self.eq.mid.set_value(g(fx.eq[1]), c);
        self.eq.high.set_value(g(fx.eq[2]), c);
    }

    fn set_compressor_levels(&mut self, fx: &FxState, c: Clock) {
        let th = if fx.comp_on { fx.comp_threshold } else { 0.0 };
        self.threshold.set_value(th, c);
        self.ratio
            .set_value(if fx.comp_on { fx.comp_ratio } else { 1.0 }, c);
    }

    fn set_filter_static(&mut self, fx: &FxState, c: Clock) {
        let f = if fx.filter_on { fx.cutoff } else { 18000.0 };
        self.filter.frequency.set_value(f, c);
        self.filter
            .q
            .set_value(if fx.filter_on { fx.resonance } else { 1.0 }, c);
        self.filter.set_kind(fx.filter_type);
    }

    /// `resetTrackEffects`.
    fn reset(&mut self, fx: &FxState, pan: f64, volume_db: f64, c: Clock) {
        for p in [
            &mut self.dist.xfade.fade,
            &mut self.reverb.xfade.fade,
            &mut self.delay.xfade.fade,
            &mut self.chorus.xfade.fade,
            &mut self.filter.frequency,
            &mut self.filter.q,
            &mut self.eq.low,
            &mut self.eq.mid,
            &mut self.eq.high,
            &mut self.pan,
            &mut self.volume,
            &mut self.threshold,
            &mut self.ratio,
        ] {
            p.cancel_scheduled_values(0.0, c);
        }
        self.set_distortion(fx, c);
        self.reverb
            .xfade
            .fade
            .set_value(if fx.reverb_on { fx.reverb_wet } else { 0.0 }, c);
        self.delay
            .delay_time
            .set_value(or_default(fx.delay_time, 0.25), c);
        self.delay
            .feedback
            .set_value(or_default(fx.delay_feedback, 0.3), c);
        self.delay
            .xfade
            .fade
            .set_value(if fx.delay_on { fx.delay_wet } else { 0.0 }, c);
        self.set_chorus(fx, c);
        self.set_filter_static(fx, c);
        let depth = if fx.filter_on { fx.lfo_depth } else { 0.0 };
        if depth > 0.0 {
            if self.filter.lfo.is_none() {
                let (min, max) = lfo_range(fx.cutoff, depth);
                let shape = osc_type(fx.lfo_wave);
                self.filter
                    .connect_lfo(&self.tables, shape, fx.lfo_rate, min, max, c);
            }
        } else {
            let f = if fx.filter_on { fx.cutoff } else { 18000.0 };
            self.filter.remove_lfo(f, c);
        }
        self.set_eq(fx, c);
        self.set_compressor_levels(fx, c);
        self.set_compressor_inline(compressor_active(fx.comp_on, fx.comp_ratio));
        self.pan.set_value(pan, c);
        self.volume.set_value(volume_db, c);
    }

    /// One automation callback (`scheduleAutomation`).
    fn automate(&mut self, param: AutomationParameter, v: f64, ramp: Option<(f64, f64)>, c: Clock) {
        use AutomationParameter as A;
        let t = c.now;
        let p = match param {
            A::Volume => &mut self.volume,
            A::Pan => &mut self.pan,
            A::ReverbWet => &mut self.reverb.xfade.fade,
            A::DelayWet => &mut self.delay.xfade.fade,
            A::DistortionWet => &mut self.dist.xfade.fade,
            A::FilterCutoff => &mut self.filter.frequency,
            A::EqLow => &mut self.eq.low,
            A::EqMid => &mut self.eq.mid,
            A::EqHigh => &mut self.eq.high,
            A::ChorusDepth => {
                // Applied at once in the callback (TS quirk: not at the event time).
                self.chorus.set_depth(v, c);
                self.chorus
                    .xfade
                    .fade
                    .set_value(if v > 0.0 { 0.5 } else { 0.0 }, c);
                return;
            }
        };
        p.set_value_at_time(v, t, c);
        if let Some((to, dur)) = ramp {
            p.linear_ramp_to_value_at_time(to, t + dur, c);
        }
    }

    /// Run one quantum (`frame`) of the instrument's stereo output through the strip, in place.
    pub fn process(&mut self, frame: u64, l: &mut [f32; Q], r: &mut [f32; Q]) {
        let sr = self.sr;
        self.filter.process(frame, l, r);
        self.dist.process(frame, l, r);
        self.eq.process(frame, l, r);
        let p = CompressorParams {
            threshold: self.threshold.k_value(frame, sr),
            knee: self.knee.k_value(frame, sr),
            ratio: self.ratio.k_value(frame, sr),
            attack: self.attack.k_value(frame, sr),
            release: self.release.k_value(frame, sr),
        };
        if self.comp_inline {
            self.comp.process(l, r, &p);
        } else {
            // bypassed: silent input (it keeps running, as Chromium does); its delayed output
            // still reaches the chorus
            let mut zl = [0f32; Q];
            let mut zr = [0f32; Q];
            self.comp.process(&mut zl, &mut zr, &p);
            for k in 0..Q {
                l[k] += zl[k];
                r[k] += zr[k];
            }
        }
        self.chorus.process(frame, l, r);
        self.delay.process(frame, l, r);
        self.reverb.process(frame, l, r);
        // Panner (channelCount 1, explicit): mono down-mix, equal power.
        let pv = self.pan.render(frame, sr, &mut self.pv);
        self.volume.render(frame, sr, &mut self.vv);
        let mut gains = pan_gains(self.pv[0]);
        for k in 0..Q {
            if pv {
                gains = pan_gains(self.pv[k]);
            }
            let mono = (l[k] + r[k]) * 0.5;
            l[k] = (mono as f64 * gains.0) as f32 * self.vv[k];
            r[k] = (mono as f64 * gains.1) as f32 * self.vv[k];
        }
    }
}

/// Chromium `StereoPanner` gains for a mono input.
#[inline]
pub fn pan_gains(pan: f32) -> (f64, f64) {
    let p = (pan as f64).clamp(-1.0, 1.0);
    let rad = (p * 0.5 + 0.5) * std::f64::consts::FRAC_PI_2;
    (rad.cos(), rad.sin())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    fn on(mut f: FxState) -> FxState {
        f.comp_on = true;
        f.comp_ratio = 4.0;
        f
    }

    /// ArrangementEngine.beat.test.ts "#10 setTrackEffect(compressor) puts the compressor in the
    /// path only while enabled".
    #[test]
    fn the_compressor_is_in_the_path_only_while_enabled_with_a_ratio_above_one() {
        let mut s = TrackStrip::new(&FxState::default(), 0.0, 0.0, SR, 0.0);
        assert!(!s.compressor_inline());
        s.apply(
            StripOp::Apply(Effect::Compressor, on(FxState::default())),
            0.1,
        );
        assert!(s.compressor_inline());
        let mut off = on(FxState::default());
        off.comp_on = false;
        s.apply(StripOp::Apply(Effect::Compressor, off), 0.2);
        assert!(!s.compressor_inline());
        let mut one = on(FxState::default());
        one.comp_ratio = 1.0;
        let reset = StripOp::Reset {
            fx: one,
            pan: 0.0,
            volume_db: 0.0,
        };
        s.apply(reset, 0.3);
        assert!(!s.compressor_inline(), "ratio 1: bypassed");
    }

    /// `resetTrackEffects`: `delay.time || 0.25`, `delay.feedback || 0.3` (kept TS quirk).
    #[test]
    fn reset_turns_a_zero_delay_feedback_into_0_3() {
        let fx = FxState {
            delay_feedback: 0.0,
            delay_time: 0.0,
            ..FxState::default()
        };
        let mut s = TrackStrip::new(&fx, 0.0, 0.0, SR, 0.0);
        assert_eq!(
            s.delay.feedback.value_at(0.0),
            0.0,
            "built with the value as is"
        );
        let reset = StripOp::Reset {
            fx,
            pan: 0.0,
            volume_db: 0.0,
        };
        s.apply(reset, 0.5);
        assert!((s.delay.feedback.value_at(0.5) - 0.3).abs() < 1e-12);
        assert!((s.delay.delay_time.value_at(0.5) - 0.25).abs() < 1e-12);
    }

    /// Removing the LFO clears the override its connection set and restores the static cutoff
    /// (Electron bug fixed in both apps, user decision 2026-11-18), on apply and on reset.
    #[test]
    fn removing_the_lfo_restores_the_static_cutoff() {
        let mut fx = FxState {
            filter_on: true,
            cutoff: 1000.0,
            lfo_depth: 0.5,
            ..FxState::default()
        };
        let mut s = TrackStrip::new(&fx, 0.0, 0.0, SR, 0.0);
        assert!(s.filter.lfo.is_some());
        fx.lfo_depth = 0.0;
        s.apply(StripOp::Apply(Effect::Filter, fx), 0.1);
        assert!(s.filter.lfo.is_none());
        assert!(!s.filter.frequency.is_overridden());
        assert_eq!(s.filter.frequency.value_at(0.2), 1000.0);
        // via a play-start reset
        fx.lfo_depth = 0.5;
        let mut r = TrackStrip::new(&fx, 0.0, 0.0, SR, 0.0);
        fx.lfo_depth = 0.0;
        let reset = StripOp::Reset {
            fx,
            pan: 0.0,
            volume_db: 0.0,
        };
        r.apply(reset, 0.1);
        assert!(r.filter.lfo.is_none());
        assert_eq!(r.filter.frequency.value_at(0.2), 1000.0);
        // a track that never had an LFO keeps its cutoff
        let plain = FxState {
            filter_on: true,
            cutoff: 1000.0,
            ..FxState::default()
        };
        let mut p = TrackStrip::new(&plain, 0.0, 0.0, SR, 0.0);
        p.apply(StripOp::Apply(Effect::Filter, plain), 0.1);
        assert_eq!(p.filter.frequency.value_at(0.2), 1000.0);
    }

    #[test]
    fn chorus_depth_automation_applies_at_once_and_others_ramp() {
        let mut s = TrackStrip::new(&FxState::default(), 0.0, 0.0, SR, 0.0);
        let op = StripOp::Automation {
            param: AutomationParameter::ChorusDepth,
            value: 0.8,
            ramp: Some((0.0, 1.0)),
        };
        s.apply(op, 0.25);
        assert_eq!(s.chorus.depth(), 0.8);
        assert_eq!(s.chorus.xfade.fade.value_at(0.25), 0.5);
        let op = StripOp::Automation {
            param: AutomationParameter::ReverbWet,
            value: 0.2,
            ramp: Some((1.0, 0.5)),
        };
        s.apply(op, 1.0);
        assert!((s.reverb.xfade.fade.value_at(1.25) - 0.6).abs() < 1e-9);
    }
}
