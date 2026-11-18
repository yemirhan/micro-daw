//! Note repeat inside the core (port of `src/services/NoteRepeat.ts`, inventory §2.8).
//!
//! - **Transport running** (pending or started, i.e. the engine is not 'stopped', count-in
//!   included): repeats fire on the transport grid of `rate·PPQ` ticks anchored at tick 0 (Tone
//!   `scheduleRepeat(cb, "<interval>i", 0)`), so the grid survives loop wraps. A grid line fires
//!   if its time is ≥ `firstTime − 1 ms`, `firstTime = press + (startTick − pressTick)·secPerTick`
//!   with `startTick = nextGridTick(pressTick + interval/2)`: the first line at least half a
//!   step after the press, then every line, across wraps.
//! - **Transport stopped**: free-running at `60/bpm·rate` seconds (bpm ≤ 0 → 120), on the grid of
//!   another held pad if one is repeating, else of this press: `nextFreeLine(press, ref, step)`.
//! - Velocity: the initial hit's. Release stops the pad's repeats at the release frame (R2-4).
//! - Restart of every held pad on enable, rate change and running ↔ stopped transitions.
//!
//! **Deviation (documented):** the TS needs a catch-up list (grid lines the Tone scheduler had
//! already passed inside its 100 ms look-ahead) and, when stopped, a 15 ms pump handing hits
//! out 40 ms ahead and skipping steps more than 30 ms late. The core evaluates the grid at the
//! exact frame it renders, so there is nothing to catch up, nothing to hand out ahead and no
//! timer to stall: the hits land on exactly the grid times the TS aims for (its intended
//! behaviour), and a release cancels every later repeat instead of leaving up to 40 ms of
//! already handed-out hits for the sampler to cancel.

use mdaw_model::{PAD_COUNT, PadIndex};

use crate::contract::PPQ;

const EPS: f64 = 1e-6;
/// Tolerance when comparing a grid time with the first planned repeat.
const FIRST_LINE_SLACK_SEC: f64 = 0.001;

/// `nextGridTick`: first multiple of `interval` at or after `tick`.
pub fn next_grid_tick(tick: f64, interval: f64) -> f64 {
    if interval <= 0.0 {
        return tick;
    }
    (tick / interval - EPS).ceil() * interval + 0.0
}

/// `planRepeat(...).startTick`: the first grid line at least half a step after the press.
pub fn repeat_start_tick(press_tick: f64, interval: f64) -> f64 {
    next_grid_tick(press_tick + interval / 2.0, interval)
}

/// `nextFreeLine`: first line of the `step` grid through `reference` at least half a step after
/// `press`.
pub fn next_free_line(press: f64, reference: f64, step: f64) -> f64 {
    if step <= 0.0 {
        return press;
    }
    let k = ((press + step / 2.0 - reference) / step - EPS).ceil();
    reference + k * step
}

/// Repeat interval in transport ticks (`Math.round(rate·PPQ)`).
pub fn repeat_interval_ticks(rate_beats: f64) -> i64 {
    (rate_beats * PPQ as f64).round().max(1.0) as i64
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Mode {
    Idle,
    /// On the transport grid; lines before `first_time` are skipped.
    Transport {
        first_time: f64,
    },
    /// Free-running; next hit at `next`.
    Free {
        next: f64,
    },
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Held {
    pub velocity: f64,
    pub mode: Mode,
    /// Press order (TS Map insertion order).
    order: u64,
}

/// What the scheduler needs to know about the transport at a press / restart.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Clock {
    pub running: bool,
    pub time: f64,
    /// Transport ticks at `time` (what is heard).
    pub ticks: f64,
    pub bpm: f64,
}

pub(crate) struct NoteRepeat {
    pub enabled: bool,
    pub rate_beats: f64,
    pub held: [Option<Held>; PAD_COUNT],
    order: u64,
}

impl NoteRepeat {
    pub fn new() -> Self {
        Self {
            enabled: false,
            rate_beats: 0.25,
            held: [None; PAD_COUNT],
            order: 0,
        }
    }

    pub fn interval_ticks(&self) -> i64 {
        repeat_interval_ticks(self.rate_beats)
    }

    fn step_seconds(&self, bpm: f64) -> f64 {
        60.0 / if bpm > 0.0 { bpm } else { 120.0 } * self.rate_beats
    }

    /// `setEnabled` / `setRate` (a no-op setting leaves the pads alone).
    pub fn configure(&mut self, enabled: bool, rate_beats: f64, clock: Clock) {
        let rate_ok = rate_beats.is_finite() && rate_beats > 0.0;
        let rate_changed = rate_ok && rate_beats != self.rate_beats;
        if rate_ok {
            self.rate_beats = rate_beats;
        }
        if enabled != self.enabled {
            self.enabled = enabled;
            self.restart_all(clock);
        } else if rate_changed {
            self.restart_all(clock);
        }
    }

    /// A pad went down (after its normal hit).
    pub fn press(&mut self, pad: PadIndex, velocity: f64, clock: Clock) {
        if pad >= PAD_COUNT {
            return;
        }
        self.held[pad] = None;
        self.order += 1;
        self.held[pad] = Some(Held {
            velocity,
            mode: Mode::Idle,
            order: self.order,
        });
        if self.enabled {
            self.start(pad, clock);
        }
    }

    pub fn release(&mut self, pad: PadIndex) {
        if pad < PAD_COUNT {
            self.held[pad] = None;
        }
    }

    pub fn is_repeating(&self, pad: PadIndex) -> bool {
        self.held
            .get(pad)
            .and_then(|h| *h)
            .is_some_and(|h| h.mode != Mode::Idle)
    }

    /// Stop everything, then (if enabled) start the held pads again in press order.
    pub fn restart_all(&mut self, clock: Clock) {
        for h in self.held.iter_mut().flatten() {
            h.mode = Mode::Idle;
        }
        if !self.enabled {
            return;
        }
        let mut last = 0;
        while let Some(pad) = (0..PAD_COUNT)
            .filter(|p| self.held[*p].is_some_and(|h| h.order > last))
            .min_by_key(|p| self.held[*p].map_or(0, |h| h.order))
        {
            last = self.held[pad].map_or(0, |h| h.order);
            self.start(pad, clock);
        }
    }

    fn start(&mut self, pad: PadIndex, clock: Clock) {
        let mode = if clock.running {
            let interval = self.interval_ticks() as f64;
            let start_tick = repeat_start_tick(clock.ticks, interval);
            let sec_per_tick = 60.0 / (clock.bpm * PPQ as f64);
            Mode::Transport {
                first_time: clock.time + (start_tick - clock.ticks) * sec_per_tick
                    - FIRST_LINE_SLACK_SEC,
            }
        } else {
            let step = self.step_seconds(clock.bpm);
            let reference = self
                .held
                .iter()
                .enumerate()
                .filter(|(p, _)| *p != pad)
                .find_map(|(_, h)| match h.map(|h| h.mode) {
                    Some(Mode::Free { next }) => Some(next),
                    _ => None,
                })
                .unwrap_or(clock.time);
            Mode::Free {
                next: next_free_line(clock.time, reference, step),
            }
        };
        if let Some(h) = &mut self.held[pad] {
            h.mode = mode;
        }
    }

    /// The earliest free-running hit before `tb`: (pad, velocity, time).
    pub fn next_free_before(&self, tb: f64) -> Option<(PadIndex, f64, f64)> {
        self.held
            .iter()
            .enumerate()
            .filter_map(|(p, h)| match h.map(|h| (h.velocity, h.mode)) {
                Some((v, Mode::Free { next })) if next < tb => Some((p, v, next)),
                _ => None,
            })
            .min_by(|a, b| a.2.total_cmp(&b.2))
    }

    /// Move a free-running pad to its next step (tempo of now, like the TS pump).
    pub fn advance_free(&mut self, pad: PadIndex, bpm: f64) {
        let step = self.step_seconds(bpm);
        if let Some(Held {
            mode: Mode::Free { next },
            ..
        }) = &mut self.held[pad]
        {
            *next += step;
        }
    }

    /// Pads repeating on the transport whose first line is due by `time`: (pad, velocity).
    pub fn transport_due(&self, time: f64) -> impl Iterator<Item = (PadIndex, f64)> + '_ {
        self.held.iter().enumerate().filter_map(move |(p, h)| {
            match h.map(|h| (h.velocity, h.mode)) {
                Some((v, Mode::Transport { first_time })) if time >= first_time => Some((p, v)),
                _ => None,
            }
        })
    }

    pub fn any_transport(&self) -> bool {
        self.held
            .iter()
            .flatten()
            .any(|h| matches!(h.mode, Mode::Transport { .. }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Ports of NoteRepeat.test.ts "note repeat scheduling math" / "nextFreeLine".
    #[test]
    fn converts_rates_to_ticks() {
        let t: Vec<i64> = [1.0, 0.5, 1.0 / 3.0, 0.25, 1.0 / 6.0, 0.125]
            .iter()
            .map(|r| repeat_interval_ticks(*r))
            .collect();
        assert_eq!(t, [192, 96, 64, 48, 32, 24]);
    }

    #[test]
    fn next_grid_line_counts_a_line_itself() {
        assert_eq!(next_grid_tick(0.0, 48.0), 0.0);
        assert_eq!(next_grid_tick(1.0, 48.0), 48.0);
        assert_eq!(next_grid_tick(48.0, 48.0), 48.0);
        assert_eq!(next_grid_tick(95.9, 48.0), 96.0);
    }

    #[test]
    fn starts_at_least_half_a_step_after_the_press() {
        assert_eq!(repeat_start_tick(90.0, 48.0), 144.0);
        assert_eq!(repeat_start_tick(50.0, 48.0), 96.0);
        assert_eq!(repeat_start_tick(72.0, 48.0), 96.0);
    }

    #[test]
    fn next_free_line_starts_one_step_after_and_joins_a_grid() {
        assert!((next_free_line(10.0, 10.0, 0.125) - 10.125).abs() < 1e-12);
        assert!((next_free_line(10.2, 10.125, 0.125) - 10.375).abs() < 1e-12);
        assert!((next_free_line(10.13, 10.125, 0.125) - 10.25).abs() < 1e-12);
    }

    fn stopped(time: f64) -> Clock {
        Clock {
            running: false,
            time,
            ticks: 0.0,
            bpm: 120.0,
        }
    }

    #[test]
    fn modes_follow_enable_rate_and_transport() {
        let mut nr = NoteRepeat::new();
        nr.press(3, 1.0, stopped(10.0));
        assert!(!nr.is_repeating(3), "does nothing while disabled");
        nr.configure(true, 0.25, stopped(10.0));
        assert_eq!(nr.held[3].unwrap().mode, Mode::Free { next: 10.125 });
        nr.press(5, 0.4, stopped(10.2));
        assert_eq!(
            nr.held[5].unwrap().mode,
            Mode::Free { next: 10.375 },
            "joins the grid"
        );
        let running = Clock {
            running: true,
            time: 11.0,
            ticks: 50.0,
            bpm: 120.0,
        };
        nr.restart_all(running);
        assert!(nr.any_transport() && nr.next_free_before(f64::INFINITY).is_none());
        nr.configure(true, 1.0 / 3.0, running);
        assert_eq!(nr.interval_ticks(), 64);
        nr.configure(false, 1.0 / 3.0, running);
        assert!(
            !nr.is_repeating(3) && nr.held[3].is_some(),
            "held, not repeating"
        );
        nr.release(3);
        assert!(nr.held[3].is_none());
    }
}
