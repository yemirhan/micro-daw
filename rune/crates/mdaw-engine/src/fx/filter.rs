//! `Tone.Filter` (rolloff −12: one biquad per channel) with an optional `Tone.LFO` on its
//! frequency, and `Tone.EQ3` (`MultibandSplit` at 400 / 2500 Hz + three dB gains).

use mdaw_dsp::factory::osc::OscType;
use mdaw_model::types::FilterType;

use super::Q;
use super::biquad::{CoefCache, Coefs, State};
use super::lfo::Lfo;
use super::osc::Tables;
use super::tone::{ToneParam, Units};
use crate::sampler::param::Clock;

/// `Tone.Filter`: frequency and Q are Signals (per-frame biquad coefficients).
#[derive(Clone)]
pub struct ToneFilter {
    pub kind: FilterType,
    pub frequency: ToneParam,
    pub q: ToneParam,
    /// An LFO connected to `frequency` (connecting it overrides the signal for good).
    pub lfo: Option<Lfo>,
    st: [State; 2],
    cache: CoefCache,
    f: [f32; Q],
    qv: [f32; Q],
    lfo_out: [f32; Q],
    sr: f64,
}

impl ToneFilter {
    pub fn new(kind: FilterType, frequency: f64, q: f64, cx: Clock) -> Self {
        Self {
            kind,
            frequency: ToneParam::signal(frequency, Units::Frequency, cx),
            q: ToneParam::signal(q, Units::Linear, cx),
            lfo: None,
            st: [State::default(); 2],
            cache: CoefCache::new(kind, frequency as f32, q as f32, cx.sr),
            f: [0.0; Q],
            qv: [0.0; Q],
            lfo_out: [0.0; Q],
            sr: cx.sr,
        }
    }

    /// `new Tone.LFO({ frequency, type, min, max }).start(); lfo.connect(filter.frequency)`.
    #[allow(clippy::too_many_arguments)]
    pub fn connect_lfo(
        &mut self,
        tables: &Tables,
        shape: OscType,
        rate: f64,
        min: f64,
        max: f64,
        cx: Clock,
    ) {
        self.lfo = Some(Lfo::new(tables.clone(), shape, false, rate, min, max, cx));
        self.frequency.connect_override(cx);
    }

    /// `filter.type = kind`: Chromium's `BiquadFilterHandler::SetType` resets the filter state
    /// when the type changes.
    pub fn set_kind(&mut self, kind: FilterType) {
        if kind != self.kind {
            self.kind = kind;
            self.st = [State::default(); 2];
        }
    }

    /// `lfo.stop(); lfo.dispose()` (the frequency signal stays overridden).
    pub fn dispose_lfo(&mut self) {
        self.lfo = None;
    }

    /// Filter `l` / `r` in place (the quantum at `frame`).
    pub fn process(&mut self, frame: u64, l: &mut [f32; Q], r: &mut [f32; Q]) {
        let sr = self.sr;
        let fv = self.frequency.render(frame, sr, &mut self.f);
        let qv = self.q.render(frame, sr, &mut self.qv);
        let nyq = (0.5 * sr) as f32;
        let mut constant = !fv && !qv;
        if let Some(lfo) = self.lfo.as_mut() {
            lfo.render(frame, sr, &mut self.lfo_out);
            for k in 0..Q {
                self.f[k] += self.lfo_out[k];
            }
            constant = false;
        }
        if constant {
            let c = *self
                .cache
                .get(self.kind, self.f[0].clamp(0.0, nyq), self.qv[0], sr);
            for k in 0..Q {
                l[k] = self.st[0].tick(&c, l[k]);
                r[k] = self.st[1].tick(&c, r[k]);
            }
        } else {
            for k in 0..Q {
                let c = *self
                    .cache
                    .get(self.kind, self.f[k].clamp(0.0, nyq), self.qv[k], sr);
                l[k] = self.st[0].tick(&c, l[k]);
                r[k] = self.st[1].tick(&c, r[k]);
            }
        }
        self.st[0].flush();
        self.st[1].flush();
    }
}

/// `Tone.EQ3` with the default crossover (400 / 2500 Hz, Q 1).
#[derive(Clone)]
pub struct Eq3 {
    pub low: ToneParam,
    pub mid: ToneParam,
    pub high: ToneParam,
    c: [Coefs; 4],
    /// Per channel: low LP, low-mid HP, mid LP, high HP.
    st: [[State; 4]; 2],
    g: [[f32; Q]; 3],
    sr: f64,
}

pub const EQ_LOW_HZ: f32 = 400.0;
pub const EQ_HIGH_HZ: f32 = 2500.0;

impl Eq3 {
    pub fn new(low: f64, mid: f64, high: f64, cx: Clock) -> Self {
        let sr = cx.sr;
        Self {
            low: ToneParam::gain(low, Units::Decibels, cx),
            mid: ToneParam::gain(mid, Units::Decibels, cx),
            high: ToneParam::gain(high, Units::Decibels, cx),
            c: [
                Coefs::compute(FilterType::Lowpass, EQ_LOW_HZ, 1.0, sr),
                Coefs::compute(FilterType::Highpass, EQ_LOW_HZ, 1.0, sr),
                Coefs::compute(FilterType::Lowpass, EQ_HIGH_HZ, 1.0, sr),
                Coefs::compute(FilterType::Highpass, EQ_HIGH_HZ, 1.0, sr),
            ],
            st: [[State::default(); 4]; 2],
            g: [[0.0; Q]; 3],
            sr,
        }
    }

    /// The crossover's BiquadFilterNodes are re-initialized (see [`super::live`]).
    pub fn reset_state(&mut self) {
        self.st = [[State::default(); 4]; 2];
    }

    pub fn process(&mut self, frame: u64, l: &mut [f32; Q], r: &mut [f32; Q]) {
        let sr = self.sr;
        self.low.render(frame, sr, &mut self.g[0]);
        self.mid.render(frame, sr, &mut self.g[1]);
        self.high.render(frame, sr, &mut self.g[2]);
        for (ch, io) in [l, r].into_iter().enumerate() {
            let st = &mut self.st[ch];
            for (k, x) in io.iter_mut().enumerate() {
                let lo = st[0].tick(&self.c[0], *x);
                let lm = st[1].tick(&self.c[1], *x);
                let mid = st[2].tick(&self.c[2], lm);
                let hi = st[3].tick(&self.c[3], *x);
                *x = lo * self.g[0][k] + mid * self.g[1][k] + hi * self.g[2][k];
            }
            for s in st.iter_mut() {
                s.flush();
            }
        }
    }
}
