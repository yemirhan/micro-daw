//! Web Audio `AudioParam` automation timeline (sample-accurate, a-rate), as scheduled by Tone.js.
//!
//! Semantics follow the Web Audio spec ("Computation of Value"):
//! - events are kept sorted by time; an event inserted at a time that already holds events goes
//!   after them (so a later `setValueAtTime` at the same time wins);
//! - ramps (`linearRampToValueAtTime`, `exponentialRampToValueAtTime`) are defined by their END
//!   time and run from the previous event's time/value;
//! - `setTargetAtTime(target, T, τ)`: `v(t) = target + (v(T) − target)·e^{−(t−T)/τ}` where `v(T)` is
//!   the value the timeline has at `T` (so a setTarget following a setTarget starts from the
//!   first curve's value at `T`);
//! - exponential ramps hold `V0` when `V0` and `V1` have opposite signs or `V0 == 0`.
//!
//! The value of frame `k` is the timeline evaluated at `t = k / sampleRate` (events take effect
//! on the first frame at or after their time), computed in f64 and stored as f32 like Chromium.
//! A ramp directly after a setTarget is treated as starting from the setTarget's start time and
//! value (the spec's "setTarget not started yet when the ramp was scheduled" case, which is the
//! only one an offline render scheduled at time 0 produces).
//!
//! Two Chromium behaviours beyond the spec text are reproduced in [`Param::render`] (both found
//! by comparing against the Chromium captures, see `parity_tests`): the start value of a
//! setTarget is Chromium's last output frame (see `start_values`), and a setTarget ends — its
//! target is output from then on — at the first render quantum (128 frames) where the value is
//! within e^−10 of the target.

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event {
    Set { time: f64, value: f64 },
    Linear { time: f64, value: f64 },
    Exponential { time: f64, value: f64 },
    Target { time: f64, value: f64, tau: f64 },
}

impl Event {
    pub fn time(&self) -> f64 {
        match *self {
            Event::Set { time, .. }
            | Event::Linear { time, .. }
            | Event::Exponential { time, .. }
            | Event::Target { time, .. } => time,
        }
    }

    fn value(&self) -> f64 {
        match *self {
            Event::Set { value, .. }
            | Event::Linear { value, .. }
            | Event::Exponential { value, .. }
            | Event::Target { value, .. } => value,
        }
    }

    fn is_ramp(&self) -> bool {
        matches!(self, Event::Linear { .. } | Event::Exponential { .. })
    }
}

/// An AudioParam with its intrinsic default value and automation events.
#[derive(Debug, Clone)]
pub struct Param {
    default: f64,
    events: Vec<Event>,
}

impl Param {
    /// A native param with `defaultValue` (no events yet).
    pub fn new(default: f64) -> Self {
        Self {
            default,
            events: Vec::new(),
        }
    }

    /// `new Tone.Param({ value })` / `Tone.Gain(value)` / `Tone.Signal(value)`: Tone schedules
    /// `setValueAtTime(value, 0)` when the value differs from the native default.
    pub fn tone(native_default: f64, value: f64) -> Self {
        let mut p = Self::new(native_default);
        if value != native_default {
            p.set_value_at_time(value, 0.0);
        }
        p
    }

    /// A Tone `Signal` (ConstantSourceNode offset, native default 1) holding `value`.
    pub fn signal(value: f64) -> Self {
        Self::tone(1.0, value)
    }

    /// A Tone `Gain` param (native default 1) holding `value`.
    pub fn gain(value: f64) -> Self {
        Self::tone(1.0, value)
    }

    pub fn events(&self) -> &[Event] {
        &self.events
    }

    fn insert(&mut self, ev: Event) {
        let t = ev.time();
        let at = self.events.partition_point(|e| e.time() <= t);
        self.events.insert(at, ev);
    }

    pub fn set_value_at_time(&mut self, value: f64, time: f64) -> &mut Self {
        self.insert(Event::Set { time, value });
        self
    }

    pub fn linear_ramp_to_value_at_time(&mut self, value: f64, time: f64) -> &mut Self {
        self.insert(Event::Linear { time, value });
        self
    }

    /// Tone replaces a 0 target by its `_minOutput` (1e-7).
    pub fn exponential_ramp_to_value_at_time(&mut self, value: f64, time: f64) -> &mut Self {
        let value = if value.abs() < 1e-6 { 1e-7 } else { value };
        self.insert(Event::Exponential { time, value });
        self
    }

    pub fn set_target_at_time(&mut self, value: f64, time: f64, tau: f64) -> &mut Self {
        self.insert(Event::Target { time, value, tau });
        self
    }

    /// Removes every event whose time is ≥ `time`.
    pub fn cancel_scheduled_values(&mut self, time: f64) -> &mut Self {
        self.events.retain(|e| e.time() < time);
        self
    }

    /// `Tone.Param.cancelAndHoldAtTime` (Tone's own implementation on top of the native calls):
    /// hold the current value at `time`, drop what follows, keep a ramp that was in flight as a
    /// ramp to the held value.
    pub fn cancel_and_hold_at_time(&mut self, time: f64) -> &mut Self {
        let held = self.value_at(time);
        let before_at_time = self
            .events
            .iter()
            .rev()
            .find(|e| e.time() <= time)
            .is_some_and(|e| (e.time() - time).abs() < 1e-6);
        let after = self.events.iter().find(|e| e.time() > time).copied();
        if before_at_time {
            match after {
                Some(a) => {
                    self.cancel_scheduled_values(a.time());
                }
                None => {
                    self.cancel_scheduled_values(time);
                }
            }
        } else if let Some(a) = after {
            self.cancel_scheduled_values(a.time());
            match a {
                Event::Linear { .. } => {
                    self.linear_ramp_to_value_at_time(held, time);
                }
                Event::Exponential { .. } => {
                    self.exponential_ramp_to_value_at_time(held, time);
                }
                _ => {}
            }
        }
        self.set_value_at_time(held, time)
    }

    /// Start value of every event: the event's own value, except for a setTarget, which starts
    /// from the value the timeline has when it begins. Without `sample_rate`: the exact value at
    /// the event time. With it, Chromium's frame-quantised behaviour (it continues from its last
    /// output value), matched against the Chromium captures:
    /// - after a ramp (or a set), the value of the last frame at or before the event time (ramps
    ///   include a frame landing exactly on their end time; a ramp ending mid-frame leaves its
    ///   last, unfinished frame value — e.g. 0.42·132/132.3 for the crash at 44.1 kHz);
    /// - after another setTarget, that curve's value at the last frame strictly before the event.
    fn start_values(&self, sample_rate: Option<f64>) -> Vec<f64> {
        let mut starts: Vec<f64> = Vec::with_capacity(self.events.len());
        for (i, ev) in self.events.iter().enumerate() {
            let s = match ev {
                Event::Target { time, .. } => {
                    let prefix = &self.events[..i];
                    let at = match sample_rate {
                        Some(sr) => {
                            let after_target = matches!(prefix.last(), Some(Event::Target { .. }));
                            let frame = if after_target {
                                (time * sr).ceil() - 1.0
                            } else {
                                (time * sr).floor()
                            };
                            let prev_frame = frame / sr;
                            let set_between = prefix.iter().any(|e| {
                                matches!(e, Event::Set { .. })
                                    && e.time() > prev_frame
                                    && e.time() <= *time
                            });
                            if prev_frame < 0.0 || set_between {
                                *time
                            } else {
                                prev_frame
                            }
                        }
                        None => *time,
                    };
                    eval(self.default, prefix, &starts, at)
                }
                other => other.value(),
            };
            starts.push(s);
        }
        starts
    }

    /// Timeline value at time `t` (seconds).
    pub fn value_at(&self, t: f64) -> f64 {
        eval(self.default, &self.events, &self.start_values(None), t)
    }

    /// Per-frame values for `frames` frames at `sample_rate` (a-rate), f32 like Chromium.
    pub fn render(&self, sample_rate: u32, frames: usize) -> Vec<f32> {
        let sr = sample_rate as f64;
        let starts = self.start_values(Some(sr));
        let ev = &self.events;
        let mut out = Vec::with_capacity(frames);
        // `p` = number of events with time ≤ t (monotonic in t).
        let mut p = 0usize;
        // Index of a setTarget that Chromium considers finished (snapped to its target).
        let mut done: Option<usize> = None;
        for k in 0..frames {
            let t = k as f64 / sr;
            while p < ev.len() && ev[p].time() <= t {
                p += 1;
            }
            let ramp_next = ev.get(p).is_some_and(Event::is_ramp);
            if let (Some(Event::Target { value: target, .. }), false) =
                (p.checked_sub(1).map(|i| ev[i]), ramp_next)
            {
                if done == Some(p - 1) {
                    out.push(target as f32);
                    continue;
                }
                let v = eval_at(self.default, ev, &starts, p, t);
                if k % RENDER_QUANTUM == 0 && set_target_reached(v, target) {
                    done = Some(p - 1);
                    out.push(target as f32);
                    continue;
                }
                out.push(v as f32);
                continue;
            }
            out.push(eval_at(self.default, ev, &starts, p, t) as f32);
        }
        out
    }
}

/// Web Audio render quantum (frames).
const RENDER_QUANTUM: usize = 128;
/// Chromium `kSetTargetThreshold` / `kSetTargetZeroThreshold` = e^−10.
const SET_TARGET_THRESHOLD: f64 = 4.539_992_976_248_485e-5;

/// Chromium ends a setTarget event (and outputs its target from then on) once, checked at the
/// start of a render quantum, the value is within e^−10 of the target (relative to the target,
/// absolute for a zero target). Matters for the snare/rim body: its amp (τ 45 ms) snaps to 0 at
/// ≈ 0.444 s, 56 ms before the oscillators stop.
fn set_target_reached(v: f64, target: f64) -> bool {
    if target == 0.0 {
        v.abs() < SET_TARGET_THRESHOLD
    } else {
        (v - target).abs() < SET_TARGET_THRESHOLD * target.abs()
    }
}

fn eval(default: f64, ev: &[Event], starts: &[f64], t: f64) -> f64 {
    let p = ev.partition_point(|e| e.time() <= t);
    eval_at(default, ev, starts, p, t)
}

/// `p` = count of events at or before `t`.
fn eval_at(default: f64, ev: &[Event], starts: &[f64], p: usize, t: f64) -> f64 {
    if let Some(next) = ev.get(p).filter(|e| e.is_ramp()) {
        let (t0, v0) = if p == 0 {
            (0.0, default)
        } else {
            (ev[p - 1].time(), starts[p - 1])
        };
        let (t1, v1) = (next.time(), next.value());
        let span = t1 - t0;
        if span <= 0.0 {
            return v1;
        }
        let x = (t - t0) / span;
        return match next {
            Event::Linear { .. } => v0 + (v1 - v0) * x,
            _ => {
                if v0 == 0.0 || v0 * v1 < 0.0 {
                    v0
                } else {
                    v0 * (v1 / v0).powf(x)
                }
            }
        };
    }
    if p == 0 {
        return default;
    }
    match ev[p - 1] {
        Event::Target { time, value, tau } => {
            value + (starts[p - 1] - value) * (-(t - time) / tau).exp()
        }
        other => other.value(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pulse_shape() {
        let mut g = Param::gain(0.0);
        g.cancel_and_hold_at_time(0.0);
        g.linear_ramp_to_value_at_time(0.8, 0.0008);
        g.set_target_at_time(0.0, 0.0028, 0.1);
        assert_eq!(g.events().len(), 3);
        assert_eq!(g.value_at(0.0), 0.0);
        assert!((g.value_at(0.0004) - 0.4).abs() < 1e-12);
        assert_eq!(g.value_at(0.002), 0.8);
        assert!((g.value_at(0.1028) - 0.8 * (-1f64).exp()).abs() < 1e-12);
    }

    #[test]
    fn same_time_set_wins_and_target_chain() {
        let mut g = Param::gain(0.0);
        g.set_value_at_time(2.0, 0.0);
        assert_eq!(g.value_at(0.0), 2.0);
        g.set_target_at_time(1.0, 0.01, 0.01);
        g.set_target_at_time(0.0, 0.02, 0.01);
        let v1 = 1.0 + (2.0 - 1.0) * (-1f64).exp();
        assert!((g.value_at(0.02) - v1).abs() < 1e-12);
        assert!((g.value_at(0.03) - v1 * (-1f64).exp()).abs() < 1e-12);
    }

    #[test]
    fn exponential_ramps() {
        let mut f = Param::signal(50.0);
        f.cancel_scheduled_values(0.0);
        f.set_value_at_time(190.0, 0.0);
        f.exponential_ramp_to_value_at_time(56.0, 0.045);
        f.exponential_ramp_to_value_at_time(46.0, 0.35);
        assert_eq!(f.value_at(0.0), 190.0);
        assert!((f.value_at(0.0225) - (190f64 * 56.0).sqrt()).abs() < 1e-9);
        assert_eq!(f.value_at(0.045), 56.0);
        assert_eq!(f.value_at(1.0), 46.0);
        let r = f.render(1000, 400);
        assert_eq!(r[0], 190.0);
        assert_eq!(r[399], 46.0);
    }

    #[test]
    fn set_target_snaps_to_target_on_a_render_quantum() {
        let mut g = Param::gain(1.0);
        g.set_target_at_time(0.0, 0.0, 0.02);
        let r = g.render(1000, 300);
        // e^(−k/20) < e^−10 from k = 201; the check runs at quantum starts → 0 from frame 256.
        assert!(r[255] > 0.0 && r[255] < 1e-5);
        assert_eq!(r[256], 0.0);
        assert_eq!(r[299], 0.0);
        // value_at stays the exact curve.
        assert!(g.value_at(0.256) > 0.0);
    }

    #[test]
    fn set_target_after_mid_frame_ramp_starts_from_last_frame() {
        let mut g = Param::gain(0.0);
        g.linear_ramp_to_value_at_time(1.0, 0.0105);
        g.set_target_at_time(0.0, 0.0105, 1.0);
        let r = g.render(1000, 12);
        // ramp frame 10 = 10/10.5; the target continues from there, not from 1.0.
        assert!((r[10] as f64 - 10.0 / 10.5).abs() < 1e-6, "{}", r[10]);
        assert!(r[11] < r[10]);
    }

    #[test]
    fn cancel_and_hold_mid_ramp() {
        let mut g = Param::gain(0.0);
        g.linear_ramp_to_value_at_time(1.0, 1.0);
        g.cancel_and_hold_at_time(0.5);
        assert!((g.value_at(0.5) - 0.5).abs() < 1e-12);
        assert!((g.value_at(2.0) - 0.5).abs() < 1e-12);
    }
}
