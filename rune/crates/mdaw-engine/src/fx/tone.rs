//! Tone.js `Param` / `Signal` semantics on top of the realtime AudioParam timeline
//! ([`crate::sampler::param::AutoParam`]: Tone's JS timeline + Chromium's native one).
//!
//! What this layer adds:
//! - **units**: `decibels` params (a `Gain` with `units: "decibels"`: EQ3 bands, `Volume`) store
//!   `dbToGain(v)` for every event, so `rampTo` / `linearRampToValueAtTime` in dB are ramps in
//!   the GAIN domain (`rampTo` on dB / frequency units is `exponentialRampTo`, linear otherwise);
//! - **override**: `connectSignal` (an LFO connected to `Filter.frequency`) sets the signal's
//!   `overridden` flag, after which `_fromType` maps every value to 0. Tone never clears it; the
//!   app clears it when it removes the LFO (`removeFilterLfo` / `AudioEngine.disconnectLfo`,
//!   [`ToneParam::clear_override`]) so the static cutoff written next is kept (Electron bug fixed
//!   in both apps, user decision 2026-11-18).
//! - **now**: Tone's `param.value = v` is `cancelScheduledValues(now)` + `setValueAtTime(v, now)`
//!   with `now = currentTime + lookAhead`. The Rust engine has no look-ahead (phase-3
//!   deviation): `now` is the command's frame time.

use crate::sampler::param::{AutoParam, Clock};

/// `Tone.Param` units that change the arithmetic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Units {
    /// Stored as given; `rampTo` is linear (`number`, `normalRange`, `positive`, `time`, `gain`).
    Linear,
    /// Stored as given; `rampTo` is exponential.
    Frequency,
    /// Stored as `dbToGain(v)`; `rampTo` is exponential (gain domain).
    Decibels,
    /// `convert: false` decibels (the compressor's threshold / knee): stored as given, `rampTo`
    /// exponential.
    RawDecibels,
}

/// Tone's `dbToGain`.
pub fn db_to_gain(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

/// Tone's `gainToDb`.
pub fn gain_to_db(gain: f64) -> f64 {
    20.0 * (gain.ln() / std::f64::consts::LN_10)
}

const MIN_OUTPUT: f64 = 1e-7;

/// One Tone `Param` / `Signal` (see the module docs).
#[derive(Debug, Clone)]
pub struct ToneParam {
    p: AutoParam,
    units: Units,
    overridden: bool,
}

impl ToneParam {
    /// `new Tone.Param({ value })` over a native param (`native_default`, nominal range).
    pub fn new(
        native_default: f64,
        value: f64,
        units: Units,
        range: (f32, f32),
        cx: Clock,
    ) -> Self {
        let mut s = Self {
            p: AutoParam::new(native_default, native_default, range.0, range.1, cx),
            units,
            overridden: false,
        };
        // Tone: `if (value !== this._toType(this._initialValue)) this.setValueAtTime(value, 0)`.
        if value != s.to_type(native_default) {
            s.set_value_at_time(value, 0.0, cx);
        }
        s
    }

    /// A `Tone.Signal` (a ConstantSourceNode offset: default 0, unbounded).
    pub fn signal(value: f64, units: Units, cx: Clock) -> Self {
        Self::new(0.0, value, units, (f32::MIN, f32::MAX), cx)
    }

    /// A `Tone.Gain` gain param (default 1).
    pub fn gain(value: f64, units: Units, cx: Clock) -> Self {
        Self::new(1.0, value, units, (f32::MIN, f32::MAX), cx)
    }

    fn to_native(&self, v: f64) -> f64 {
        if self.overridden {
            0.0
        } else if self.units == Units::Decibels {
            db_to_gain(v)
        } else {
            v
        }
    }

    fn to_type(&self, v: f64) -> f64 {
        if self.units == Units::Decibels {
            gain_to_db(v)
        } else {
            v
        }
    }

    /// `connectSignal(src, this)`: cancel everything, 0 from time 0, overridden for good.
    pub fn connect_override(&mut self, cx: Clock) {
        self.cancel_scheduled_values(0.0, cx);
        self.set_value_at_time(0.0, 0.0, cx);
        self.overridden = true;
    }

    /// `param.overridden = false` (the app, after disposing the LFO): later writes store their
    /// value again.
    pub fn clear_override(&mut self) {
        self.overridden = false;
    }

    pub fn is_overridden(&self) -> bool {
        self.overridden
    }

    /// `param.value` (`getValueAtTime(now)`, in Tone units).
    pub fn value_at(&self, t: f64) -> f64 {
        self.to_type(self.p.get_value_at_time(t))
    }

    pub fn set_value_at_time(&mut self, v: f64, t: f64, cx: Clock) {
        let n = self.to_native(v);
        self.p.set_value_at_time(n, t, cx);
    }

    pub fn linear_ramp_to_value_at_time(&mut self, v: f64, t: f64, cx: Clock) {
        let n = self.to_native(v);
        self.p.linear_ramp_to_value_at_time(n, t, cx);
    }

    fn exponential_ramp_to_value_at_time(&mut self, v: f64, t: f64, cx: Clock) {
        let n = self.to_native(v);
        self.p.exponential_ramp_to_value_at_time(n, t, cx);
    }

    pub fn cancel_scheduled_values(&mut self, t: f64, cx: Clock) {
        self.p.cancel_scheduled_values(t, cx);
    }

    /// `param.value = v`.
    pub fn set_value(&mut self, v: f64, cx: Clock) {
        self.cancel_scheduled_values(cx.now, cx);
        self.set_value_at_time(v, cx.now, cx);
    }

    /// `Param.setRampPoint(now)`.
    fn set_ramp_point(&mut self, cx: Clock) {
        let t = cx.now;
        let mut current = self.value_at(t);
        self.p.cancel_and_hold_at_time(t, cx);
        if self.to_native(current) == 0.0 {
            current = self.to_type(MIN_OUTPUT);
        }
        self.set_value_at_time(current, t, cx);
    }

    /// `param.rampTo(v, ramp)` from now (exponential for dB / frequency units).
    pub fn ramp_to(&mut self, v: f64, ramp: f64, cx: Clock) {
        self.set_ramp_point(cx);
        match self.units {
            Units::Linear => self.linear_ramp_to_value_at_time(v, cx.now + ramp, cx),
            _ => self.exponential_ramp_to_value_at_time(v, cx.now + ramp, cx),
        }
    }

    /// One quantum of values (native units). Returns whether they vary within the quantum.
    pub fn render(&mut self, frame: u64, sr: f64, out: &mut [f32]) -> bool {
        self.p.prune_tone(frame as f64 / sr);
        self.p.render(frame, sr, out)
    }

    /// The k-rate value of the quantum starting at `frame` (`FinalValue()`).
    pub fn k_value(&mut self, frame: u64, sr: f64) -> f32 {
        let mut one = [0f32; 1];
        self.render(frame, sr, &mut one);
        one[0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 48000.0;
    fn cx(now: f64) -> Clock {
        Clock { now, sr: SR }
    }

    #[test]
    fn decibel_params_store_gains_and_ramp_in_the_gain_domain() {
        let mut g = ToneParam::gain(-6.0, Units::Decibels, cx(0.0));
        let mut q = [0f32; 128];
        g.render(0, SR, &mut q);
        assert_eq!(q[5], db_to_gain(-6.0) as f32);
        g.set_value_at_time(-40.0, 128.0 / SR, cx(0.0));
        g.linear_ramp_to_value_at_time(0.0, 256.0 / SR, cx(0.0));
        g.render(128, SR, &mut q);
        let (a, b) = (db_to_gain(-40.0), 1.0);
        assert!(
            (q[64] as f64 - (a + (b - a) * 0.5)).abs() < 1e-6,
            "{}",
            q[64]
        );
        assert!((g.value_at(0.0) + 6.0).abs() < 1e-9);
    }

    #[test]
    fn an_overridden_signal_maps_every_value_to_zero() {
        let mut f = ToneParam::signal(1000.0, Units::Frequency, cx(0.0));
        f.connect_override(cx(0.0));
        f.set_value(5000.0, cx(0.01));
        f.linear_ramp_to_value_at_time(3000.0, 0.015, cx(0.01));
        let mut q = [1f32; 128];
        for k in 0..20 {
            f.render(k * 128, SR, &mut q);
            assert!(q.iter().all(|&x| x == 0.0));
        }
        // `rampTo` is exponential: Tone replaces its 0 target (and ramp point) by 1e-7.
        f.ramp_to(2000.0, 0.02, cx(0.02));
        for k in 20..40 {
            f.render(k * 128, SR, &mut q);
            assert!(q.iter().all(|&x| (0.0..=1.0e-7).contains(&x)), "{q:?}");
        }
    }

    #[test]
    fn frequency_ramp_to_is_exponential() {
        let mut f = ToneParam::signal(1000.0, Units::Frequency, cx(0.0));
        f.ramp_to(4000.0, 0.02, cx(0.0));
        assert!((f.value_at(0.01) - 2000.0).abs() < 1e-6);
        let mut l = ToneParam::signal(0.0, Units::Linear, cx(0.0));
        l.ramp_to(1.0, 0.02, cx(0.0));
        assert!((l.value_at(0.01) - 0.5).abs() < 1e-6);
    }
}
