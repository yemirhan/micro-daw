//! midir timestamps → wall-clock instants → engine audio time.
//!
//! midir stamps each message in microseconds from a backend-specific origin (CoreMIDI: host time
//! since boot; ALSA: the input queue's start; WinMM: `midiInStart`). [`TimestampMapper`] learns
//! that origin against [`Instant`] with a **minimum-offset filter**: a message is never delivered
//! before it happened, so `arrival − timestamp` is at least the true origin offset, and its
//! minimum over the messages seen converges to it (plus the smallest delivery delay). Event
//! instants are then `timestamp + offset`, clamped to the arrival instant: bursts delivered late
//! keep their original spacing.
//!
//! - **Re-anchoring.** If a message's offset exceeds the learned one by more than
//!   [`REANCHOR_AFTER`] (a backend clock reset, or a stall that long) the filter restarts from
//!   it: no event is ever pushed into the far past. [`TimestampMapper::reset`] restarts it too
//!   (every new connection: ALSA / WinMM origins are per connection).
//! - **Audio time.** [`audio_time`] maps the instant through the core's published clock anchor
//!   (`ClockSnapshot::audio_time_for_instant`: one callback period + one render quantum after the
//!   instant, so every event gets the same latency and lands at or after the next quantum).
//!   Without an anchor (no device callback yet, offline) the event plays "now" (None).

use std::time::{Duration, Instant};

use mdaw_engine::core::Shared;

/// See the module docs.
pub const REANCHOR_AFTER: Duration = Duration::from_millis(250);

/// See the module docs.
#[derive(Debug, Clone)]
pub struct TimestampMapper {
    epoch: Instant,
    /// Learned `arrival − timestamp`, in ns relative to `epoch`.
    offset_ns: Option<i128>,
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
            offset_ns: None,
        }
    }

    /// Forget the learned origin (new connection).
    pub fn reset(&mut self) {
        self.offset_ns = None;
    }

    /// The instant the message stamped `timestamp_us` happened, given that it arrived at
    /// `arrival`.
    pub fn map(&mut self, timestamp_us: u64, arrival: Instant) -> Instant {
        let arrival_ns = arrival.saturating_duration_since(self.epoch).as_nanos() as i128;
        let ts_ns = i128::from(timestamp_us) * 1000;
        let candidate = arrival_ns - ts_ns;
        let offset = match self.offset_ns {
            Some(o) if candidate <= o => candidate,
            Some(o) if candidate - o <= REANCHOR_AFTER.as_nanos() as i128 => o,
            _ => candidate,
        };
        self.offset_ns = Some(offset);
        let at_ns = (ts_ns + offset).clamp(0, arrival_ns);
        self.epoch + Duration::from_nanos(at_ns as u64)
    }
}

/// The context time to play an event that happened at `instant` (None: no device clock yet, so
/// "as soon as possible").
pub fn audio_time(shared: &Shared, instant: Instant) -> Option<f64> {
    let snap = shared.snapshot();
    snap.anchor?;
    Some(snap.audio_time_for_instant(instant))
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
