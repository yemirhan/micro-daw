//! A `Tone.Param` over a native AudioParam: Tone's JS mirror ([`ToneTimeline`]) + Chromium's
//! timeline ([`NativeTimeline`]) + the intrinsic value, with Tone's scheduling API
//! (`Param.ts`: `setRampPoint`, `linearRampTo`, `exponentialRampTo`, `targetRampTo`,
//! `exponentialApproachValueAtTime`, `cancelAndHoldAtTime`, the `value` setter).

use super::native_tl::NativeTimeline;
use super::tone_tl::{Kind, ToneEv, ToneTimeline, eq};

/// What the param's owner knows when it schedules: Web Audio `currentTime` (= the start of the
/// next quantum to render; also Tone's `now()`, the Rust engine has no look-ahead) and the rate.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Clock {
    pub now: f64,
    pub sr: f64,
}

/// Tone's `_minOutput` (an exponential ramp / ramp point never sits on 0).
const MIN_OUTPUT: f64 = 1e-7;

#[derive(Debug, Clone)]
pub(crate) struct AutoParam {
    tone: ToneTimeline,
    nat: NativeTimeline,
    /// Tone `_initialValue` (the native default value).
    initial: f64,
    intrinsic: f32,
    min: f32,
    max: f32,
}

impl AutoParam {
    /// `new Tone.Param({ value })` over a native param with `native_default`: Tone schedules
    /// `setValueAtTime(value, 0)` when the value differs from the default.
    pub fn new(native_default: f64, value: f64, min: f32, max: f32, cx: Clock) -> Self {
        let mut p = Self {
            tone: ToneTimeline::default(),
            nat: NativeTimeline::default(),
            initial: native_default,
            intrinsic: native_default as f32,
            min,
            max,
        };
        if value != native_default {
            p.set_value_at_time(value, 0.0, cx);
        }
        p
    }

    /// A Tone `Gain`'s gain (native default 1, unbounded).
    pub fn gain(value: f64, cx: Clock) -> Self {
        Self::new(1.0, value, f32::MIN, f32::MAX, cx)
    }

    /// A Tone `Signal` (ConstantSource offset, native default 1) holding `value`.
    pub fn signal(value: f64, cx: Clock) -> Self {
        Self::new(1.0, value, f32::MIN, f32::MAX, cx)
    }

    fn add(&mut self, kind: Kind, value: f64, time: f64, cx: Clock) {
        self.tone.add(ToneEv { time, kind, value });
        self.nat
            .insert(kind, value as f32, time, cx.now, cx.sr, self.intrinsic);
    }

    pub fn set_value_at_time(&mut self, value: f64, time: f64, cx: Clock) {
        self.add(Kind::Set, value, time, cx);
    }

    pub fn linear_ramp_to_value_at_time(&mut self, value: f64, time: f64, cx: Clock) {
        self.add(Kind::Linear, value, time, cx);
    }

    /// A 0 target becomes `_minOutput`.
    pub fn exponential_ramp_to_value_at_time(&mut self, value: f64, time: f64, cx: Clock) {
        let value = if eq(value, 0.0) { MIN_OUTPUT } else { value };
        self.add(Kind::Exp, value, time, cx);
    }

    pub fn set_target_at_time(&mut self, value: f64, time: f64, constant: f64, cx: Clock) {
        self.add(Kind::Target(constant), value, time, cx);
    }

    pub fn cancel_scheduled_values(&mut self, time: f64, cx: Clock) {
        self.tone.cancel(time);
        self.nat.cancel(time, cx.now);
    }

    /// `param.value = v`: cancel from now, set at now (an event exactly at the quantum start,
    /// effective from `ceil(now·sr)`).
    pub fn set_value(&mut self, value: f64, cx: Clock) {
        self.cancel_scheduled_values(cx.now, cx);
        self.set_value_at_time(value, cx.now, cx);
    }

    /// `Param.getValueAtTime`.
    pub fn get_value_at_time(&self, time: f64) -> f64 {
        self.tone.value_at(time, self.initial)
    }

    /// `Param.cancelAndHoldAtTime` (Tone's implementation over native calls).
    pub fn cancel_and_hold_at_time(&mut self, time: f64, cx: Clock) {
        let value = self.get_value_at_time(time);
        let before = self.tone.get(time);
        let after = self.tone.get_after(time);
        if before.is_some_and(|b| eq(b.time, time)) {
            if let Some(a) = after {
                self.nat.cancel(a.time, cx.now);
                self.tone.cancel(a.time);
            } else {
                self.nat.cancel_and_hold(time, cx.now);
                self.tone.cancel(time + 1.0 / cx.sr);
            }
        } else if let Some(a) = after {
            self.nat.cancel(a.time, cx.now);
            self.tone.cancel(a.time);
            match a.kind {
                Kind::Linear => self.linear_ramp_to_value_at_time(value, time, cx),
                Kind::Exp => self.exponential_ramp_to_value_at_time(value, time, cx),
                _ => {}
            }
        }
        self.tone.add(ToneEv {
            time,
            kind: Kind::Set,
            value,
        });
        self.nat
            .insert(Kind::Set, value as f32, time, cx.now, cx.sr, self.intrinsic);
    }

    /// `Param.setRampPoint`.
    pub fn set_ramp_point(&mut self, time: f64, cx: Clock) {
        let mut current = self.get_value_at_time(time);
        self.cancel_and_hold_at_time(time, cx);
        if current == 0.0 {
            current = MIN_OUTPUT;
        }
        self.set_value_at_time(current, time, cx);
    }

    /// `Param.linearRampTo(value, rampTime, startTime)`.
    pub fn linear_ramp_to(&mut self, value: f64, ramp: f64, start: f64, cx: Clock) {
        self.set_ramp_point(start, cx);
        self.linear_ramp_to_value_at_time(value, start + ramp, cx);
    }

    /// `Param.exponentialRampTo(value, rampTime, startTime)`.
    pub fn exponential_ramp_to(&mut self, value: f64, ramp: f64, start: f64, cx: Clock) {
        self.set_ramp_point(start, cx);
        self.exponential_ramp_to_value_at_time(value, start + ramp, cx);
    }

    /// `Param.targetRampTo(value, rampTime, startTime)`.
    pub fn target_ramp_to(&mut self, value: f64, ramp: f64, start: f64, cx: Clock) {
        self.set_ramp_point(start, cx);
        self.exponential_approach_value_at_time(value, start, ramp, cx);
    }

    /// `Param.exponentialApproachValueAtTime`: setTarget with `τ = ln(ramp + 1) / ln 200`, held
    /// at 90 % and finished by a linear ramp.
    pub fn exponential_approach_value_at_time(
        &mut self,
        value: f64,
        time: f64,
        ramp: f64,
        cx: Clock,
    ) {
        let constant = (ramp + 1.0).ln() / 200f64.ln();
        self.set_target_at_time(value, time, constant, cx);
        self.cancel_and_hold_at_time(time + ramp * 0.9, cx);
        self.linear_ramp_to_value_at_time(value, time + ramp, cx);
    }

    /// Drop mirror events no query at or after `now` can see.
    pub fn prune(&mut self, now: f64) {
        self.tone.prune(now);
    }

    /// Render one quantum of the param's own (intrinsic) values from context frame `frame`.
    /// `connected`: the param has audio inputs, which makes Chromium compute it sample-accurately
    /// every quantum; otherwise it is sample-accurate only while events are in play
    /// (`HasSampleAccurateValues`), else the k-rate value fills the quantum.
    pub fn render(&mut self, frame: u64, sr: f64, connected: bool, out: &mut [f32]) -> bool {
        if out.is_empty() {
            return false;
        }
        if connected || self.nat.has_values(frame, sr) {
            let default = self.intrinsic;
            let last = self.nat.values(frame, out, sr, default);
            let (lo, hi) = (self.min, self.max);
            for v in out.iter_mut() {
                *v = v.clamp(lo, hi);
            }
            self.intrinsic = last.clamp(lo, hi);
            true
        } else {
            if self.nat.started_at(frame, sr) {
                let mut one = [0f32; 1];
                let last = self.nat.values(frame, &mut one, sr, self.intrinsic);
                self.intrinsic = last.clamp(self.min, self.max);
            }
            out.fill(self.intrinsic);
            false
        }
    }

    /// A GainNode with this gain silences its output this quantum (Chromium zeroes the bus and
    /// marks it silent when the gain is k-rate and 0). Call after [`AutoParam::render`].
    pub fn gate_silent(&self, sample_accurate: bool) -> bool {
        !sample_accurate && self.intrinsic == 0.0
    }

    /// The value now when nothing is scheduled (fast path for idle params).
    pub fn idle_value(&self) -> Option<f32> {
        self.nat.is_settled().then_some(self.intrinsic)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 48000.0;

    fn cx(now: f64) -> Clock {
        Clock { now, sr: SR }
    }

    fn render(p: &mut AutoParam, frames: usize) -> Vec<f32> {
        let mut out = vec![0f32; frames];
        for (q, chunk) in out.chunks_mut(128).enumerate() {
            p.render(q as u64 * 128, SR, true, chunk);
        }
        out
    }

    #[test]
    fn an_event_at_now_after_settled_events_starts_at_ceil() {
        // Chromium keeps the last (past) event, so a set at exactly the quantum start whose
        // t·sr rounds above the frame (2.2 s · 48 kHz) starts one frame late.
        let mut p = AutoParam::signal(261.63, cx(0.0));
        p.set_value_at_time(65.4, 2.0, cx(2.0));
        let mut out = [0f32; 128];
        for f in (96000u64..105600).step_by(128) {
            p.render(f, SR, false, &mut out);
        }
        p.cancel_scheduled_values(2.2, cx(2.2));
        p.set_value_at_time(98.0, 2.2, cx(2.2));
        p.render(105600, SR, false, &mut out);
        assert_eq!(out[0], 65.4);
        assert_eq!(out[1], 98.0);
    }

    #[test]
    fn exponential_approach_holds_at_ninety_percent_then_ramps() {
        let mut p = AutoParam::signal(1.0, cx(0.0));
        p.exponential_approach_value_at_time(0.0, 0.0, 0.1, cx(0.0));
        let tau = 1.1f64.ln() / 200f64.ln();
        let v = p.get_value_at_time(0.05);
        assert!((v - (-0.05 / tau).exp()).abs() < 1e-12);
        let hold = (-0.09 / tau).exp();
        assert!((p.get_value_at_time(0.095) - hold * 0.5).abs() < 1e-9);
        assert_eq!(p.get_value_at_time(0.2), 0.0);
        let r = render(&mut p, 9600);
        assert!(
            (r[2400] as f64 - (-0.05 / tau).exp()).abs() < 1e-4,
            "{}",
            r[2400]
        );
        assert_eq!(r[4800], 0.0);
        // Linear segment from the 90 % hold.
        assert!((r[4560] as f64 - hold * 0.5).abs() < 1e-5, "{}", r[4560]);
    }

    #[test]
    fn set_target_converges_to_zero() {
        let mut p = AutoParam::signal(1.0, cx(0.0));
        p.set_target_at_time(0.0, 0.0, 0.001, cx(0.0));
        let r = render(&mut p, 1024);
        assert_eq!(r[0], 1.0);
        // e^-10 is reached after 10 ms = 480 frames: the quantum starting at 512 snaps to 0.
        assert!(r[511] > 0.0);
        assert_eq!(r[512], 0.0);
    }

    #[test]
    fn mirror_set_target_value_uses_previous_event() {
        let mut p = AutoParam::signal(0.0, cx(0.0));
        p.linear_ramp_to(1.0, 0.01, 0.0, cx(0.0));
        p.set_target_at_time(0.5, 0.01, 0.1, cx(0.0));
        let v = p.get_value_at_time(0.11);
        assert!((v - (0.5 + 0.5 * (-1f64).exp())).abs() < 1e-12, "{v}");
    }
}
