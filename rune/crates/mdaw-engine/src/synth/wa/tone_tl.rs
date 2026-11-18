//! Tone.js's JS-side mirror of an AudioParam's automation (`Tone.Param._events`, a
//! `Tone.Timeline`), which Tone consults for `getValueAtTime`, `cancelAndHoldAtTime` and
//! `setRampPoint` — and so for every envelope decision (`Envelope.triggerAttack` shortens the
//! attack by the current value, `triggerRelease` only fires above 0, holds use the mirror's value).
//!
//! f64 values, Tone's 1e-6 comparisons (`EQ`, `LT`, `GT`), an event added at a time that already
//! holds events goes after them. Fixed capacity: when full the oldest event is dropped (queries
//! only ever look at times ≥ the current time, and [`ToneTimeline::prune`] keeps the event in
//! effect at `now`).

pub(crate) const EPSILON: f64 = 1e-6;
/// Events per mirror timeline.
pub(crate) const CAP: usize = 48;

pub(crate) fn eq(a: f64, b: f64) -> bool {
    (a - b).abs() < EPSILON
}
pub(crate) fn gt(a: f64, b: f64) -> bool {
    a > b + EPSILON
}
pub(crate) fn lt(a: f64, b: f64) -> bool {
    a + EPSILON < b
}
pub(crate) fn gte(a: f64, b: f64) -> bool {
    gt(a, b) || eq(a, b)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Kind {
    Set,
    Linear,
    Exp,
    /// `setTargetAtTime` with its time constant.
    Target(f64),
}

impl Kind {
    pub(crate) fn is_ramp(self) -> bool {
        matches!(self, Kind::Linear | Kind::Exp)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ToneEv {
    pub time: f64,
    pub kind: Kind,
    pub value: f64,
}

const EMPTY: ToneEv = ToneEv {
    time: 0.0,
    kind: Kind::Set,
    value: 0.0,
};

/// `Tone.Timeline` of param events.
#[derive(Debug, Clone)]
pub(crate) struct ToneTimeline {
    buf: [ToneEv; CAP],
    len: usize,
}

impl Default for ToneTimeline {
    fn default() -> Self {
        Self {
            buf: [EMPTY; CAP],
            len: 0,
        }
    }
}

impl ToneTimeline {
    fn ev(&self) -> &[ToneEv] {
        &self.buf[..self.len]
    }

    /// `Timeline._search`: index of the last event at or before `time` (the last of several
    /// events "equal" to `time`), or None.
    fn search(&self, time: f64) -> Option<usize> {
        let tl = self.ev();
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

    /// `Timeline.get`.
    pub fn get(&self, time: f64) -> Option<ToneEv> {
        self.search(time).map(|i| self.buf[i])
    }

    /// `Timeline.getAfter`.
    pub fn get_after(&self, time: f64) -> Option<ToneEv> {
        let next = self.search(time).map_or(0, |i| i + 1);
        (next < self.len).then(|| self.buf[next])
    }

    /// `Timeline.getBefore`.
    fn get_before(&self, time: f64) -> Option<ToneEv> {
        let len = self.len;
        if len > 0 && self.buf[len - 1].time < time {
            return Some(self.buf[len - 1]);
        }
        match self.search(time) {
            Some(i) if i >= 1 => Some(self.buf[i - 1]),
            _ => None,
        }
    }

    /// `Timeline.add`.
    pub fn add(&mut self, ev: ToneEv) {
        if self.len == CAP {
            self.remove_front(1);
        }
        let at = self.search(ev.time).map_or(0, |i| i + 1);
        self.buf.copy_within(at..self.len, at + 1);
        self.buf[at] = ev;
        self.len += 1;
    }

    fn remove_front(&mut self, n: usize) {
        let n = n.min(self.len);
        self.buf.copy_within(n..self.len, 0);
        self.len -= n;
    }

    /// `Timeline.cancel(after)`.
    pub fn cancel(&mut self, after: f64) {
        let len = self.len;
        if len > 1 {
            match self.search(after) {
                Some(mut index) => {
                    if eq(self.buf[index].time, after) {
                        for i in (0..=index).rev() {
                            if eq(self.buf[i].time, after) {
                                index = i;
                            } else {
                                break;
                            }
                        }
                        self.len = index;
                    } else {
                        self.len = index + 1;
                    }
                }
                None => self.len = 0,
            }
        } else if len == 1 && gte(self.buf[0].time, after) {
            self.len = 0;
        }
    }

    /// Drop events no query at or after `now` can see (keeps the one in effect at `now` and the
    /// one before it, which a setTarget's value needs).
    pub fn prune(&mut self, now: f64) {
        if self.len > CAP / 2
            && let Some(i) = self.search(now - 2.0 * EPSILON)
            && i >= 1
        {
            self.remove_front(i - 1);
        }
    }

    /// `Param.getValueAtTime` (before `_toType`), `initial` = `_initialValue`.
    pub fn value_at(&self, time: f64, initial: f64) -> f64 {
        let t = time.max(0.0);
        let after = self.get_after(t);
        let Some(before) = self.get(t) else {
            return initial;
        };
        if let Kind::Target(constant) = before.kind
            && after.is_none_or(|a| a.kind == Kind::Set)
        {
            let previous = self.get_before(before.time).map_or(initial, |p| p.value);
            return before.value
                + (previous - before.value) * (-(t - before.time) / constant).exp();
        }
        match after {
            None => before.value,
            Some(a) if a.kind.is_ramp() => {
                let before_value = if matches!(before.kind, Kind::Target(_)) {
                    self.get_before(before.time).map_or(initial, |p| p.value)
                } else {
                    before.value
                };
                let x = (t - before.time) / (a.time - before.time);
                if a.kind == Kind::Linear {
                    before_value + (a.value - before_value) * x
                } else {
                    before_value * (a.value / before_value).powf(x)
                }
            }
            Some(_) => before.value,
        }
    }
}
