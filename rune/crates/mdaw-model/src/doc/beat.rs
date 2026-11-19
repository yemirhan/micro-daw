//! Beat mode: active drum track, kits and pads, selected pad, loop settings, loop ranges and the
//! data half of loop recording (ArrangementEngine.ts AE:2318-2590, 2718-2808;
//! src/services/arrangement/LoopRecorder.ts).

use std::borrow::Cow;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::arrangement_helpers::{STEP_EPSILON, merge_drum_hit};
use crate::kit::{create_factory_kit, normalize_kit, normalize_pad, pad_source_key};
use crate::loop_settings::{normalize_beat_state, normalize_loop_settings};
use crate::types::{
    Arrangement, BeatState, InstrumentType, KitState, LoopMarkers, LoopRecordSettings, PAD_COUNT,
    PadIndex, PadPlayMode, PadSample, PadSettings, QuantizeGrid, RegionNote, SwingGrid,
    pad_to_note,
};

use super::{ArrangementDoc, DocEvent, KitSelectedEvent, KitSelectionSource, clamp01};

/// `Partial<LoopRecordSettings>` for `setLoopSettings` (unset fields keep their value).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoopSettingsPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bars: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quantize: Option<QuantizeGrid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swing: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swing_grid: Option<SwingGrid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count_in_bars: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metronome: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overdub: Option<bool>,
}

/// The pure part of a loop-recording take (`LoopRecordingSession` without the count-in time).
#[derive(Debug, Clone, PartialEq)]
pub struct LoopTake {
    pub track_id: String,
    /// Absolute beat where the pattern (and the loop) starts.
    pub pattern_start: f64,
    /// Loop length in beats (bars × 4).
    pub loop_beats: f64,
    pub count_in_beats: f64,
    /// overdub=false: the pattern was already cleared by the first hit of this take.
    pub replaced: bool,
    /// Arrangement before the take; pushed as ONE 'Loop Record' undo step on the first change.
    pub undo_snapshot: Arrangement,
    pub undo_pushed: bool,
    /// The take created / grew the pattern region (needs an undo step even without hits).
    pub structure_changed: bool,
}

/// What `begin_loop_take` prepared (the engine schedules the transport from it).
#[derive(Debug, Clone, PartialEq)]
pub struct LoopTakeStart {
    pub track_id: String,
    pub pattern_start: f64,
    pub loop_beats: f64,
    pub count_in_beats: f64,
    pub structure_changed: bool,
}

/// Result of [`ArrangementDoc::loop_take_record_hit`].
#[derive(Debug, Clone, PartialEq)]
pub struct LoopHit {
    pub track_id: String,
    pub region_id: String,
    pub region_start_beat: f64,
    /// The stored hit (region-relative start).
    pub note: RegionNote,
    /// The hit added a note (not merged into an existing one).
    pub added: bool,
    /// The pattern region was created / grown for it.
    pub grown: bool,
    /// The replace pass cleared existing pattern notes.
    pub cleared: bool,
}

impl ArrangementDoc {
    // --- Active drum track ---

    /// The beat state, read-only: the stored one, else what `beat()` would create.
    pub(crate) fn beat_view(&self) -> Cow<'_, BeatState> {
        match &self.arrangement.beat {
            Some(b) => Cow::Borrowed(b),
            None => Cow::Owned(normalize_beat_state(None, &self.arrangement.tracks)),
        }
    }

    /// `getActiveDrumTrackId()`, read-only (review M5): a stale / unset id resolves to the first
    /// drum track. The TS getter also *stores* that fallback; here the store happens in
    /// `resolve_active_drum_track_id`, which every TS caller that persists it goes through
    /// (`syncDrumSink`, run by every edit that can make the id stale: removeTrack,
    /// setTrackInstrument, restore/load), so the value returned is the same either way and the
    /// stored JSON matches the TS after every op (doc goldens).
    pub fn get_active_drum_track_id(&self) -> Option<String> {
        let beat = self.beat_view();
        if let Some(id) = beat.active_drum_track_id.as_deref()
            && self.drum_track_index(id).is_some()
        {
            return Some(id.to_owned());
        }
        self.arrangement
            .tracks
            .iter()
            .find(|t| t.instrument.kind == InstrumentType::Drums)
            .map(|t| t.id.clone())
    }

    /// The mutating half of the TS `getActiveDrumTrackId()`: resolve and store.
    pub(crate) fn resolve_active_drum_track_id(&mut self) -> Option<String> {
        let id = self.get_active_drum_track_id();
        if self.beat().active_drum_track_id != id {
            self.beat().active_drum_track_id = id.clone();
        }
        id
    }

    /// `setActiveDrumTrack(id)` (no undo). Ends a take on another track first.
    pub fn set_active_drum_track(&mut self, track_id: &str) {
        if self.drum_track_index(track_id).is_none()
            || self.beat().active_drum_track_id.as_deref() == Some(track_id)
        {
            return;
        }
        if self
            .loop_take
            .as_ref()
            .is_some_and(|t| t.track_id != track_id)
        {
            self.end_loop_take(true);
        }
        self.beat().active_drum_track_id = Some(track_id.to_owned());
        self.sync_drum_sink();
        self.emit_state_change();
        self.emit_kit_change();
    }

    /// `ensureDrumTrack()`: the active drum track id; creates a "Drums" track when there is none.
    pub fn ensure_drum_track(&mut self) -> String {
        match self.resolve_active_drum_track_id() {
            Some(id) => id,
            None => self.add_track(InstrumentType::Drums, 0).id,
        }
    }

    /// `syncDrumSink()`: reads the active drum track (lazy fix-up) and asks the engine to rebind.
    pub(crate) fn sync_drum_sink(&mut self) {
        self.resolve_active_drum_track_id();
        self.emit(DocEvent::SyncDrumSink);
    }

    // --- Kits ---

    /// `getKit(id)`, read-only (review M5): the drum track's kit, else a fresh factory kit. The
    /// TS also stores a factory kit on a kitless drum track; that state cannot occur here
    /// (`ensure_track_defaults` gives every drum track a kit on add / instrument change / load).
    pub fn get_kit(&self, track_id: &str) -> KitState {
        self.kit(track_id)
            .cloned()
            .unwrap_or_else(create_factory_kit)
    }

    /// The stored kit of a drum track, without the lazy creation.
    pub fn kit(&self, track_id: &str) -> Option<&KitState> {
        let i = self.drum_track_index(track_id)?;
        self.arrangement.tracks[i].instrument.kit.as_ref()
    }

    /// `setKit(id, kit, source = 'user')` — undo 'Change Kit'. Normalizes, applies, reports the
    /// selection ([`DocEvent::KitSelected`]).
    pub fn set_kit(&mut self, track_id: &str, kit: &KitState, source: KitSelectionSource) {
        let Some(i) = self.drum_track_index(track_id) else {
            return;
        };
        self.push_undo_snapshot("Change Kit");
        let raw = serde_json::to_value(kit).unwrap_or(Value::Null);
        let next = normalize_kit(&raw, self.packs_dir.as_deref(), &mut *self.new_id);
        let kit_id = next.id.clone();
        self.apply_kit(i, next);
        self.emit(DocEvent::KitSelected(KitSelectedEvent {
            track_id: track_id.to_owned(),
            kit_id,
            source,
        }));
    }

    /// `updatePad(id, pad, patch)` — undo 'Edit Pad'. `normalizePad({ ...pad, ...patch })`.
    pub fn update_pad(&mut self, track_id: &str, pad: PadIndex, patch: &Map<String, Value>) {
        let Some(i) = self.drum_track_index(track_id) else {
            return;
        };
        if pad >= PAD_COUNT {
            return;
        }
        self.push_undo_snapshot("Edit Pad");
        let mut kit = self.get_kit(track_id);
        let mut merged = serde_json::to_value(&kit.pads[pad]).unwrap_or(Value::Null);
        if let Some(obj) = merged.as_object_mut() {
            for (k, v) in patch {
                obj.insert(k.clone(), v.clone());
            }
        }
        kit.pads[pad] = normalize_pad(&merged, self.packs_dir.as_deref());
        self.apply_kit(i, kit);
    }

    /// `setPadSample(id, pad, sample | null)` — undo 'Assign Sample' / 'Clear Pad'. A sample with a
    /// different source key resets `startSec = 0, endSec = null, reverse = false`.
    pub fn set_pad_sample(&mut self, track_id: &str, pad: PadIndex, sample: Option<&PadSample>) {
        let Some(i) = self.drum_track_index(track_id) else {
            return;
        };
        if pad >= PAD_COUNT {
            return;
        }
        self.push_undo_snapshot(if sample.is_some() {
            "Assign Sample"
        } else {
            "Clear Pad"
        });
        let mut kit = self.get_kit(track_id);
        let prev = &kit.pads[pad];
        let dir = self.packs_dir.as_deref();
        let key = |s: &PadSample| pad_source_key(&s.source, dir, &|_| 0);
        let same_source = matches!((&prev.sample, sample), (Some(a), Some(b)) if key(a) == key(b));
        let mut next: PadSettings = prev.clone();
        next.sample = sample.map(|s| PadSample {
            name: s.name.clone(),
            source: s.source.clone(),
        });
        if !same_source {
            next.start_sec = 0.0;
            next.end_sec = None;
            next.reverse = false;
        }
        let raw = serde_json::to_value(&next).unwrap_or(Value::Null);
        kit.pads[pad] = normalize_pad(&raw, dir);
        self.apply_kit(i, kit);
    }

    /// `applyKit(track, kit)`.
    pub(crate) fn apply_kit(&mut self, index: usize, kit: KitState) {
        self.arrangement.tracks[index].instrument.kit = Some(kit);
        // AE:2483 `sampler?.setKit(kit)`.
        let id = self.arrangement.tracks[index].id.clone();
        self.emit(DocEvent::KitApplied(id));
        self.emit_state_change();
        self.emit_kit_change();
    }

    /// The pad plays in gate mode (`isGatePad` of the loop recorder host).
    pub fn is_gate_pad(&self, track_id: &str, pad: PadIndex) -> bool {
        self.get_kit(track_id)
            .pads
            .get(pad)
            .is_some_and(|p| p.mode == PadPlayMode::Gate)
    }

    // --- Selected pad + loop settings ---

    pub fn get_selected_pad(&self) -> PadIndex {
        self.beat_view().selected_pad
    }

    /// `setSelectedPad(pad)` (no undo).
    pub fn set_selected_pad(&mut self, pad: PadIndex) {
        if pad >= PAD_COUNT || self.beat().selected_pad == pad {
            return;
        }
        self.beat().selected_pad = pad;
        self.emit_state_change();
    }

    pub fn get_loop_settings(&self) -> LoopRecordSettings {
        self.beat_view().r#loop
    }

    /// `setLoopSettings(patch)` (no undo). Only a `bars` change moves the loop markers to
    /// `[patternStart, +bars*4)` (and resizes a running take, growing its pattern); a swing /
    /// swing-grid change reschedules every drum track ([`DocEvent::SwingChanged`]).
    pub fn set_loop_settings(&mut self, patch: &LoopSettingsPatch) {
        let prev = self.beat().r#loop;
        let mut raw = serde_json::to_value(prev).unwrap_or(Value::Null);
        if let (Some(obj), Ok(Value::Object(p))) =
            (raw.as_object_mut(), serde_json::to_value(patch))
        {
            for (k, v) in p {
                obj.insert(k, v);
            }
        }
        let next = normalize_loop_settings(&raw, &prev);
        self.beat().r#loop = next;
        let length = f64::from(next.bars) * 4.0;
        if next.bars != prev.bars {
            let start = self.get_pattern_start();
            self.apply_pattern_loop(start, length);
            let take_track = self.loop_take.as_mut().map(|take| {
                take.loop_beats = length;
                take.track_id.clone()
            });
            if let Some(track_id) = take_track
                && let Some(ti) = self.track_index(&track_id)
                && self.ensure_pattern_region_on(ti).1
            {
                if let Some(take) = &mut self.loop_take {
                    take.structure_changed = true;
                }
                self.emit(DocEvent::RescheduleTrack(track_id));
            }
            self.emit(DocEvent::LoopRangeChanged);
        }
        if next.swing != prev.swing || next.swing_grid != prev.swing_grid {
            self.emit(DocEvent::SwingChanged);
            let drums: Vec<String> = self
                .arrangement
                .tracks
                .iter()
                .filter(|t| t.instrument.kind == InstrumentType::Drums)
                .map(|t| t.id.clone())
                .collect();
            for id in drums {
                self.emit(DocEvent::RescheduleTrack(id));
            }
        }
        self.emit(DocEvent::MetronomeUpdate);
        self.emit_state_change();
    }

    // --- Pattern range + loop ranges ---

    /// `getPatternStart()`: the loop start, or 0.
    pub fn get_pattern_start(&self) -> f64 {
        self.arrangement.loop_markers.map_or(0.0, |m| m.start_beat)
    }

    /// `getPatternLength()`: bars × 4.
    pub fn get_pattern_length(&self) -> f64 {
        f64::from(self.beat_view().r#loop.bars) * 4.0
    }

    /// `applyPatternLoop(start, length)`: the loop markers become the pattern range.
    pub(crate) fn apply_pattern_loop(&mut self, start: f64, length: f64) {
        let s = super::js_max(0.0, start);
        let markers = LoopMarkers {
            start_beat: s,
            end_beat: s + length,
        };
        self.arrangement.loop_markers = Some(markers);
        if self.arrangement.length_beats < markers.end_beat {
            self.arrangement.length_beats = (markers.end_beat / 4.0).ceil() * 4.0;
        }
        self.emit(DocEvent::LoopRangeChanged);
    }

    /// `getActiveLoopRange()` given the engine's pattern-playback flag: the take's range, then
    /// the pattern range while pattern-playing, then the markers when the Song loop is on.
    pub fn get_active_loop_range(&self, pattern_playback: bool) -> Option<LoopMarkers> {
        if let Some(take) = &self.loop_take {
            return Some(LoopMarkers {
                start_beat: take.pattern_start,
                end_beat: take.pattern_start + take.loop_beats,
            });
        }
        if pattern_playback {
            let start = self.get_pattern_start();
            return Some(LoopMarkers {
                start_beat: start,
                end_beat: start + self.get_pattern_length(),
            });
        }
        self.arrangement.loop_markers.filter(|_| self.loop_enabled)
    }

    /// The data half of `playPattern()`: the pattern range, growing the song to cover it.
    pub fn prepare_play_pattern(&mut self) -> LoopMarkers {
        let start = self.get_pattern_start();
        let end = start + self.get_pattern_length();
        if self.arrangement.length_beats < end {
            self.arrangement.length_beats = (end / 4.0).ceil() * 4.0;
        }
        LoopMarkers {
            start_beat: start,
            end_beat: end,
        }
    }

    // --- Loop recording (data half) ---

    pub fn is_loop_recording(&self) -> bool {
        self.loop_take.is_some()
    }

    pub fn loop_take(&self) -> Option<&LoopTake> {
        self.loop_take.as_ref()
    }

    /// The data half of `startLoopRecording()` after the engine stopped the transport: ensure the
    /// drum track, snapshot, ensure the pattern region, open the take. None when a take runs.
    pub fn begin_loop_take(&mut self) -> Option<LoopTakeStart> {
        if self.loop_take.is_some() {
            return None;
        }
        let track_id = self.ensure_drum_track();
        let ti = self.track_index(&track_id)?;
        let undo_snapshot = self.arrangement.clone();
        let lp = self.beat().r#loop;
        let pattern_start = self.get_pattern_start();
        let loop_beats = f64::from(lp.bars) * 4.0;
        let structure_changed = self.ensure_pattern_region_on(ti).1;
        let count_in_beats = f64::from(lp.count_in_bars) * 4.0;
        self.loop_take = Some(LoopTake {
            track_id: track_id.clone(),
            pattern_start,
            loop_beats,
            count_in_beats,
            replaced: false,
            undo_snapshot,
            undo_pushed: false,
            structure_changed,
        });
        Some(LoopTakeStart {
            track_id,
            pattern_start,
            loop_beats,
            count_in_beats,
            structure_changed,
        })
    }

    /// Roll back a take whose start failed (`startLoopRecording` catch): the structure change is
    /// restored from the pre-take snapshot (no history), else the loop range is re-applied.
    pub fn abort_loop_take_start(&mut self) {
        let Some(take) = self.loop_take.take() else {
            return;
        };
        if take.structure_changed {
            self.restore_from_snapshot(&take.undo_snapshot);
        } else {
            self.emit(DocEvent::LoopRangeChanged);
        }
        self.emit_state_change();
    }

    /// `stopLoopRecording()`: end the take (a take without hits that created / grew the pattern
    /// still gets its 'Loop Record' step).
    pub fn stop_loop_take(&mut self) {
        self.end_loop_take(true);
    }

    /// The pattern-merge half of `LoopRecorder.captureHit` (LR:292-330): first change pushes the
    /// take's 'Loop Record' step; the pattern region is ensured; the replace pass (overdub off)
    /// clears the pattern on the take's first hit; the hit (`quantized` = loop-relative beat,
    /// velocity on 0..1 or MIDI 0..127) is merged (louder wins). The caller handles suppression,
    /// held gates and playback (`added && !grown && !cleared` → add to the running Part, else
    /// reschedule).
    pub fn loop_take_record_hit(
        &mut self,
        pad: PadIndex,
        velocity: f64,
        quantized: f64,
        duration_beats: f64,
    ) -> Option<LoopHit> {
        let take = self.loop_take.as_ref()?;
        let ti = self.track_index(&take.track_id)?;
        if pad >= PAD_COUNT {
            return None;
        }
        let take = self.loop_take.as_mut().expect("checked above");
        if !take.undo_pushed {
            take.undo_pushed = true;
            let snapshot = take.undo_snapshot.clone();
            self.undo.push_undo("Loop Record", snapshot);
        }
        let overdub = self.beat().r#loop.overdub;
        let (ri, grown) = self.ensure_pattern_region_on(ti);
        let take = self.loop_take.as_mut().expect("checked above");
        let pattern_start = take.pattern_start;
        let replace = !overdub && !take.replaced;
        take.replaced = true;
        let region_start = self.arrangement.tracks[ti].regions[ri].start_beat;
        let mut cleared = false;
        if replace {
            let in_pattern = self.pattern_note_filter(region_start, None, None);
            let region = &mut self.regions_mut(ti)[ri];
            let before = region.notes.len();
            region.notes.retain(|n| !in_pattern(n));
            cleared = region.notes.len() != before;
        }
        let hit = RegionNote {
            note: f64::from(pad_to_note(pad)),
            velocity: clamp01(if velocity > 1.0 {
                velocity / 127.0
            } else {
                velocity
            }),
            start_beat: pattern_start - region_start + quantized,
            duration_beats,
            is_drum: true,
        };
        let track_id = self.arrangement.tracks[ti].id.clone();
        let region = &mut self.regions_mut(ti)[ri];
        let before = region.notes.len();
        region.notes = merge_drum_hit(&region.notes, hit.clone());
        let added = region.notes.len() == before + 1;
        let out = LoopHit {
            track_id,
            region_id: region.id.clone(),
            region_start_beat: region_start,
            note: hit,
            added,
            grown,
            cleared,
        };
        self.emit_state_change();
        Some(out)
    }

    /// The note-update half of `LoopRecorder.captureRelease` (LR:357-370): set the length of the
    /// pad note at `start_beat` (± STEP_EPSILON) in the take track's region. True when changed
    /// (then reschedules the track).
    pub fn loop_take_set_gate_length(
        &mut self,
        region_id: &str,
        note: f64,
        start_beat: f64,
        duration_beats: f64,
    ) -> bool {
        let Some(track_id) = self.loop_take.as_ref().map(|t| t.track_id.clone()) else {
            return false;
        };
        let Some(ti) = self.track_index(&track_id) else {
            return false;
        };
        let Some(region) = self.regions_mut(ti).iter_mut().find(|r| r.id == region_id) else {
            return false;
        };
        let mut changed = false;
        for n in region.notes.iter_mut() {
            // JS: `n.note !== note || Math.abs(...) >= STEP_EPSILON` keeps the note (NaN distance matches).
            let d = (n.start_beat - start_beat).abs();
            if n.note == note && (d < STEP_EPSILON || d.is_nan()) {
                n.duration_beats = duration_beats;
                changed = true;
            }
        }
        if !changed {
            return false;
        }
        self.emit_state_change();
        self.emit(DocEvent::RescheduleTrack(track_id));
        true
    }
}
