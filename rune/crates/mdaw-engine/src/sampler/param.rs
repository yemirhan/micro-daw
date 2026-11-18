//! An `AudioParam` driven through Tone.js, reproduced for realtime use (fixed capacity, no
//! allocation): Tone's own JS-side event timeline (`Tone.Param._events`, used by
//! `getValueAtTime`, `cancelAndHoldAtTime`, `setRampPoint`) on top of Chromium's native timeline
//! (`AudioParamHandler`, Blink `audio_param_handler.cc`).
//!
//! Only the event types SamplerKit produces are modelled: `setValueAtTime`,
//! `linearRampToValueAtTime`, `exponentialRampToValueAtTime`.
//!
//! **Tone mirror** (`Tone.Timeline`): f64 values, comparisons with Tone's 1e-6 epsilon (`EQ`,
//! `LT`, `GT`), an event added at a time that already holds events goes after them.
//!
//! **Chromium timeline**:
//! - events are inserted after the events at the same time (`upper_bound`), values stored as f32;
//! - `cancelScheduledValues(t)` removes events at or after `max(t, currentTime)`;
//! - an event inserted with a time before the current time is clamped to it when the next render
//!   quantum is computed (`ClampNewEventsToCurrentTime`, stable re-sort);
//! - frame `k` belongs to the last event with `time·sr ≤ k` (`fill_to_end_frame = ceil(t·sr)`);
//! - linear ramps: `x = f32((k/sr − t1)·f32(1/(t2 − t1)))`, `v = v1 + x·(v2 − v1)` in f32 (the
//!   serial path of `ProcessLinearRamp`, which arm64 builds use);
//! - exponential ramps: `V·m^k` accumulated in f64 from the quantum start, clipped to `v2`
//!   within half a frame of the end;
//! - a quantum is sample-accurate or constant per `HasSampleAccurateValues` (decides the
//!   StereoPanner path), events whose successor is in the past are removed, and once every event
//!   is 1.5 quanta in the past the timeline is cleared and the intrinsic value holds.

const EPSILON: f64 = 1e-6;
/// Events per timeline. SamplerKit needs at most ~12 per voice envelope; pad params are pruned as
/// they render.
pub(crate) const CAP: usize = 32;
const QUANTUM: f64 = 128.0;

fn eq(a: f64, b: f64) -> bool {
    (a - b).abs() < EPSILON
}
fn gt(a: f64, b: f64) -> bool {
    a > b + EPSILON
}
fn lt(a: f64, b: f64) -> bool {
    a + EPSILON < b
}
fn gte(a: f64, b: f64) -> bool {
    gt(a, b) || eq(a, b)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Set,
    Linear,
    Exp,
}

#[derive(Debug, Clone, Copy)]
struct ToneEv {
    time: f64,
    kind: Kind,
    value: f64,
}

#[derive(Debug, Clone, Copy)]
struct NatEv {
    time: f64,
    kind: Kind,
    value: f32,
    /// Inserted since the last rendered quantum (`new_events_`).
    new: bool,
}

/// Fixed-capacity ordered list.
#[derive(Debug, Clone)]
struct List<T: Copy> {
    buf: [T; CAP],
    len: usize,
}

impl<T: Copy> List<T> {
    fn new(fill: T) -> Self {
        Self {
            buf: [fill; CAP],
            len: 0,
        }
    }
    fn as_slice(&self) -> &[T] {
        &self.buf[..self.len]
    }
    fn insert(&mut self, at: usize, ev: T) -> bool {
        if self.len == CAP {
            return false;
        }
        self.buf.copy_within(at..self.len, at + 1);
        self.buf[at] = ev;
        self.len += 1;
        true
    }
    fn truncate(&mut self, len: usize) {
        self.len = self.len.min(len);
    }
    fn remove_front(&mut self, n: usize) {
        let n = n.min(self.len);
        self.buf.copy_within(n..self.len, 0);
        self.len -= n;
    }
}

/// What the param's owner knows about the context when it schedules: Web Audio
/// `currentTime` (= the start of the next quantum) and the sample rate.
#[derive(Debug, Clone, Copy)]
pub struct Clock {
    pub now: f64,
    pub sr: f64,
}

impl Clock {
    fn sample_time(&self) -> f64 {
        1.0 / self.sr
    }
}

/// One Tone-wrapped AudioParam (Tone timeline + Chromium timeline + intrinsic value).
#[derive(Debug, Clone)]
pub(crate) struct AutoParam {
    tone: List<ToneEv>,
    nat: List<NatEv>,
    /// Tone `_initialValue` (the native default value).
    initial: f64,
    /// Chromium intrinsic value.
    intrinsic: f32,
    min: f32,
    max: f32,
}

impl AutoParam {
    /// `new Tone.Param({ value })` over a native param whose default is `native_default`: Tone
    /// schedules `setValueAtTime(value, 0)` when the value differs from the default.
    pub fn new(native_default: f64, value: f64, min: f32, max: f32, cx: Clock) -> Self {
        let mut p = Self {
            tone: List::new(ToneEv {
                time: 0.0,
                kind: Kind::Set,
                value: 0.0,
            }),
            nat: List::new(NatEv {
                time: 0.0,
                kind: Kind::Set,
                value: 0.0,
                new: false,
            }),
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

    /// Reset to a fresh `new Tone.Param` (pool reuse, no allocation).
    pub fn reset(&mut self, native_default: f64, value: f64, cx: Clock) {
        self.tone.len = 0;
        self.nat.len = 0;
        self.initial = native_default;
        self.intrinsic = native_default as f32;
        if value != native_default {
            self.set_value_at_time(value, 0.0, cx);
        }
    }

    // ---------------------------------------------------------------- Tone timeline

    /// `Timeline._search`.
    fn search(&self, time: f64) -> Option<usize> {
        let tl = self.tone.as_slice();
        let len = tl.len();
        if len == 0 {
            return None;
        }
        if tl[len - 1].time <= time {
            return Some(len - 1);
        }
        let (mut beginning, mut end) = (0usize, len);
        while beginning < end {
            let mut mid = beginning + (end - beginning) / 2;
            let ev = tl[mid];
            if eq(ev.time, time) {
                for (i, e) in tl.iter().enumerate().skip(mid) {
                    if eq(e.time, time) {
                        mid = i;
                    } else {
                        break;
                    }
                }
                return Some(mid);
            } else if lt(ev.time, time) && tl.get(mid + 1).is_some_and(|n| gt(n.time, time)) {
                return Some(mid);
            } else if gt(ev.time, time) {
                end = mid;
            } else {
                beginning = mid + 1;
            }
        }
        None
    }

    fn tone_get(&self, time: f64) -> Option<ToneEv> {
        self.search(time).map(|i| self.tone.buf[i])
    }

    fn tone_get_after(&self, time: f64) -> Option<ToneEv> {
        let next = self.search(time).map_or(0, |i| i + 1);
        (next < self.tone.len).then(|| self.tone.buf[next])
    }

    fn tone_add(&mut self, ev: ToneEv) {
        let at = self.search(ev.time).map_or(0, |i| i + 1);
        if !self.tone.insert(at, ev) {
            // Full: drop the oldest event (Tone itself keeps 1000; queries are at ≥ now).
            self.tone.remove_front(1);
            let at = self.search(ev.time).map_or(0, |i| i + 1);
            self.tone.insert(at, ev);
        }
    }

    /// `Timeline.cancel(after)`.
    fn tone_cancel(&mut self, after: f64) {
        let len = self.tone.len;
        if len > 1 {
            match self.search(after) {
                Some(mut index) => {
                    if eq(self.tone.buf[index].time, after) {
                        for i in (0..=index).rev() {
                            if eq(self.tone.buf[i].time, after) {
                                index = i;
                            } else {
                                break;
                            }
                        }
                        self.tone.truncate(index);
                    } else {
                        self.tone.truncate(index + 1);
                    }
                }
                None => self.tone.truncate(0),
            }
        } else if len == 1 && gte(self.tone.buf[0].time, after) {
            self.tone.truncate(0);
        }
    }

    /// `Param.getValueAtTime` (set / linear / exponential events).
    pub fn get_value_at_time(&self, time: f64) -> f64 {
        let t = time.max(0.0);
        let after = self.tone_get_after(t);
        let Some(before) = self.tone_get(t) else {
            return self.initial;
        };
        match after {
            Some(a) if a.kind == Kind::Linear => {
                before.value
                    + (a.value - before.value) * ((t - before.time) / (a.time - before.time))
            }
            Some(a) if a.kind == Kind::Exp => {
                before.value
                    * (a.value / before.value).powf((t - before.time) / (a.time - before.time))
            }
            _ => before.value,
        }
    }

    // ---------------------------------------------------------------- Chromium timeline

    fn nat_insert(&mut self, kind: Kind, value: f32, time: f64, cx: Clock) {
        if self.nat.len == 0 && kind != Kind::Set {
            // A ramp with nothing before it starts from the current value now.
            self.nat_insert(Kind::Set, self.intrinsic, cx.now, cx);
        }
        if self.nat.len == CAP {
            self.prune_native(cx);
        }
        if self.nat.len == CAP {
            self.collapse_native();
        }
        let at = self.nat.as_slice().partition_point(|e| e.time <= time);
        let ok = self.nat.insert(
            at,
            NatEv {
                time,
                kind,
                value,
                new: true,
            },
        );
        debug_assert!(ok, "AutoParam: native timeline full");
    }

    /// `InsertEvent`'s pruning of events that are over at the current frame.
    fn prune_native(&mut self, cx: Clock) {
        let frame = (cx.now * cx.sr).round() as u64;
        let ev = self.nat.as_slice();
        let mut skipped = 0;
        for i in 0..ev.len() {
            if is_event_current(&ev[i], ev.get(i + 1), frame, cx.sr) {
                break;
            }
            skipped += 1;
        }
        self.nat.remove_front(skipped);
    }

    /// Last resort when full: merge consecutive set events at the same time (equivalent values).
    fn collapse_native(&mut self) {
        let mut out = 0;
        for i in 0..self.nat.len {
            let e = self.nat.buf[i];
            if out > 0 {
                let prev = self.nat.buf[out - 1];
                if prev.kind == Kind::Set && e.kind == Kind::Set && prev.time == e.time {
                    self.nat.buf[out - 1] = e;
                    continue;
                }
            }
            self.nat.buf[out] = e;
            out += 1;
        }
        self.nat.len = out;
    }

    fn nat_cancel(&mut self, time: f64, cx: Clock) {
        let t = time.max(cx.now);
        let at = self.nat.as_slice().iter().position(|e| e.time >= t);
        if let Some(at) = at {
            self.nat.truncate(at);
        }
    }

    // ---------------------------------------------------------------- Tone API

    /// `Param.setValueAtTime`.
    pub fn set_value_at_time(&mut self, value: f64, time: f64, cx: Clock) {
        self.tone_add(ToneEv {
            time,
            kind: Kind::Set,
            value,
        });
        self.nat_insert(Kind::Set, value as f32, time, cx);
    }

    /// `Param.linearRampToValueAtTime`.
    pub fn linear_ramp_to_value_at_time(&mut self, value: f64, time: f64, cx: Clock) {
        self.tone_add(ToneEv {
            time,
            kind: Kind::Linear,
            value,
        });
        self.nat_insert(Kind::Linear, value as f32, time, cx);
    }

    /// `Param.exponentialRampToValueAtTime` (a 0 target becomes `_minOutput`, 1e-7).
    pub fn exponential_ramp_to_value_at_time(&mut self, value: f64, time: f64, cx: Clock) {
        let value = if eq(value, 0.0) { 1e-7 } else { value };
        self.tone_add(ToneEv {
            time,
            kind: Kind::Exp,
            value,
        });
        self.nat_insert(Kind::Exp, value as f32, time, cx);
    }

    /// `Param.cancelScheduledValues`.
    pub fn cancel_scheduled_values(&mut self, time: f64, cx: Clock) {
        self.tone_cancel(time);
        self.nat_cancel(time, cx);
    }

    /// `Param.cancelAndHoldAtTime` (Tone's own implementation).
    pub fn cancel_and_hold_at_time(&mut self, time: f64, cx: Clock) {
        let value = self.get_value_at_time(time);
        let before = self.tone_get(time);
        let after = self.tone_get_after(time);
        if before.is_some_and(|b| eq(b.time, time)) {
            if let Some(a) = after {
                self.nat_cancel(a.time, cx);
                self.tone_cancel(a.time);
            } else {
                // Native cancelAndHoldAtTime: nothing to cancel after a set/ramp.
                self.tone_cancel(time + cx.sample_time());
            }
        } else if let Some(a) = after {
            self.nat_cancel(a.time, cx);
            self.tone_cancel(a.time);
            match a.kind {
                Kind::Linear => self.linear_ramp_to_value_at_time(value, time, cx),
                Kind::Exp => self.exponential_ramp_to_value_at_time(value, time, cx),
                Kind::Set => {}
            }
        }
        self.tone_add(ToneEv {
            time,
            kind: Kind::Set,
            value,
        });
        self.nat_insert(Kind::Set, value as f32, time, cx);
    }

    /// `Param.setRampPoint`.
    fn set_ramp_point(&mut self, time: f64, cx: Clock) {
        let mut current = self.get_value_at_time(time);
        self.cancel_and_hold_at_time(time, cx);
        if current == 0.0 {
            current = 1e-7;
        }
        self.set_value_at_time(current, time, cx);
    }

    /// `Param.rampTo(value, ramp)` for linear units (gain, audioRange): `linearRampTo` from now.
    pub fn linear_ramp_to(&mut self, value: f64, ramp: f64, cx: Clock) {
        self.set_ramp_point(cx.now, cx);
        self.linear_ramp_to_value_at_time(value, cx.now + ramp, cx);
    }

    /// `Param.rampTo(value, ramp)` for frequency units: `exponentialRampTo` from now.
    pub fn exponential_ramp_to(&mut self, value: f64, ramp: f64, cx: Clock) {
        self.set_ramp_point(cx.now, cx);
        self.exponential_ramp_to_value_at_time(value, cx.now + ramp, cx);
    }

    /// `param.value = v` (Tone: cancel from now, set at now).
    pub fn set_value(&mut self, value: f64, cx: Clock) {
        self.cancel_scheduled_values(cx.now, cx);
        self.set_value_at_time(value, cx.now, cx);
    }

    /// Drop Tone-mirror events that no query at or after `now` can see.
    pub fn prune_tone(&mut self, now: f64) {
        if self.tone.len > CAP / 2
            && let Some(i) = self.search(now - 2.0 * EPSILON)
        {
            self.tone.remove_front(i);
        }
    }

    // ---------------------------------------------------------------- rendering

    /// `HasSampleAccurateValues` for a param without input connections.
    fn has_sample_accurate(&self, frame: u64, sr: f64) -> bool {
        let ev = self.nat.as_slice();
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

    fn clip(&self, v: f32) -> f32 {
        v.clamp(self.min, self.max)
    }

    /// Render one quantum (`out.len()` frames from context frame `frame`) the way the owning
    /// node does: sample-accurate values when the timeline needs them, else the k-rate value
    /// (`Value()`) for the whole quantum. Returns whether the quantum was sample-accurate.
    pub fn render(&mut self, frame: u64, sr: f64, out: &mut [f32]) -> bool {
        if out.is_empty() {
            return false;
        }
        if self.has_sample_accurate(frame, sr) {
            self.values_for_frame_range(frame, out, sr);
            let last = out[out.len() - 1];
            self.intrinsic = self.clip(last);
            true
        } else {
            let mut v = self.intrinsic;
            if self.nat.len > 0 && frame as f64 / sr >= self.nat.buf[0].time {
                let mut one = [0f32; 1];
                self.values_for_frame_range(frame, &mut one, sr);
                v = one[0];
            }
            self.intrinsic = self.clip(v);
            out.fill(self.intrinsic);
            false
        }
    }

    /// True when the param holds one value for the foreseeable future (no events left).
    pub fn is_idle(&self) -> bool {
        self.nat.len == 0
    }

    /// `ValuesForFrameRange` (values clipped to the nominal range).
    fn values_for_frame_range(&mut self, start: u64, out: &mut [f32], sr: f64) {
        let default = self.intrinsic;
        self.values_impl(start, out, sr, default);
        for v in out.iter_mut() {
            *v = self.clip(*v);
        }
    }

    fn clamp_new(&mut self, current_time: f64) {
        let mut clamped = false;
        for e in &mut self.nat.buf[..self.nat.len] {
            if e.new && e.time < current_time {
                e.time = current_time;
                clamped = true;
            }
            e.new = false;
        }
        if clamped {
            // Stable insertion sort by time.
            let s = &mut self.nat.buf[..self.nat.len];
            for i in 1..s.len() {
                let mut j = i;
                while j > 0 && s[j].time < s[j - 1].time {
                    s.swap(j, j - 1);
                    j -= 1;
                }
            }
        }
    }

    /// `ValuesForFrameRangeImpl` for set / linear / exponential events.
    fn values_impl(&mut self, start: u64, out: &mut [f32], sr: f64, default: f32) {
        let n = out.len();
        let end = start + n as u64;
        if self.nat.len == 0 || end as f64 / sr <= self.nat.buf[0].time {
            out.fill(default);
            return;
        }
        if self.nat.as_slice().iter().any(|e| e.new) {
            self.clamp_new(start as f64 / sr);
        }
        // HandleAllEventsInThePast.
        let current_time = start as f64 / sr;
        if self.nat.buf[self.nat.len - 1].time + 1.5 * QUANTUM / sr < current_time {
            out.fill(default);
            self.nat.len = 0;
            return;
        }
        // HandleFirstEvent.
        let mut current_frame = start;
        let mut w = 0usize;
        let first_time = self.nat.buf[0].time;
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
        let mut skipped = 0;
        let len = self.nat.len;
        let mut i = 0;
        while i < len && w < n {
            let ev = self.nat.buf[i];
            let next = (i + 1 < len).then(|| self.nat.buf[i + 1]);
            if !is_event_current(&ev, next.as_ref(), current_frame, sr) {
                skipped += 1;
                i += 1;
                continue;
            }
            let (value1, time1) = (ev.value, ev.time);
            let (value2, time2, next_kind) = match next {
                Some(nx) => (nx.value, nx.time, Some(nx.kind)),
                None => (value1, end as f64 / sr + 1.0, None),
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
                    let k: f32 = if delta_time <= f32::MIN_POSITIVE as f64 {
                        0.0
                    } else {
                        (1.0 / delta_time) as f32
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
                        let multiplier = ((value2 / value1) as f64).powf(1.0 / num_sample_frames);
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
                _ => {
                    current_frame = fill_to_end_frame;
                    value = ev.value;
                    out[w..fill_to].fill(value);
                    w = fill_to;
                }
            }
            i += 1;
        }
        if skipped > 0 {
            self.nat.remove_front(skipped);
        }
        out[w..].fill(value);
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
            p.render(q as u64 * 128, SR, chunk);
        }
        out
    }

    #[test]
    fn gain_envelope_hold_and_linear_fade() {
        let mut g = AutoParam::new(1.0, 0.0, f32::MIN, f32::MAX, cx(0.0));
        g.set_value_at_time(0.8, 100.0 / SR, cx(0.0));
        g.set_value_at_time(0.8, 200.0 / SR, cx(0.0));
        g.linear_ramp_to_value_at_time(0.0, 300.0 / SR, cx(0.0));
        let v = render(&mut g, 512);
        assert_eq!(v[99], 0.0);
        assert_eq!(v[100], 0.8);
        assert_eq!(v[200], 0.8);
        assert!((v[250] - 0.4).abs() < 1e-6);
        assert_eq!(v[300], 0.0);
        assert_eq!(v[511], 0.0);
    }

    #[test]
    fn tone_cancel_and_hold_mid_ramp_keeps_the_level() {
        let mut g = AutoParam::new(1.0, 0.0, f32::MIN, f32::MAX, cx(0.0));
        g.set_value_at_time(1.0, 0.0, cx(0.0));
        g.linear_ramp_to_value_at_time(0.0, 1.0, cx(0.0));
        assert!((g.get_value_at_time(0.25) - 0.75).abs() < 1e-12);
        g.cancel_and_hold_at_time(0.25, cx(0.0));
        g.linear_ramp_to_value_at_time(0.0, 0.5, cx(0.0));
        assert!((g.get_value_at_time(0.25) - 0.75).abs() < 1e-12);
        assert!((g.get_value_at_time(0.375) - 0.375).abs() < 1e-12);
        assert_eq!(g.get_value_at_time(0.75), 0.0);
    }

    #[test]
    fn set_ramp_point_replaces_zero_by_min_output() {
        let mut p = AutoParam::new(0.0, 0.0, -1.0, 1.0, cx(0.0));
        p.set_value(0.0, cx(0.0));
        p.linear_ramp_to(1.0, 0.02, cx(0.5));
        assert_eq!(p.get_value_at_time(0.5), 1e-7);
        assert!((p.get_value_at_time(0.51) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn events_in_the_past_are_clamped_and_cleared() {
        let mut g = AutoParam::new(1.0, 0.25, f32::MIN, f32::MAX, cx(0.0));
        let v = render(&mut g, 1024);
        assert!(v.iter().all(|&x| x == 0.25));
        assert!(g.is_idle());
        // A set "at 0" added later is clamped to the current time.
        g.set_value_at_time(0.5, 0.0, cx(1024.0 / SR));
        let mut q = [0f32; 128];
        g.render(1024, SR, &mut q);
        assert!(q.iter().all(|&x| x == 0.5));
    }

    #[test]
    fn exponential_ramp_reaches_its_target() {
        let mut f = AutoParam::new(1.0, 1000.0, 0.0, 24000.0, cx(0.0));
        f.exponential_ramp_to(250.0, 0.002, cx(0.0));
        let v = render(&mut f, 256);
        assert_eq!(v[0], 1000.0);
        assert!((v[48] as f64 - 500.0).abs() < 0.01, "{}", v[48]);
        assert_eq!(v[96], 250.0);
        assert_eq!(v[255], 250.0);
    }
}
