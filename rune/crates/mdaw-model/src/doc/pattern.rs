//! Pattern region + step editing (ArrangementEngine.ts AE:2591-2714).

use crate::arrangement_helpers::{DEFAULT_STEP_VELOCITY, STEP_EPSILON, pattern_step_at};
use crate::types::{PAD_COUNT, PadIndex, Region, RegionNote, note_to_pad, pad_to_note};

use super::{ArrangementDoc, DocEvent, clamp01, js_max, js_round};

/// JS `stepBeats > 0` (false for NaN).
fn positive(v: f64) -> bool {
    v > 0.0
}

impl ArrangementDoc {
    fn pattern_region_index(&self, ti: usize) -> Option<usize> {
        let start = self.get_pattern_start();
        let mut best: Option<usize> = None;
        for (ri, r) in self.arrangement.tracks[ti].regions.iter().enumerate() {
            if r.audio.is_some() {
                continue;
            }
            if r.start_beat > start + STEP_EPSILON
                || start >= r.start_beat + r.length_beats - STEP_EPSILON
            {
                continue;
            }
            if best.is_none_or(|b| r.start_beat > self.arrangement.tracks[ti].regions[b].start_beat)
            {
                best = Some(ri);
            }
        }
        best
    }

    /// `getPatternRegion(id)`: the MIDI region covering the pattern start (latest start wins).
    pub fn get_pattern_region(&self, track_id: &str) -> Option<&Region> {
        let ti = self.track_index(track_id)?;
        let ri = self.pattern_region_index(ti)?;
        Some(&self.arrangement.tracks[ti].regions[ri])
    }

    /// `ensurePatternRegion(id)`: the pattern region, created ('Pattern') or grown to cover the
    /// pattern — undo 'Add Pattern' only when it changed. An unknown track uses the active drum
    /// track (created if needed).
    pub fn ensure_pattern_region(&mut self, track_id: &str) -> Region {
        let ti = match self.track_index(track_id) {
            Some(ti) => ti,
            None => {
                let id = self.ensure_drum_track();
                self.track_index(&id)
                    .expect("ensure_drum_track returns an existing track")
            }
        };
        let snapshot = self.undo_snapshot();
        let (ri, changed) = self.ensure_pattern_region_on(ti);
        if changed {
            self.commit_undo("Add Pattern", snapshot);
            self.emit_state_change();
        }
        self.arrangement.tracks[ti].regions[ri].clone()
    }

    /// `ensurePatternRegionOn(track)` → (region index, changed). No undo.
    pub(crate) fn ensure_pattern_region_on(&mut self, ti: usize) -> (usize, bool) {
        let start = self.get_pattern_start();
        let length = self.get_pattern_length();
        if let Some(ri) = self.pattern_region_index(ti) {
            let existing = &mut self.arrangement.tracks[ti].regions[ri];
            let needed = start - existing.start_beat + length;
            if existing.length_beats >= needed - STEP_EPSILON {
                return (ri, false);
            }
            existing.length_beats = needed;
            self.auto_extend_length();
            return (ri, true);
        }
        let region = Region {
            id: self.next_id(),
            start_beat: start,
            length_beats: length,
            notes: Vec::new(),
            color: Some(self.arrangement.tracks[ti].color.clone()),
            name: Some("Pattern".to_owned()),
            audio: None,
        };
        self.arrangement.tracks[ti].regions.push(region);
        self.auto_extend_length();
        (self.arrangement.tracks[ti].regions.len() - 1, true)
    }

    /// `patternNoteFilter(region, pad?, step?)`: a pad note inside the pattern window of the
    /// region starting at `region_start` (of `pad`, on step `(index, stepBeats)` when given).
    pub fn pattern_note_filter(
        &mut self,
        region_start: f64,
        pad: Option<PadIndex>,
        step: Option<(i64, f64)>,
    ) -> impl Fn(&RegionNote) -> bool + use<> {
        let offset = self.get_pattern_start() - region_start;
        let length = self.get_pattern_length();
        let pad_note = pad.map(|p| f64::from(pad_to_note(p)));
        let steps = step.map_or(0.0, |(_, sb)| js_max(1.0, js_round(length / sb)));
        move |n: &RegionNote| {
            let is_pad = match pad_note {
                Some(p) => n.note == p,
                None => note_to_pad(n.note).is_some(),
            };
            if !is_pad {
                return false;
            }
            let local = n.start_beat - offset;
            if local < -STEP_EPSILON || local >= length - STEP_EPSILON {
                return false;
            }
            step.is_none_or(|(index, sb)| pattern_step_at(local, sb, steps) == index as f64)
        }
    }

    /// `toggleStep(id, pad, step, stepBeats, velocity = 0.8)` — undo 'Toggle Step'. Stored straight
    /// at `step * stepBeats` with duration `stepBeats`.
    pub fn toggle_step(
        &mut self,
        track_id: &str,
        pad: PadIndex,
        step: i64,
        step_beats: f64,
        velocity: Option<f64>,
    ) {
        let Some(ti) = self.drum_track_index(track_id) else {
            return;
        };
        if pad >= PAD_COUNT || step < 0 || !positive(step_beats) {
            return;
        }
        let local = step as f64 * step_beats;
        if local >= self.get_pattern_length() - STEP_EPSILON {
            return;
        }
        let snapshot = self.undo_snapshot();
        let (ri, _) = self.ensure_pattern_region_on(ti);
        let region_start = self.arrangement.tracks[ti].regions[ri].start_beat;
        let on_step = self.pattern_note_filter(region_start, Some(pad), Some((step, step_beats)));
        let pattern_start = self.get_pattern_start();
        let region = &mut self.arrangement.tracks[ti].regions[ri];
        if region.notes.iter().any(&on_step) {
            region.notes.retain(|n| !on_step(n));
        } else {
            region.notes.push(RegionNote {
                note: f64::from(pad_to_note(pad)),
                velocity: clamp01(velocity.unwrap_or(DEFAULT_STEP_VELOCITY)),
                start_beat: pattern_start - region_start + local,
                duration_beats: step_beats,
                is_drum: true,
            });
        }
        self.commit_undo("Toggle Step", snapshot);
        self.emit_state_change();
        self.emit(DocEvent::RescheduleTrack(track_id.to_owned()));
    }

    /// `setStepVelocity(id, pad, step, stepBeats, velocity)` — undo 'Step Velocity'; no-op on an
    /// empty step.
    pub fn set_step_velocity(
        &mut self,
        track_id: &str,
        pad: PadIndex,
        step: i64,
        step_beats: f64,
        velocity: f64,
    ) {
        let Some(ti) = self.drum_track_index(track_id) else {
            return;
        };
        let Some(ri) = self.pattern_region_index(ti) else {
            return;
        };
        if pad >= PAD_COUNT || !positive(step_beats) {
            return;
        }
        let region_start = self.arrangement.tracks[ti].regions[ri].start_beat;
        let on_step = self.pattern_note_filter(region_start, Some(pad), Some((step, step_beats)));
        if !self.arrangement.tracks[ti].regions[ri]
            .notes
            .iter()
            .any(&on_step)
        {
            return;
        }
        self.push_undo_snapshot("Step Velocity");
        let v = clamp01(velocity);
        for n in self.arrangement.tracks[ti].regions[ri].notes.iter_mut() {
            if on_step(n) {
                n.velocity = v;
            }
        }
        self.emit_state_change();
        self.emit(DocEvent::RescheduleTrack(track_id.to_owned()));
    }

    /// `clearPattern(id, pad?)` — undo 'Clear Pattern' (only when something is removed).
    pub fn clear_pattern(&mut self, track_id: &str, pad: Option<PadIndex>) {
        let Some(ti) = self.track_index(track_id) else {
            return;
        };
        let Some(ri) = self.pattern_region_index(ti) else {
            return;
        };
        if pad.is_some_and(|p| p >= PAD_COUNT) {
            return;
        }
        let region_start = self.arrangement.tracks[ti].regions[ri].start_beat;
        let in_pattern = self.pattern_note_filter(region_start, pad, None);
        if !self.arrangement.tracks[ti].regions[ri]
            .notes
            .iter()
            .any(&in_pattern)
        {
            return;
        }
        self.push_undo_snapshot("Clear Pattern");
        self.arrangement.tracks[ti].regions[ri]
            .notes
            .retain(|n| !in_pattern(n));
        self.emit_state_change();
        self.emit(DocEvent::RescheduleTrack(track_id.to_owned()));
    }
}
