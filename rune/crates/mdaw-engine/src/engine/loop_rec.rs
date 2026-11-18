//! Loop recording, control side (`startLoopRecording` / `stopLoopRecording`, `AE:2718-2808`, and
//! the timing half of `LoopRecorder.ts`; the pattern edits are the doc's `loop_take_*`).
//!
//! - Start: stops a Song recording / playback, opens the take in the doc (drum track, pattern
//!   region, snapshot), loops the pattern range (transiently), schedules every track, clicks per
//!   `shouldClick`, and with a count-in pre-schedules `countInBars·4` clicks from
//!   `t0 = now + 0.05 s` and starts the transport at their end (`patternStart`). A start the core
//!   could not accept (command ring full: the TS equivalent is Tone throwing) rolls back
//!   (`abort_loop_take_start`) and notifies: never stuck in 'recording'.
//! - Hits come from the core's pad captures (frame-exact `time`, the transport `beat` from ticks,
//!   B1 start beat while pending, repeat `grid_beats`), exactly as `LoopRecorder.captureHit`:
//!   count-in hits more than 0.5 beat before the end are ignored, later ones land on beat 1
//!   (`ahead = left/spb`); after it the hit is quantized on the absolute grid against the
//!   sounding swing (or snapped to a finer note-repeat grid), wrapped into the loop, merged
//!   (louder wins, first change = the take's ONE 'Loop Record' undo step, replace pass when
//!   overdub is off); a hit snapped forward into this pass suppresses the pattern's replay of it
//!   until `at + ahead·spb + 0.15 s`.
//! - Gate pads: the held length is musical: `loopGateBeats(onBeat, offBeat, wrapped, loop)` with
//!   `wrapped` = the loop beats wrapped between note-on and note-off, from the core's running
//!   wrap counter (no wrap list to prune: L-e cannot happen). One-shots keep their length.
//! - Captures from before the take's transport start (older generation) are dropped.
//! - The take ends where the doc ends it (stop, undo/redo `settleHistory`, restore, active drum
//!   track change, track removal) through the take-end hook: held gates get their length at
//!   the core's now, then the transport stops (`LoopTakeEnded`).
//!
//! Deviation: the forward-snap suppression is installed when the engine reads the capture (next
//! UI frame), not inside the MIDI callback; a hit snapped forward by less than that delay onto a
//! step that ALREADY holds a note of the pad may sound twice (the TS suppresses it).

use mdaw_model::arrangement_helpers::{
    STEP_EPSILON, grid_step_beats, loop_gate_beats, loop_swing_grid, quantize_beat,
    swing_offset_beats, wrap_beat,
};
use mdaw_model::doc::ArrangementDoc;
use mdaw_model::{note_to_pad, pad_to_note};

use super::{ArrangementEngine, PendingPosition, TransportState};
use crate::core::{CaptureKind, CoreHandle, PadCapture, Shared};

/// Hits this close before the end of the count-in are recorded on the first beat.
const COUNT_IN_PRE_ROLL_BEATS: f64 = 0.5;
/// Extra time (s) a suppressed pattern hit stays suppressed after its expected play time.
const SUPPRESS_MARGIN_SEC: f64 = 0.15;
/// The count-in's first click is this far ahead of now.
pub const COUNT_IN_MARGIN_SEC: f64 = 0.05;

#[derive(Debug, Clone)]
struct Held {
    pad: usize,
    region_id: String,
    start_beat: f64,
    on_beat: f64,
    on_wrapped: f64,
}

/// The running take's timing state (`LoopRecorder` minus the doc's `LoopTake`).
#[derive(Debug, Clone, Default)]
pub(super) struct LoopRec {
    /// Transport generation of the take's start.
    pub generation: u64,
    /// Context time the count-in ends (transport start), None without a count-in.
    pub count_in_end: Option<f64>,
    held: Vec<Held>,
}

impl LoopRec {
    /// Note-off of a held gate pad at `off` (`offBeat`, wrapped beats so far): set its length.
    fn release(
        &mut self,
        doc: &mut ArrangementDoc,
        pad: usize,
        off: f64,
        off_beat_core: f64,
        wrapped_now: f64,
    ) {
        let Some(i) = self.held.iter().position(|h| h.pad == pad) else {
            return;
        };
        let held = self.held.remove(i);
        let Some(take) = doc.loop_take().cloned() else {
            return;
        };
        let spb = 60.0 / doc.get_bpm();
        let left = self.count_in_end.map_or(0.0, |e| e - off);
        let off_beat = if left > 0.0 {
            take.pattern_start - left / spb
        } else {
            off_beat_core
        };
        let wrapped = wrapped_now - held.on_wrapped;
        let beats = loop_gate_beats(held.on_beat, off_beat, wrapped, take.loop_beats);
        doc.loop_take_set_gate_length(
            &held.region_id,
            f64::from(pad_to_note(pad)),
            held.start_beat,
            beats,
        );
    }

    /// `loopRecorder.end()`: gate pads still held get their length at the core's now.
    pub fn end(mut self, doc: &mut ArrangementDoc, shared: &Shared) {
        let pads: Vec<usize> = self.held.iter().map(|h| h.pad).collect();
        for pad in pads {
            self.release(
                doc,
                pad,
                shared.now(),
                shared.playhead_beat(),
                shared.wrapped_beats(),
            );
        }
    }

    /// `getCountInRemaining()` at `now`.
    pub fn count_in_remaining(&self, doc: &ArrangementDoc, now: f64) -> u32 {
        let (Some(end), Some(take)) = (self.count_in_end, doc.loop_take()) else {
            return 0;
        };
        let left = end - now;
        if left <= 0.0 {
            return 0;
        }
        let spb = 60.0 / doc.get_bpm();
        (left / spb - 1e-6).ceil().min(take.count_in_beats).max(0.0) as u32
    }
}

/// `LoopRecorder.captureHit` for a core capture. Returns the track to reschedule.
fn capture_hit(
    doc: &mut ArrangementDoc,
    rec: &mut LoopRec,
    h: &mut CoreHandle,
    c: &PadCapture,
) -> Option<String> {
    let take = doc.loop_take()?.clone();
    let pad = note_to_pad(f64::from(pad_to_note(c.pad)))?;
    let at = c.time;
    let spb = 60.0 / doc.get_bpm();
    let lp = doc.get_loop_settings();
    let length = take.loop_beats;
    let quant_step = grid_step_beats(lp.quantize);
    let fine_grid = c
        .grid_beats
        .filter(|&g| g > 0.0 && quant_step.is_none_or(|q| g < q - STEP_EPSILON));
    let swing_grid = loop_swing_grid(Some(lp.swing_grid));
    let left = rec.count_in_end.map_or(0.0, |e| e - at);
    let (quantized, mut ahead, on_beat) = if left > 0.0 {
        if left > COUNT_IN_PRE_ROLL_BEATS * spb {
            return None;
        }
        let ahead = left / spb;
        (0.0, ahead, take.pattern_start - ahead)
    } else {
        let on_beat = c.beat;
        let played = wrap_beat(on_beat - take.pattern_start, length);
        let abs = take.pattern_start + played;
        let snapped = match fine_grid {
            Some(g) => (abs / g).round() * g,
            None => quantize_beat(abs, lp.quantize, lp.swing, swing_grid),
        };
        let quantized = wrap_beat(snapped - take.pattern_start, length);
        let plays_at =
            quantized + swing_offset_beats(take.pattern_start + quantized, swing_grid, lp.swing);
        (quantized, plays_at - played, on_beat)
    };
    if left <= 0.0 {
        if ahead < -length / 2.0 {
            ahead += length;
        } else if ahead > length / 2.0 {
            ahead -= length;
        }
    }
    let duration = fine_grid.or(quant_step).unwrap_or(0.25);
    let gate = doc.is_gate_pad(&take.track_id, pad);
    let hit = doc.loop_take_record_hit(pad, c.velocity, quantized, duration)?;
    if gate {
        rec.held.retain(|x| x.pad != pad);
        rec.held.push(Held {
            pad,
            region_id: hit.region_id.clone(),
            start_beat: hit.note.start_beat,
            on_beat,
            on_wrapped: c.wrapped_beats,
        });
    }
    if ahead > 0.0 {
        h.suppress_pattern_hit(
            hit.note.note,
            hit.region_start_beat + hit.note.start_beat,
            at + ahead * spb + SUPPRESS_MARGIN_SEC,
        );
    }
    Some(hit.track_id)
}

impl ArrangementEngine {
    pub fn is_loop_recording(&self) -> bool {
        self.doc.is_loop_recording()
    }

    /// `getCountInRemaining()`: beats of count-in left (0 when not counting in).
    pub fn get_count_in_remaining(&self) -> u32 {
        self.rec
            .borrow()
            .as_ref()
            .map_or(0, |r| r.count_in_remaining(&self.doc, self.h.now()))
    }

    /// Context time the take's count-in ends (its transport start), if it has one.
    pub fn get_count_in_end(&self) -> Option<f64> {
        self.rec.borrow().as_ref().and_then(|r| r.count_in_end)
    }

    /// The count-in's per-beat UI notification (`scheduleCountInTick`).
    pub(super) fn count_in_tick(&mut self) {
        let remaining = self.get_count_in_remaining();
        if remaining != self.last_count_in {
            self.last_count_in = remaining;
            self.doc.notify_state_change();
        }
    }

    pub(super) fn install_take_end_hook(&mut self) {
        let rec = self.rec.clone();
        let shared = self.h.shared_arc();
        self.doc.set_take_end_hook(Some(Box::new(move |doc| {
            let r = rec.borrow_mut().take();
            if let Some(r) = r {
                r.end(doc, &shared);
            }
        })));
    }

    /// `startLoopRecording()`.
    pub fn start_loop_recording(&mut self) {
        self.pump();
        if self.doc.is_loop_recording() {
            return;
        }
        if self.state == TransportState::Recording {
            self.stop_recording_to_track();
        } else if self.state == TransportState::Playing {
            self.stop_transport();
        }
        let start = self.doc.begin_loop_take();
        self.flush_doc();
        let Some(start) = start else {
            return;
        };
        *self.rec.borrow_mut() = Some(super::loop_rec::LoopRec::default());
        self.state = TransportState::Recording;
        let mut ok = self.h.set_bpm(self.doc.get_bpm());
        self.apply_loop_to_transport();
        self.schedule_all_tracks();
        if self.should_click() {
            ok &= self.h.set_metronome(true);
        }
        #[cfg(test)]
        {
            ok &= !std::mem::take(&mut self.fail_next_start);
        }
        let ps = start.pattern_start;
        let (generation, count_in_end) = if start.count_in_beats > 0.0 && ok {
            let spb = 60.0 / self.doc.get_bpm();
            let t0 = self.h.now() + COUNT_IN_MARGIN_SEC;
            let (g, end) = self
                .h
                .start_with_count_in(ps, t0, start.count_in_beats as u32, spb);
            self.transport_start = Some((end, ps));
            self.pending_position = Some(PendingPosition {
                generation: g,
                beat: ps,
            });
            (g, Some(end))
        } else if ok {
            (self.start_transport_at(ps), None)
        } else {
            (0, None)
        };
        if !ok || generation == 0 {
            // Rollback (the TS catch): abort the take, stop, restore the structure.
            *self.rec.borrow_mut() = None;
            self.stop_transport();
            self.doc.abort_loop_take_start();
            self.flush_doc();
            self.doc.notify_state_change();
            return;
        }
        if let Some(r) = self.rec.borrow_mut().as_mut() {
            r.generation = generation;
            r.count_in_end = count_in_end;
        }
        self.last_count_in = self.get_count_in_remaining();
        self.doc.notify_state_change();
    }

    /// `stopLoopRecording()`: end the take (its gates closed), stop the transport.
    pub fn stop_loop_recording(&mut self) {
        self.edit(|d| d.stop_loop_take());
    }

    /// Test hook: the next loop-record start fails like a Tone exception.
    #[cfg(test)]
    pub(super) fn inject_start_failure(&mut self) {
        self.fail_next_start = true;
    }

    /// A pad capture during a take (`captureHit` / `captureRelease`).
    pub(super) fn loop_capture(&mut self, c: &PadCapture) {
        let mut guard = self.rec.borrow_mut();
        let Some(rec) = guard.as_mut() else {
            return;
        };
        if c.generation < rec.generation {
            return;
        }
        let track = match c.kind {
            CaptureKind::Hit | CaptureKind::RepeatHit => {
                capture_hit(&mut self.doc, rec, &mut self.h, c)
            }
            CaptureKind::Release => {
                rec.release(&mut self.doc, c.pad, c.time, c.beat, c.wrapped_beats);
                None
            }
        };
        drop(guard);
        if let Some(id) = track {
            // (the doc's record_hit already notified the state listeners once)
            self.refresh_playback_schedule(&id);
        }
        self.flush_doc();
    }
}
