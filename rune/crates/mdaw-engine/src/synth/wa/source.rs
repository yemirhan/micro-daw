//! `Tone.Source` start / stop bookkeeping (its `StateTimeline`) and the `OneShotSource` gain gate
//! every native source plays through.
//!
//! - `start(t)` (t clamped to now): if the state at `t` is already "started" Tone asserts the new
//!   start is strictly later than the last start (else throws: the voices' `startSource` then
//!   retries 1 ms later), cancels the timeline from `t` and RESTARTS (the running native node
//!   continues, its pending stop is cancelled); otherwise it records "started" and starts a NEW
//!   native node.
//! - `stop(t)` only acts when the state at `t` is "started" or a start follows `t`.
//!
//! The gate is the `OneShotSource` gain (0 before the node starts): `setValueAtTime(1, t)` on
//! start; `cancelStop` = `cancelScheduledValues(startTime + 1/sr)`; a stop is `cancelStop`,
//! `cancelAndHoldAtTime(t)`, `setValueAtTime(0, t)`. Successive native nodes of one Tone source
//! never sound at once (a new node only starts once the old one's gate is 0), so they share one
//! gate param here.

use super::param::{AutoParam, Clock};
use super::tone_tl::{eq, gt};

const CAP: usize = 16;

/// What `Source.start` did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Started {
    /// A new native node starts at this time.
    New(f64),
    /// The running node continues (`_restart`).
    Restart(f64),
}

#[derive(Debug, Clone)]
pub(crate) struct SourceState {
    /// (time, started) events, increasing.
    ev: [(f64, bool); CAP],
    len: usize,
}

impl Default for SourceState {
    fn default() -> Self {
        let mut s = Self {
            ev: [(0.0, false); CAP],
            len: 0,
        };
        s.push(0.0, false);
        s
    }
}

impl SourceState {
    fn push(&mut self, time: f64, started: bool) {
        if self.len == CAP {
            self.ev.copy_within(1.., 0);
            self.len -= 1;
        }
        self.ev[self.len] = (time, started);
        self.len += 1;
    }

    /// Index of the event in effect at `time` (Tone `_search`, 1e-6 tolerance).
    fn search(&self, time: f64) -> Option<usize> {
        (0..self.len)
            .rev()
            .find(|&i| self.ev[i].0 <= time || eq(self.ev[i].0, time))
    }

    pub fn started_at(&self, time: f64) -> bool {
        self.search(time).is_some_and(|i| self.ev[i].1)
    }

    /// `Timeline.cancel(after)`.
    fn cancel(&mut self, after: f64) {
        if self.len > 1 {
            match self.search(after) {
                Some(mut index) => {
                    if eq(self.ev[index].0, after) {
                        while index > 0 && eq(self.ev[index - 1].0, after) {
                            index -= 1;
                        }
                        self.len = index;
                    } else {
                        self.len = index + 1;
                    }
                }
                None => self.len = 0,
            }
        } else if self.len == 1 && (self.ev[0].0 >= after || eq(self.ev[0].0, after)) {
            self.len = 0;
        }
    }

    /// `Source.start(time)`. `Err` = Tone's "Start time must be strictly greater than previous
    /// start time" assertion.
    pub fn start(&mut self, time: f64, now: f64) -> Result<Started, ()> {
        let t = time.max(now);
        if self.started_at(t) {
            let last = self.search(t).map_or(0.0, |i| self.ev[i].0);
            if !gt(t, last) {
                return Err(());
            }
            self.cancel(t);
            self.push(t, true);
            // restart(): the state at t is "started" → cancel(t) and `_restart(t)`.
            self.cancel(t);
            Ok(Started::Restart(t))
        } else {
            self.push(t, true);
            Ok(Started::New(t))
        }
    }

    /// `Source.stop(time)`: Some(t) when `_stop(t)` runs.
    pub fn stop(&mut self, time: f64, now: f64) -> Option<f64> {
        let t = time.max(now);
        let next_start = self
            .search(t)
            .is_some_and(|i| self.ev[i..self.len].iter().any(|e| e.1));
        if self.started_at(t) || next_start {
            self.cancel(t);
            self.push(t, false);
            Some(t)
        } else {
            None
        }
    }
}

/// The `OneShotSource` gain gate of a Tone source's native node(s).
#[derive(Debug, Clone)]
pub(crate) struct Gate {
    pub param: AutoParam,
    /// `_startTime` of the current native node.
    start_time: f64,
}

impl Gate {
    pub fn new(cx: Clock) -> Self {
        Self {
            param: AutoParam::gain(0.0, cx),
            start_time: -1.0,
        }
    }

    /// `_startGain(t)` of a new node.
    pub fn start(&mut self, t: f64, cx: Clock) {
        self.start_time = t.max(cx.now);
        self.param.set_value_at_time(1.0, t, cx);
    }

    /// `cancelStop()`.
    pub fn cancel_stop(&mut self, cx: Clock) {
        if self.start_time >= 0.0 {
            self.param
                .cancel_scheduled_values(self.start_time + 1.0 / cx.sr, cx);
        }
    }

    /// `_stopGain(t)`.
    pub fn stop(&mut self, t: f64, cx: Clock) {
        self.cancel_stop(cx);
        self.param.cancel_and_hold_at_time(t, cx);
        self.param.set_value_at_time(0.0, t, cx);
    }
}

/// Try `start`, then `start(t + 1 ms)` (the voices' `startSource`); None if both throw.
pub(crate) fn start_source(state: &mut SourceState, time: f64, now: f64) -> Option<Started> {
    state
        .start(time, now)
        .or_else(|_| state.start(time + 0.001, now))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restart_while_running_and_new_after_stop() {
        let mut s = SourceState::default();
        assert_eq!(s.start(1.0, 0.0), Ok(Started::New(1.0)));
        assert_eq!(s.stop(2.0, 0.0), Some(2.0));
        // Re-attack before the scheduled stop: restart (stop cancelled).
        assert_eq!(s.start(1.5, 0.0), Ok(Started::Restart(1.5)));
        assert!(s.started_at(3.0));
        assert_eq!(s.stop(3.0, 0.0), Some(3.0));
        assert_eq!(s.start(4.0, 0.0), Ok(Started::New(4.0)));
        // Same-time restart throws.
        assert_eq!(s.start(4.0, 0.0), Err(()));
        assert_eq!(
            start_source(&mut s, 4.0, 0.0),
            Some(Started::Restart(4.001))
        );
        // Stopping a stopped source does nothing.
        let mut s = SourceState::default();
        assert_eq!(s.stop(1.0, 0.0), None);
    }
}
