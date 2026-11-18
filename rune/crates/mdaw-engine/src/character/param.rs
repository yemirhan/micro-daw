//! A Tone `Signal` / `Param` driven only by "set now" and `rampTo` (what CharacterChain does),
//! evaluated per frame without allocating.
//!
//! `rampTo(v, d)` at `now` = `setRampPoint(now)` (hold Tone's value at `now`) + a linear or
//! exponential ramp ending at `now + d`. Chromium evaluates the ramp per frame at `t = k / sr`
//! from the f32 event values; an exponential ramp whose ends have opposite signs (or start at
//! 0) holds its start value until the end (Web Audio spec). Tone replaces a 0 end of an
//! exponential ramp (and a 0 hold point) by 1e-7.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Curve {
    Linear,
    Exponential,
}

#[derive(Debug, Clone, Copy)]
pub struct Automated {
    v0: f64,
    v1: f64,
    t0: f64,
    t1: f64,
    curve: Curve,
}

const MIN_OUTPUT: f64 = 1e-7;

#[inline]
fn f32v(v: f64) -> f64 {
    v as f32 as f64
}

impl Automated {
    /// A constant (`setValueAtTime(v, 0)`).
    pub fn new(v: f64) -> Self {
        let v = f32v(v);
        Self {
            v0: v,
            v1: v,
            t0: 0.0,
            t1: 0.0,
            curve: Curve::Linear,
        }
    }

    /// `value = v` at `now` (cancel + setValueAtTime(v, now)).
    pub fn set(&mut self, v: f64, now: f64) {
        let v = f32v(v);
        *self = Self {
            v0: v,
            v1: v,
            t0: now,
            t1: now,
            curve: Curve::Linear,
        };
    }

    /// `rampTo(v, dur, now)`.
    pub fn ramp_to(&mut self, v: f64, dur: f64, now: f64, curve: Curve) {
        let mut start = self.value_at(now);
        let mut end = v;
        if curve == Curve::Exponential {
            if start == 0.0 {
                start = MIN_OUTPUT;
            }
            if end.abs() < 1e-6 {
                end = MIN_OUTPUT;
            }
        }
        *self = Self {
            v0: f32v(start),
            v1: f32v(end),
            t0: now,
            t1: now + dur.max(0.0),
            curve,
        };
    }

    /// The timeline value at `t` (for `t ≥` the last update time).
    #[inline]
    pub fn value_at(&self, t: f64) -> f64 {
        if t >= self.t1 {
            return self.v1;
        }
        if t < self.t0 {
            return self.v0;
        }
        let x = (t - self.t0) / (self.t1 - self.t0);
        match self.curve {
            Curve::Linear => self.v0 + (self.v1 - self.v0) * x,
            Curve::Exponential => {
                if self.v0 == 0.0 || self.v0 * self.v1 < 0.0 {
                    self.v0
                } else {
                    self.v0 * (self.v1 / self.v0).powf(x)
                }
            }
        }
    }

    /// The f32 the param outputs at `t`.
    #[inline]
    pub fn at(&self, t: f64) -> f32 {
        self.value_at(t) as f32
    }

    /// No ramp in flight from `t` on.
    #[inline]
    pub fn constant_from(&self, t: f64) -> bool {
        t >= self.t1
    }

    /// The value once the current ramp is over.
    pub fn target(&self) -> f64 {
        self.v1
    }
}
