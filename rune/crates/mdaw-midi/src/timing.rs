//! midir timestamps → wall-clock instants → engine audio time.
//!
//! midir stamps each message in microseconds from a backend-specific origin (CoreMIDI: host time
//! since boot; ALSA: the input queue's start; WinMM: `midiInStart`). [`TimestampMapper`] learns
//! that origin against [`Instant`] with a **windowed minimum-offset filter**: a message is never
//! delivered before it happened, so `arrival − timestamp` is at least the true origin offset,
//! and its minimum over recent messages converges to it (plus the smallest delivery delay).
//! Event instants are then `timestamp + offset`, clamped to the arrival instant: bursts
//! delivered late keep their original spacing.
//!
//! - **Drift.** The device clock and [`Instant`] drift apart (≈100 ppm is common). A lifetime
//!   minimum only follows a clock that runs fast; one that runs slow would map events earlier
//!   and earlier (0.36 s after an hour at 100 ppm). The minimum is therefore taken over the
//!   last one to two [`DRIFT_WINDOW`]s (two buckets: the current window and the previous one),
//!   so it follows drift both ways with an error of at most `2 × DRIFT_WINDOW × drift`
//!   (0.4 ms at 100 ppm) plus the best delivery delay in that window.
//! - **Re-anchoring.** If a message's offset exceeds the learned one by more than
//!   [`REANCHOR_AFTER`] (a backend clock reset, or a stall that long) the filter restarts from
//!   it: no event is ever pushed into the far past. After more than two windows of silence it
//!   restarts too. [`TimestampMapper::reset`] restarts it (every new connection: ALSA / WinMM
//!   origins are per connection).
//! - **Audio time.** [`audio_time`] maps the instant through the core's published clock anchor
//!   (`ClockSnapshot::map_event`): one device block (the largest seen) + one render quantum
//!   after the instant, clamped to `[next quantum, extrapolated now + that latency]`.
//!   - **On time** (delivered within that budget): constant latency, no jitter, relative
//!     timing kept.
//!   - **Late** (delivered after the budget, e.g. a burst the OS held back): as soon as
//!     possible (the next quantum). Its lateness is reported ([`EventTime::late_by`]) and
//!     counted ([`crate::host::MidiStats`]). A late burst therefore loses its spacing: the
//!     events play together at the next quantum rather than late by a constant.
//!   - **Device stalled** (no callback for more than two periods): capped at two periods past
//!     the last callback, so nothing is stamped into the far future.
//!
//!   Without an anchor (no device callback yet, offline) the event plays "now" (None).

use std::time::{Duration, Instant};

use mdaw_engine::core::{EventTime, Shared};

/// See the module docs.
pub const REANCHOR_AFTER: Duration = Duration::from_millis(250);
/// See the module docs.
pub const DRIFT_WINDOW: Duration = Duration::from_secs(2);

/// See the module docs.
#[derive(Debug, Clone)]
pub struct TimestampMapper {
    epoch: Instant,
    /// Minimum `arrival − timestamp` (ns, relative to `epoch`) in the current window.
    cur: Option<i128>,
    /// The same over the previous window.
    prev: Option<i128>,
    /// Arrival (ns) the current window started at.
    window_start: i128,
}

impl Default for TimestampMapper {
    fn default() -> Self {
        Self::new()
    }
}

impl TimestampMapper {
    pub fn new() -> Self {
        Self {
            epoch: Instant::now(),
            cur: None,
            prev: None,
            window_start: 0,
        }
    }

    /// Forget the learned origin (new connection).
    pub fn reset(&mut self) {
        self.cur = None;
        self.prev = None;
    }

    /// The learned offset (ns): the minimum over the current and the previous window.
    fn offset(&self) -> Option<i128> {
        match (self.prev, self.cur) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// The instant the message stamped `timestamp_us` happened, given that it arrived at
    /// `arrival`.
    pub fn map(&mut self, timestamp_us: u64, arrival: Instant) -> Instant {
        let arrival_ns = arrival.saturating_duration_since(self.epoch).as_nanos() as i128;
        let ts_ns = i128::from(timestamp_us) * 1000;
        let candidate = arrival_ns - ts_ns;
        let window = DRIFT_WINDOW.as_nanos() as i128;
        let elapsed = arrival_ns - self.window_start;
        let restart = self
            .offset()
            .is_none_or(|o| candidate - o > REANCHOR_AFTER.as_nanos() as i128)
            || elapsed >= 2 * window;
        if restart {
            self.prev = None;
            self.cur = None;
            self.window_start = arrival_ns;
        } else if elapsed >= window {
            self.prev = self.cur.take();
            self.window_start += window;
        }
        self.cur = Some(self.cur.map_or(candidate, |c| c.min(candidate)));
        let offset = self.offset().unwrap_or(candidate);
        let at_ns = (ts_ns + offset).clamp(0, arrival_ns);
        self.epoch + Duration::from_nanos(at_ns as u64)
    }
}

/// The context time to play an event that happened at `instant`, and its lateness (None: no
/// device clock yet, so "as soon as possible").
pub fn audio_time(shared: &Shared, instant: Instant) -> Option<EventTime> {
    let snap = shared.snapshot();
    snap.anchor?;
    Some(snap.map_event(instant))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_late_burst_keeps_its_spacing() {
        let mut m = TimestampMapper::new();
        let t0 = m.epoch + Duration::from_millis(100);
        // calibrating message delivered 1 ms after it happened at ts = 5 s
        let a = m.map(5_000_000, t0 + Duration::from_millis(1));
        assert_eq!(a, t0 + Duration::from_millis(1));
        // three messages 10 ms apart, delivered together 40 ms later
        let arrival = t0 + Duration::from_millis(41);
        let got: Vec<Instant> = (1..=3)
            .map(|k| m.map(5_000_000 + k * 10_000, arrival))
            .collect();
        for (k, g) in got.iter().enumerate() {
            let want = t0 + Duration::from_millis(1 + 10 * (k as u64 + 1));
            assert_eq!(*g, want);
        }
        // a faster delivery tightens the offset
        let b = m.map(5_100_000, t0 + Duration::from_micros(100_500));
        assert_eq!(b, t0 + Duration::from_micros(100_500));
        let c = m.map(5_200_000, t0 + Duration::from_millis(250));
        assert_eq!(c, t0 + Duration::from_micros(200_500));
    }

    /// Phase-5 review finding 6: an hour of messages from a device clock 100 ppm slow / fast
    /// (one every 50 ms, delivery delay 0.2–2.2 ms): the mapping error stays below 1 ms. The
    /// lifetime minimum it replaces was 360 ms early after the hour for the slow clock.
    #[test]
    fn follows_100_ppm_drift_both_ways() {
        for ppm in [-100.0f64, 100.0] {
            let mut m = TimestampMapper::new();
            let t0 = m.epoch + Duration::from_millis(5);
            let mut seed = 0x2545_f491u64;
            let mut worst = 0f64;
            let mut worst_lifetime = 0f64;
            let mut lifetime_min = i128::MAX;
            for k in 0..72_000u64 {
                let real = k as f64 * 0.05; // seconds since t0
                let ts_us = 1_000_000 + (real * (1.0 + ppm * 1e-6) * 1e6) as u64;
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                let delay = 0.0002 + (seed % 2000) as f64 * 1e-6;
                let happened = t0 + Duration::from_secs_f64(real);
                let arrival = happened + Duration::from_secs_f64(delay);
                let got = m.map(ts_us, arrival);
                let err = (got.saturating_duration_since(happened).as_secs_f64())
                    .max(happened.saturating_duration_since(got).as_secs_f64());
                // the lifetime-minimum filter, for the record
                let cand =
                    arrival.duration_since(m.epoch).as_nanos() as i128 - i128::from(ts_us) * 1000;
                lifetime_min = lifetime_min.min(cand);
                let old = m.epoch
                    + Duration::from_nanos((i128::from(ts_us) * 1000 + lifetime_min).max(0) as u64);
                let old_err = happened.saturating_duration_since(old).as_secs_f64();
                if k > 200 {
                    worst = worst.max(err);
                    worst_lifetime = worst_lifetime.max(old_err);
                }
            }
            assert!(worst < 0.001, "{ppm} ppm: worst error {worst}");
            eprintln!(
                "{ppm:+} ppm: windowed min worst {:.3} ms, lifetime min worst {:.1} ms",
                worst * 1e3,
                worst_lifetime * 1e3
            );
        }
    }

    #[test]
    fn clock_reset_reanchors_and_never_maps_into_the_future() {
        let mut m = TimestampMapper::new();
        let t0 = m.epoch + Duration::from_secs(1);
        m.map(9_000_000, t0);
        // the backend clock restarted at 0: re-anchor at the arrival
        let a = m.map(1_000, t0 + Duration::from_secs(2));
        assert_eq!(a, t0 + Duration::from_secs(2));
        let b = m.map(
            11_000,
            t0 + Duration::from_secs(2) + Duration::from_millis(15),
        );
        assert_eq!(b, t0 + Duration::from_secs(2) + Duration::from_millis(10));
        // a timestamp "after" its arrival is clamped to the arrival
        let mut m = TimestampMapper::new();
        let t = m.epoch + Duration::from_secs(1);
        m.map(1_000_000, t);
        let c = m.map(1_000_000 + 50_000, t + Duration::from_millis(1));
        assert!(c <= t + Duration::from_millis(1));
    }
}
