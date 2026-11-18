//! The tick transport (≙ Tone.Transport with PPQ 192, as the Electron app uses it).
//!
//! Position is kept in **ticks**, a piecewise-linear function of context time: while started,
//! `ticks(t) = anchor_ticks + (t − anchor_time) · bpm/60 · PPQ`, re-anchored at every tempo
//! change, seek and loop wrap. So musical positions (transportBeatAt) come from integrated
//! ticks, exactly as Tone's TickSource integrates its BPM signal.
//!
//! **Loop.** Tone stores loopStart/loopEnd in ticks (`toTicks` of the seconds it is given) and
//! wraps on the first tick ≥ loopEnd (Transport._processTick). The TS sets the loop in seconds
//! (`beats·60/bpm`) and re-applies it on every BPM change, so the ticks are always
//! `beats·PPQ`: storing the loop in ticks here is the same thing without the re-apply.
//!
//! **Pending start.** A start scheduled in the future (count-in end, or an explicit time) leaves
//! the transport *pending*; `beat_at` before it returns the start beat (the TS remembers
//! `transportStart = {time, beat}` for exactly this, R4-B1). Stop clears it (R5-N1); a seek
//! while pending moves the start beat (R5-N4).

use super::api::Phase;
use crate::contract::PPQ;

/// Tolerance in ticks when deciding which integer ticks fall inside a span.
pub(crate) const TICK_EPS: f64 = 1e-6;
const PPQ_F: f64 = PPQ as f64;

/// A loop wrap that happened (for events and capture counters).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Wrap {
    pub time: f64,
    pub from_beat: f64,
    pub to_beat: f64,
}

#[derive(Debug, Clone)]
pub(crate) struct Transport {
    pub phase: Phase,
    pub pending_at: f64,
    pub pending_beat: f64,
    anchor_time: f64,
    anchor_ticks: f64,
    pub bpm: f64,
    stopped_ticks: f64,
    /// Loop range in ticks (start < end).
    pub loop_ticks: Option<(f64, f64)>,
    pub wrap_count: u64,
    pub wrapped_beats: f64,
    /// The integer tick an inclusive advance already fired at the current end time (so the next
    /// span starting there does not fire it again). Cleared whenever the ticks jump.
    pub fired_tick: Option<i64>,
}

impl Transport {
    pub fn new(bpm: f64) -> Self {
        Self {
            phase: Phase::Stopped,
            pending_at: 0.0,
            pending_beat: 0.0,
            anchor_time: 0.0,
            anchor_ticks: 0.0,
            bpm,
            stopped_ticks: 0.0,
            loop_ticks: None,
            wrap_count: 0,
            wrapped_beats: 0.0,
            fired_tick: None,
        }
    }

    /// Ticks per second at the current tempo.
    pub fn rate(&self) -> f64 {
        self.bpm / 60.0 * PPQ_F
    }

    pub fn sec_per_beat(&self) -> f64 {
        60.0 / self.bpm
    }

    /// Ticks at `t` (started: the running clock without wrapping; otherwise the held position).
    pub fn ticks_at(&self, t: f64) -> f64 {
        match self.phase {
            Phase::Started => self.anchor_ticks + (t - self.anchor_time) * self.rate(),
            Phase::Pending => self.pending_beat * PPQ_F,
            Phase::Stopped => self.stopped_ticks,
        }
    }

    /// Context time of tick `k` (started).
    pub fn time_of_tick(&self, k: f64) -> f64 {
        self.anchor_time + (k - self.anchor_ticks) / self.rate()
    }

    /// `transportBeatAt(t)` for a time at or after the last processed point. Folds a loop end
    /// that `t` has reached, like the next processed tick would.
    pub fn beat_at(&self, t: f64) -> f64 {
        if self.phase == Phase::Pending && t >= self.pending_at {
            return self.pending_beat + (t - self.pending_at) * self.bpm / 60.0;
        }
        let mut k = self.ticks_at(t);
        if self.phase == Phase::Started
            && let Some((ls, le)) = self.loop_ticks
            && k >= le - TICK_EPS
        {
            k = ls + (k - le) % (le - ls);
        }
        k / PPQ_F
    }

    pub fn start(&mut self, at: f64, beat: f64) {
        self.phase = Phase::Pending;
        self.fired_tick = None;
        self.pending_at = at;
        self.pending_beat = beat;
    }

    /// The pending start takes effect at its time.
    pub fn begin(&mut self) {
        self.phase = Phase::Started;
        self.fired_tick = None;
        self.anchor_time = self.pending_at;
        self.anchor_ticks = self.pending_beat * PPQ_F;
    }

    pub fn stop(&mut self) {
        self.phase = Phase::Stopped;
        self.fired_tick = None;
        self.stopped_ticks = 0.0;
    }

    /// Move to `beat` at time `t`.
    pub fn seek(&mut self, t: f64, beat: f64) {
        self.fired_tick = None;
        match self.phase {
            Phase::Started => {
                self.anchor_time = t;
                self.anchor_ticks = beat * PPQ_F;
            }
            Phase::Pending => self.pending_beat = beat,
            Phase::Stopped => self.stopped_ticks = beat * PPQ_F,
        }
    }

    /// New tempo from time `t` (ticks so far are kept).
    pub fn set_bpm(&mut self, t: f64, bpm: f64) {
        if self.phase == Phase::Started {
            self.anchor_ticks = self.ticks_at(t);
            self.anchor_time = t;
        }
        self.bpm = bpm;
    }

    pub fn set_loop(&mut self, range: Option<(f64, f64)>) {
        self.loop_ticks = range
            .filter(|(s, e)| s.is_finite() && e.is_finite() && e > s && *s >= 0.0)
            .map(|(s, e)| (s * PPQ_F, e * PPQ_F));
    }

    /// Started and at/past the loop end at `t`: jump to the loop start (Tone wraps on the first
    /// processed tick ≥ loopEnd).
    pub fn wrap_if_due(&mut self, t: f64) -> Option<Wrap> {
        if self.phase != Phase::Started {
            return None;
        }
        let (ls, le) = self.loop_ticks?;
        if self.ticks_at(t) < le - TICK_EPS {
            return None;
        }
        Some(self.wrap_at(t, ls, le))
    }

    fn wrap_at(&mut self, t: f64, ls: f64, le: f64) -> Wrap {
        self.fired_tick = None;
        self.anchor_time = t;
        self.anchor_ticks = ls;
        self.wrap_count += 1;
        self.wrapped_beats += (le - ls) / PPQ_F;
        Wrap {
            time: t,
            from_beat: le / PPQ_F,
            to_beat: ls / PPQ_F,
        }
    }

    /// Plan the next started sub-span from `t` to at most `tb`: returns (k_a, k_b, t_end, wraps)
    /// where integer ticks in [k_a, k_b) fire inside [t, t_end) and `wraps` says whether the loop
    /// end is reached at `t_end` (then call [`Transport::wrap_at_end`]).
    pub fn span(&self, t: f64, tb: f64) -> (f64, f64, f64, bool) {
        let k_a = self.ticks_at(t);
        let k_full = self.ticks_at(tb);
        match self.loop_ticks {
            Some((_, le)) if k_full >= le - TICK_EPS => (k_a, le, self.time_of_tick(le), true),
            _ => (k_a, k_full, tb, false),
        }
    }

    /// The loop end was reached at `t` (from [`Transport::span`]).
    pub fn wrap_at_end(&mut self, t: f64) -> Option<Wrap> {
        let (ls, le) = self.loop_ticks?;
        Some(self.wrap_at(t, ls, le))
    }
}

/// Integer ticks `n` with `k_a ≤ n < k_b` (with [`TICK_EPS`] tolerance): `[lo, hi)`.
pub(crate) fn tick_range(k_a: f64, k_b: f64) -> (i64, i64) {
    (
        (k_a - TICK_EPS).ceil() as i64,
        (k_b - TICK_EPS).ceil() as i64,
    )
}

/// First multiple of `interval` (≥ 1) that is ≥ `lo`.
pub(crate) fn first_multiple(lo: i64, interval: i64) -> i64 {
    lo.div_euclid(interval) * interval
        + if lo.rem_euclid(interval) == 0 {
            0
        } else {
            interval
        }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_integrate_tempo_and_loop_stays_in_ticks() {
        let mut tr = Transport::new(120.0);
        tr.set_loop(Some((0.0, 4.0)));
        tr.start(1.0, 0.0);
        assert_eq!(tr.beat_at(0.5), 0.0, "pending: the start beat");
        tr.begin();
        assert!((tr.beat_at(2.0) - 2.0).abs() < 1e-9);
        tr.set_bpm(2.0, 60.0);
        assert!(
            (tr.beat_at(3.0) - 3.0).abs() < 1e-9,
            "1 s at 60 BPM after beat 2"
        );
        // loop end at beat 4 is reached at t = 4 s (60 BPM): still 4 beats of loop
        let (_, k_b, t_end, wraps) = tr.span(3.0, 5.0);
        assert!(wraps && k_b == 768.0 && (t_end - 4.0).abs() < 1e-9);
        let w = tr.wrap_at_end(t_end).unwrap();
        assert_eq!((w.from_beat, w.to_beat), (4.0, 0.0));
        assert_eq!((tr.wrap_count, tr.wrapped_beats), (1, 4.0));
        assert!((tr.beat_at(4.5) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn tick_ranges() {
        assert_eq!(tick_range(0.0, 2.5), (0, 3));
        assert_eq!(tick_range(2.5, 3.0), (3, 3));
        assert_eq!(tick_range(2.9999999, 4.0), (3, 4));
        assert_eq!(first_multiple(0, 48), 0);
        assert_eq!(first_multiple(1, 48), 48);
        assert_eq!(first_multiple(96, 48), 96);
        assert_eq!(first_multiple(-5, 48), 0);
    }
}
