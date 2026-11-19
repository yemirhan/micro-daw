//! `Tone.LFO`: `Oscillator → Gain(amplitude 1) → AudioToGain → Scale(min, max)`, i.e. per frame
//! `out = a2g(osc)·mult + add` where `a2g` is the 1024-point WaveShaper `x ↦ (x + 1)/2`, `mult`
//! the `Multiply` gain (`max − min`) and `add` the `Add` signal (`min`), both set with
//! `.value =` by the `min` / `max` setters (sample-accurate steps at "now").
//!
//! The LFO is started where it is built (the app calls `.start()` right away) and its output is
//! summed into the param it is connected to.

use std::sync::LazyLock;

use mdaw_dsp::factory::osc::OscType;
use mdaw_dsp::shaping::{tone_curve, waveshape};

use super::Q;
use super::osc::{Osc, Tables};
use super::tone::{ToneParam, Units};
use crate::sampler::param::Clock;

static A2G: LazyLock<Vec<f32>> = LazyLock::new(|| tone_curve(1024, |x| (x + 1.0) / 2.0));

/// The AudioToGain curve (resolve on the control side: the first use allocates).
pub fn a2g_curve() -> &'static [f32] {
    &A2G
}

#[derive(Clone)]
pub struct Lfo {
    osc: Osc,
    pub frequency: ToneParam,
    mult: ToneParam,
    add: ToneParam,
    min: f64,
    max: f64,
    a2g: &'static [f32],
    f: [f32; Q],
    m: [f32; Q],
    a: [f32; Q],
}

impl Lfo {
    /// `new Tone.LFO({ frequency, type, min, max, phase }).start()` (phase 0 or 180: `flip`).
    pub fn new(
        tables: Tables,
        shape: OscType,
        flip: bool,
        frequency: f64,
        min: f64,
        max: f64,
        cx: Clock,
    ) -> Self {
        Self::with_osc(
            Osc::new(tables, shape, flip, cx.sr),
            frequency,
            min,
            max,
            cx,
        )
    }

    /// An LFO around a given oscillator (a custom phase: `Osc::with_wave`).
    pub fn with_osc(osc: Osc, frequency: f64, min: f64, max: f64, cx: Clock) -> Self {
        Self {
            osc,
            frequency: ToneParam::signal(frequency, Units::Frequency, cx),
            // Scale(min 0, max 1) → then the LFO constructor's `min = …; max = …`.
            mult: {
                let mut p = ToneParam::gain(1.0, Units::Linear, cx);
                p.set_value_at_time(1.0, 0.0, cx);
                p.set_value(max - min, cx);
                p
            },
            add: {
                let mut p = ToneParam::signal(0.0, Units::Linear, cx);
                p.set_value(min, cx);
                p
            },
            min,
            max,
            a2g: a2g_curve(),
            f: [0.0; Q],
            m: [0.0; Q],
            a: [0.0; Q],
        }
    }

    /// `lfo.min = v` (Scale._setRange: add then mult).
    pub fn set_min(&mut self, v: f64, cx: Clock) {
        self.min = v;
        self.set_range(cx);
    }

    /// `lfo.max = v`.
    pub fn set_max(&mut self, v: f64, cx: Clock) {
        self.max = v;
        self.set_range(cx);
    }

    fn set_range(&mut self, cx: Clock) {
        self.add.set_value(self.min, cx);
        self.mult.set_value(self.max - self.min, cx);
    }

    pub fn set_type(&mut self, shape: OscType) {
        self.osc.set_type(shape);
    }

    /// The LFO's output for the quantum at `frame`.
    pub fn render(&mut self, frame: u64, sr: f64, out: &mut [f32; Q]) {
        self.frequency.render(frame, sr, &mut self.f);
        self.mult.render(frame, sr, &mut self.m);
        self.add.render(frame, sr, &mut self.a);
        for (k, o) in out.iter_mut().enumerate() {
            let x = waveshape(self.a2g, self.osc.tick(self.f[k]));
            *o = x * self.m[k] + self.a[k];
        }
    }
}
