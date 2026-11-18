//! Chromium's native AudioParam timeline (`AudioParamTimeline`, Blink
//! `audio_param_timeline.cc`), realtime-safe (fixed capacity, no allocation). Extends the
//! SamplerKit port (`sampler/param.rs`) with `setTargetAtTime`, which Tone's envelopes use for
//! every decay and release (`exponentialApproachValueAtTime`).
//!
//! - events are inserted after the events at the same time, values stored as f32;
//! - `cancelScheduledValues(t)` removes events at or after `max(t, currentTime)`;
//! - an event inserted with a time before the current time is clamped to it when the next render
//!   quantum is computed (`ClampNewEventsToCurrentTime`, stable re-sort);
//! - frame `k` belongs to the last event with `time·sr ≤ k` (`fill_to_end_frame = ceil(t·sr)`);
//! - linear ramps: `x = f32((k/sr − t1)·f32(1/(t2 − t1)))`, `v = v1 + x·(v2 − v1)` in f32;
//! - exponential ramps: `V·m^k` accumulated in f64 from the quantum start, clipped to `v2`
//!   within half a frame of the end;
//! - setTarget (`ProcessSetTarget`): on the event's first frame `v = T + (v − T)·e^{−(k/sr − t1)/τ}`
//!   from the last computed value `v`, then `v += (T − v)·c` per frame in f32 with
//!   `c = f32(1 − e^{−1/(sr·τ)})`; once the value is within e^−10 of the target (relative, or
//!   absolute for a 0 target: the factory-kit captures) the target is output instead;
//! - a setTarget followed by a ramp becomes a set event at the current frame
//!   (`ProcessSetTargetFollowedByRamp`);
//! - events whose successor is in the past are removed, and once every event is 1.5 quanta in the
//!   past (and a final setTarget has converged) the intrinsic value holds
//!   (`HandleAllEventsInThePast`); the last event stays in the timeline, so an event added later
//!   at exactly the current time starts at `ceil(t·sr)` (one frame late when `t·sr` rounds above
//!   the quantum start), not at the quantum start.

use super::tone_tl::Kind;

/// Events per native timeline.
pub(crate) const CAP: usize = 48;
const QUANTUM: f64 = 128.0;
/// Chromium `kSetTargetThreshold` (e^−10).
const SET_TARGET_THRESHOLD: f32 = 4.539_993e-5;

#[derive(Debug, Clone, Copy)]
pub(crate) struct NatEv {
    pub time: f64,
    pub kind: Kind,
    pub value: f32,
    /// Inserted since the last rendered quantum (`new_events_`).
    pub new: bool,
}

const EMPTY: NatEv = NatEv {
    time: 0.0,
    kind: Kind::Set,
    value: 0.0,
    new: false,
};

#[derive(Debug, Clone)]
pub(crate) struct NativeTimeline {
    buf: [NatEv; CAP],
    len: usize,
    /// Every event is over (`HandleAllEventsInThePast` fired since the last insertion).
    settled: bool,
}

impl Default for NativeTimeline {
    fn default() -> Self {
        Self {
            buf: [EMPTY; CAP],
            len: 0,
            settled: false,
        }
    }
}

fn set_target_converged(value: f32, target: f32) -> bool {
    if target == 0.0 {
        value.abs() < SET_TARGET_THRESHOLD
    } else {
        (value - target).abs() < SET_TARGET_THRESHOLD * target.abs()
    }
}

/// `IsEventCurrent`.
fn is_event_current(ev: &NatEv, next: Option<&NatEv>, current_frame: u64, sr: f64) -> bool {
    let Some(next) = next else {
        return true;
    };
    if next.time * sr >= current_frame as f64 {
        return true;
    }
    let event_frame = ev.time * sr;
    ev.kind == Kind::Set
        && event_frame <= current_frame as f64
        && (current_frame as f64) < event_frame + 1.0
}

impl NativeTimeline {
    /// Nothing scheduled any more: no events, or only the last one, over (see `values`).
    pub fn is_settled(&self) -> bool {
        self.len == 0 || self.settled
    }

    fn ev(&self) -> &[NatEv] {
        &self.buf[..self.len]
    }

    fn remove_front(&mut self, n: usize) {
        let n = n.min(self.len);
        self.buf.copy_within(n..self.len, 0);
        self.len -= n;
    }

    /// `InsertEvent`. `intrinsic`: the param's current value (a ramp with nothing before it
    /// starts from it, at `now`).
    pub fn insert(&mut self, kind: Kind, value: f32, time: f64, now: f64, sr: f64, intrinsic: f32) {
        if self.len == 0 && kind.is_ramp() {
            self.insert(Kind::Set, intrinsic, now, now, sr, intrinsic);
        }
        if self.len == CAP {
            self.prune(now, sr);
        }
        if self.len == CAP {
            self.collapse();
        }
        if self.len == CAP {
            // Still full (pathological scheduling far ahead): drop the oldest event.
            self.remove_front(1);
        }
        self.settled = false;
        let at = self.ev().partition_point(|e| e.time <= time);
        self.buf.copy_within(at..self.len, at + 1);
        self.buf[at] = NatEv {
            time,
            kind,
            value,
            new: true,
        };
        self.len += 1;
    }

    /// `InsertEvent`'s pruning of events that are over at the current frame.
    fn prune(&mut self, now: f64, sr: f64) {
        let frame = (now * sr).round() as u64;
        let ev = self.ev();
        let mut skipped = 0;
        for i in 0..ev.len() {
            if is_event_current(&ev[i], ev.get(i + 1), frame, sr) {
                break;
            }
            skipped += 1;
        }
        self.remove_front(skipped);
    }

    /// Merge consecutive set events at the same time (equivalent values).
    fn collapse(&mut self) {
        let mut out = 0;
        for i in 0..self.len {
            let e = self.buf[i];
            if out > 0 {
                let prev = self.buf[out - 1];
                if prev.kind == Kind::Set && e.kind == Kind::Set && prev.time == e.time {
                    self.buf[out - 1] = e;
                    continue;
                }
            }
            self.buf[out] = e;
            out += 1;
        }
        self.len = out;
    }

    /// `cancelScheduledValues(time)`: events at or after `max(time, now)` go.
    pub fn cancel(&mut self, time: f64, now: f64) {
        let t = time.max(now);
        if let Some(at) = self.ev().iter().position(|e| e.time >= t) {
            self.len = at;
        }
    }

    /// Native `cancelAndHoldAtTime(time)` (Tone calls it when an event sits exactly at `time`
    /// and nothing follows in its mirror): the events after `time` go, except that a ramp still in
    /// progress at `time` is cut there, ending at its value at `time` (Chromium's
    /// `CancelValues` event).
    pub fn cancel_and_hold(&mut self, time: f64, now: f64) {
        let t = time.max(now);
        let Some(at) = self.ev().iter().position(|e| e.time > t) else {
            return;
        };
        let ev = self.buf[at];
        if ev.kind.is_ramp() && at > 0 {
            let prev = self.buf[at - 1];
            let (t1, v1, t2, v2) = (prev.time, prev.value as f64, ev.time, ev.value as f64);
            let x = if t2 > t1 { (t - t1) / (t2 - t1) } else { 1.0 };
            let v = if ev.kind == Kind::Linear {
                v1 + (v2 - v1) * x
            } else if v1 * v2 > 0.0 {
                v1 * (v2 / v1).powf(x)
            } else {
                v1
            };
            self.buf[at] = NatEv {
                time: t,
                kind: ev.kind,
                value: v as f32,
                new: true,
            };
            self.len = at + 1;
        } else {
            self.len = at;
        }
    }

    /// `HasValues` for a param without input connections: sample-accurate this quantum?
    pub fn has_values(&self, frame: u64, sr: f64) -> bool {
        let ev = self.ev();
        let Some(first) = ev.first() else {
            return false;
        };
        if first.time > (frame as f64 + QUANTUM) / sr && first.kind == Kind::Set {
            return false;
        }
        if ev.len() >= 2 {
            return true;
        }
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        let past = !(first.time < frame as f64 / sr);
        past
    }

    /// Whether the first event has started at `frame` (k-rate `ValueForContextTime`).
    pub fn started_at(&self, frame: u64, sr: f64) -> bool {
        self.len > 0 && frame as f64 / sr >= self.buf[0].time
    }

    fn clamp_new(&mut self, current_time: f64) {
        let mut clamped = false;
        for e in &mut self.buf[..self.len] {
            if e.new && e.time < current_time {
                e.time = current_time;
                clamped = true;
            }
            e.new = false;
        }
        if clamped {
            let s = &mut self.buf[..self.len];
            for i in 1..s.len() {
                let mut j = i;
                while j > 0 && s[j].time < s[j - 1].time {
                    s.swap(j, j - 1);
                    j -= 1;
                }
            }
        }
    }

    /// `ValuesForFrameRangeImpl`: `out.len()` values from context frame `start`; `default` is
    /// the intrinsic value. Returns the last computed value (the new intrinsic value).
    pub fn values(&mut self, start: u64, out: &mut [f32], sr: f64, default: f32) -> f32 {
        let n = out.len();
        let end = start + n as u64;
        if self.len == 0 || end as f64 / sr <= self.buf[0].time {
            out.fill(default);
            return default;
        }
        if self.ev().iter().any(|e| e.new) {
            self.clamp_new(start as f64 / sr);
        }
        // HandleAllEventsInThePast.
        let current_time = start as f64 / sr;
        let last = self.buf[self.len - 1];
        if last.time + 1.5 * QUANTUM / sr < current_time {
            let mut value = default;
            let done = match last.kind {
                Kind::Target(_) => {
                    if set_target_converged(default, last.value) {
                        value = last.value;
                        true
                    } else {
                        false
                    }
                }
                _ => true,
            };
            if done {
                // Chromium keeps the last event (it still defines where a later event that lands
                // on the current time starts: `ceil(t·sr)`, not the quantum start).
                out.fill(value);
                self.remove_front(self.len - 1);
                self.settled = true;
                return value;
            }
        }
        // HandleFirstEvent.
        let mut current_frame = start;
        let mut w = 0usize;
        let first_time = self.buf[0].time;
        if first_time > start as f64 / sr {
            let first_frame = (first_time * sr).ceil();
            let fill_to_end = if end as f64 > first_frame {
                first_frame as u64
            } else {
                end
            };
            let fill_to = ((fill_to_end - start) as usize).min(n);
            out[..fill_to].fill(default);
            w = fill_to;
            current_frame += fill_to as u64;
        }
        let mut value = default;
        let mut last_skipped: Option<usize> = None;
        let len = self.len;
        let mut i = 0;
        while i < len && w < n {
            let next = (i + 1 < len).then(|| self.buf[i + 1]);
            if !is_event_current(&self.buf[i], next.as_ref(), current_frame, sr) {
                last_skipped = Some(i);
                i += 1;
                continue;
            }
            let next_kind = next.map(|e| e.kind);
            // ProcessSetTargetFollowedByRamp.
            if let Kind::Target(tau) = self.buf[i].kind
                && next_kind.is_some_and(Kind::is_ramp)
            {
                let ev = self.buf[i];
                if (2.0 * sr * ev.time - 2.0 * current_frame as f64 + 1.0).abs() <= 1.0 {
                    value = (ev.value as f64
                        + (value as f64 - ev.value as f64)
                            * (-(current_frame as f64 / sr - ev.time) / tau).exp())
                        as f32;
                } else {
                    let c = (1.0 - (-1.0 / (sr * tau)).exp()) as f32;
                    value += (ev.value - value) * c;
                }
                self.buf[i] = NatEv {
                    time: current_frame as f64 / sr,
                    kind: Kind::Set,
                    value,
                    new: false,
                };
            }
            let ev = self.buf[i];
            let (value1, time1) = (ev.value, ev.time);
            let (value2, time2) = match next {
                Some(nx) => (nx.value, nx.time),
                None => (value1, end as f64 / sr + 1.0),
            };
            let fill_to_end_frame = if end as f64 > time2 * sr {
                (time2 * sr).ceil() as u64
            } else {
                end
            };
            let fill_to = (fill_to_end_frame.saturating_sub(start) as usize)
                .min(n)
                .max(w);
            match next_kind {
                Some(Kind::Linear) => {
                    let delta_time = time2 - time1;
                    let k: f32 = if delta_time > 0.0 {
                        (1.0 / delta_time) as f32
                    } else {
                        0.0
                    };
                    let value_delta = value2 - value1;
                    for o in &mut out[w..fill_to] {
                        let x = ((current_frame as f64 / sr - time1) * k as f64) as f32;
                        value = value1 + x * value_delta;
                        *o = value;
                        current_frame += 1;
                    }
                    w = fill_to;
                }
                Some(Kind::Exp) => {
                    if value1 * value2 <= 0.0 || time1 >= time2 {
                        value = value1;
                        out[w..fill_to].fill(value);
                    } else {
                        let delta_time = time2 - time1;
                        let num_sample_frames = delta_time * sr;
                        let multiplier =
                            (value2 as f64 / value1 as f64).powf(1.0 / num_sample_frames);
                        value = (value1 as f64
                            * (value2 as f64 / value1 as f64)
                                .powf((current_frame as f64 / sr - time1) / delta_time))
                            as f32;
                        let mut acc = value as f64;
                        for o in &mut out[w..fill_to] {
                            value = acc as f32;
                            acc *= multiplier;
                            current_frame += 1;
                            *o = value;
                        }
                        if current_frame as f64 > time2 * sr - 0.5 {
                            value = value2;
                        }
                    }
                    w = fill_to;
                }
                _ => match ev.kind {
                    Kind::Target(tau) => {
                        let target = value1;
                        let c = (1.0 - (-1.0 / (sr * tau)).exp()) as f32;
                        let ramp_start_frame = time1 * sr;
                        let cf = current_frame as f64;
                        if ramp_start_frame <= cf && cf < ramp_start_frame + 1.0 {
                            value = (target as f64
                                + (value as f64 - target as f64) * (-(cf / sr - time1) / tau).exp())
                                as f32;
                        } else {
                            value += (target - value) * c;
                        }
                        if set_target_converged(value, target) {
                            out[w..fill_to].fill(target);
                            current_frame += (fill_to - w) as u64;
                            value = target;
                        } else {
                            for o in &mut out[w..fill_to] {
                                *o = value;
                                value += (target - value) * c;
                            }
                            if fill_to >= 1 && fill_to > w {
                                value = out[fill_to - 1];
                            }
                            current_frame = fill_to_end_frame;
                        }
                        w = fill_to;
                    }
                    Kind::Exp => {
                        current_frame = fill_to_end_frame;
                        if i >= 1 && self.buf[i - 1].value * ev.value > 0.0 {
                            value = ev.value;
                        }
                        out[w..fill_to].fill(value);
                        w = fill_to;
                    }
                    _ => {
                        current_frame = fill_to_end_frame;
                        value = ev.value;
                        out[w..fill_to].fill(value);
                        w = fill_to;
                    }
                },
            }
            i += 1;
        }
        if let Some(s) = last_skipped {
            self.remove_front(s + 1);
        }
        out[w..].fill(value);
        value
    }
}
