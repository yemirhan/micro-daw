//! Time advance: count-in clicks, the pending start, and the transport's ticks fired in time
//! order (automation callbacks, pattern notes, metronome, note-repeat lines, punch close; see the same-time order in
//! the module docs of `core.rs`), or the free-running repeats while stopped.

use super::api::{CaptureKind, CoreEvent, Phase, SlotFx, TrackInstrument};
use super::click::{ACCENT_HZ, BEAT_HZ};
use super::transport::{TICK_EPS, first_multiple, tick_range};
use super::{Core, MAX_PADS};
use crate::contract::PPQ;

impl Core {
    /// Fire everything due in frames `[fa, fb)`; with `inclusive`, also the transport events at
    /// exactly `fb`'s time (a live pad command at `fb` then applies after them).
    pub(super) fn advance(&mut self, fa: u64, fb: u64, inclusive: bool) {
        if fa > fb || (fa == fb && !inclusive) {
            return;
        }
        let (ta, tb) = (self.time(fa), self.time(fb));
        self.count_in_events(tb);
        match self.tr.phase {
            Phase::Stopped => self.free_repeats(ta, tb, inclusive),
            Phase::Pending => {
                let due = self.tr.pending_at < tb || (inclusive && self.tr.pending_at <= tb);
                if due {
                    let at = self.tr.pending_at.max(ta);
                    self.tr.pending_at = at;
                    self.tr.begin();
                    self.emit(CoreEvent::Started {
                        time: at,
                        beat: self.tr.pending_beat,
                        generation: self.generation,
                    });
                    self.run_started(at, tb, inclusive);
                }
            }
            Phase::Started => self.run_started(ta, tb, inclusive),
        }
    }

    fn count_in_events(&mut self, tb: f64) {
        while let Some(ci) = self.ci {
            if ci.next >= ci.beats || ci.t0 + ci.next as f64 * ci.spb >= tb {
                break;
            }
            self.emit_visual(CoreEvent::CountInClick {
                time: ci.t0 + ci.next as f64 * ci.spb,
                index: ci.next,
                remaining: ci.beats - ci.next,
                generation: self.generation,
            });
            if let Some(c) = &mut self.ci {
                c.next += 1;
            }
        }
    }

    fn run_started(&mut self, mut t: f64, tb: f64, inclusive: bool) {
        for _ in 0..64 {
            if t > tb || (t == tb && !inclusive) {
                break;
            }
            self.settle_wrap(t);
            let (k_a, k_b, t_end, wraps) = self.tr.span(t, tb);
            let last = !(wraps && (t_end < tb || (inclusive && t_end <= tb)));
            let (mut lo, hi) = if last && inclusive {
                (
                    (k_a - TICK_EPS).ceil() as i64,
                    (k_b + TICK_EPS).floor() as i64 + 1,
                )
            } else {
                tick_range(k_a, k_b)
            };
            if let Some(f) = self.tr.fired_tick {
                lo = lo.max(f + 1);
            }
            if lo < hi {
                self.fire_ticks(lo, hi);
                if last && inclusive {
                    self.tr.fired_tick = Some(hi - 1);
                }
            }
            if last {
                break;
            }
            if let Some(w) = self.tr.wrap_at_end(t_end) {
                self.on_wrap(w);
            }
            t = t_end;
        }
    }

    /// Integer ticks `[lo, hi)` of the running transport (no wrap inside), tick by tick.
    fn fire_ticks(&mut self, lo: i64, hi: i64) {
        let mut k = lo;
        while k < hi {
            let mut next = hi;
            for slot in &self.tracks {
                if let Some(s) = &slot.schedule
                    && let Some(t) = s.next_tick(k, next, slot.instrument.is_some())
                {
                    next = next.min(t);
                }
            }
            if self.metronome_on {
                next = next.min(first_multiple(k, PPQ as i64));
            }
            if self.repeat.enabled && self.repeat.any_transport() {
                next = next.min(first_multiple(k, self.repeat.interval_ticks()));
            }
            if let Some(p) = self.punch_tick
                && p >= k
            {
                next = next.min(p);
            }
            if next >= hi {
                break;
            }
            self.fire_tick(next);
            k = next + 1;
        }
    }

    /// Everything on integer tick `k`: pattern notes (slot order), metronome, repeat lines,
    /// punch close.
    fn fire_tick(&mut self, k: i64) {
        let spb = self.tr.sec_per_beat();
        for i in 0..self.tracks.len() {
            let super::Slot {
                instrument,
                schedule,
                strip,
                ..
            } = &mut self.tracks[i];
            let Some(sched) = schedule.as_deref() else {
                continue;
            };
            // automation callbacks first (inserted before the track's Part in Tone)
            if let Some(SlotFx::Track(strip)) = strip.as_mut() {
                for a in sched.automation_in_ticks(k, k + 1) {
                    strip.apply(a.op, self.tr.time_of_tick(a.tick));
                }
            }
            let Some(inst) = instrument.as_mut() else {
                continue;
            };
            let events = sched.in_ticks(k, k + 1);
            if events.is_empty() {
                continue;
            }
            for e in events {
                let time = self.tr.time_of_tick(e.tick);
                match inst {
                    TrackInstrument::Drum(d) => {
                        let Some(pad) = e.pad else { continue };
                        if self.suppress.consume(e.note, e.beat, time, self.generation) {
                            continue;
                        }
                        d.trigger(pad, e.velocity, time);
                        if d.pad_is_gate(pad) {
                            // R2-11: the tempo at event time.
                            d.release(pad, time + (e.duration_beats * spb).max(0.01));
                        }
                        if self
                            .visual
                            .push(CoreEvent::PatternHit { slot: i, pad, time })
                            .is_err()
                        {
                            self.shared.count_dropped_event();
                        }
                    }
                    TrackInstrument::Synth(s) => {
                        s.note_on(e.note, e.velocity, time);
                        s.note_off(e.note, time + e.duration_beats * spb);
                    }
                    TrackInstrument::Audio(a) => {
                        if let Some(region) = e.pad {
                            a.start_region(region, time);
                        }
                    }
                }
            }
            self.drain_slot(i);
        }
        let time = self.tr.time_of_tick(k as f64);
        if self.metronome_on && k.rem_euclid(PPQ as i64) == 0 && time > self.last_click {
            let beat = k / PPQ as i64;
            let accent = beat % 4 == 0;
            let hz = if accent { ACCENT_HZ } else { BEAT_HZ };
            self.metro.schedule(time, spb / 8.0, hz);
            self.last_click = time;
            self.emit_visual(CoreEvent::MetronomeClick {
                time,
                beat: beat as f64,
                accent,
            });
        }
        if self.repeat.enabled
            && self.repeat.any_transport()
            && k.rem_euclid(self.repeat.interval_ticks()) == 0
        {
            let grid = self.repeat.rate_beats;
            let mut due = [(0usize, 0f64); MAX_PADS];
            let mut n = 0;
            for (pad, vel) in self.repeat.transport_due(time) {
                due[n] = (pad, vel);
                n += 1;
            }
            for &(pad, vel) in &due[..n] {
                self.live_trigger(pad, vel, time);
                self.capture(CaptureKind::RepeatHit, None, pad, vel, time, Some(grid));
            }
        }
        if self.punch_tick == Some(k) {
            self.emit(CoreEvent::PunchClose {
                time,
                generation: self.generation,
            });
        }
    }

    /// Free-running repeats with the transport stopped.
    fn free_repeats(&mut self, ta: f64, tb: f64, inclusive: bool) {
        let grid = self.repeat.rate_beats;
        // `inclusive`: a hit at exactly tb fires before the live command at tb
        let bound = if inclusive { tb.next_up() } else { tb };
        for _ in 0..256 {
            let Some((pad, vel, time)) = self.repeat.next_free_before(bound) else {
                break;
            };
            self.repeat.advance_free(pad, self.tr.bpm);
            // The core never runs late; the TS skip rule (> 30 ms late) is kept for safety.
            if time >= ta - 0.03 {
                let time = time.max(ta);
                self.live_trigger(pad, vel, time);
                self.capture(CaptureKind::RepeatHit, None, pad, vel, time, Some(grid));
            }
        }
    }
}
