//! Regions, notes (piano roll), quantize, clipboard, automation, markers, audio import and the
//! data half of Song recording (ArrangementEngine.ts AE:645-831, 908-988, 1665-1792, 1964-2090,
//! 2262-2280).

use serde::{Deserialize, Serialize};

use crate::automation::{delete_automation_point, set_automation_point};
use crate::piano_roll::{QuantizeOptions, quantize_notes};
use crate::types::{
    AudioRegionData, AutomationLane, AutomationParameter, InstrumentType, LoopMarkers, Marker,
    Region, RegionNote, SampleRef,
};

use super::{
    ArrangementDoc, DocEvent, MARKER_COLORS, UnavailableAudioRegion, js_max, js_min, js_round,
};

/// The region clipboard (`copyRegion` / `pasteRegion`).
#[derive(Debug, Clone, PartialEq)]
pub struct RegionClipboard {
    pub region: Region,
    pub track_id: String,
}

/// `Partial<Pick<Marker, 'name' | 'beat' | 'color'>>`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MarkerUpdate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beat: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// JS truthiness of an optional name (`region.name ? ... : undefined`).
fn truthy(name: &Option<String>) -> Option<&str> {
    name.as_deref().filter(|n| !n.is_empty())
}

/// Stable sort by beat (`(a, b) => a.beat - b.beat`, NaN compares equal).
fn sort_markers(markers: &mut [Marker]) {
    markers.sort_by(|a, b| {
        (a.beat - b.beat)
            .partial_cmp(&0.0)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

impl ArrangementDoc {
    fn region_index(&self, ti: usize, region_id: &str) -> Option<usize> {
        self.arrangement.tracks[ti]
            .regions
            .iter()
            .position(|r| r.id == region_id)
    }

    fn region_mut(&mut self, track_id: &str, region_id: &str) -> Option<&mut Region> {
        let ti = self.track_index(track_id)?;
        let ri = self.region_index(ti, region_id)?;
        Some(&mut self.arrangement.tracks[ti].regions[ri])
    }

    // --- Region operations ---

    /// `addRegion(trackId, region)` — undo 'Add Region'; auto-extends.
    pub fn add_region(&mut self, track_id: &str, region: Region) {
        self.push_undo_snapshot("Add Region");
        let Some(ti) = self.track_index(track_id) else {
            return;
        };
        self.arrangement.tracks[ti].regions.push(region);
        self.auto_extend_length();
        self.emit_state_change();
    }

    /// `removeRegion` — undo 'Remove Region'.
    pub fn remove_region(&mut self, track_id: &str, region_id: &str) {
        self.push_undo_snapshot("Remove Region");
        let Some(ti) = self.track_index(track_id) else {
            return;
        };
        self.arrangement.tracks[ti]
            .regions
            .retain(|r| r.id != region_id);
        self.emit_state_change();
    }

    /// `moveRegion(t, r, start)` — undo 'Move Region'; start ≥ 0; auto-extends.
    pub fn move_region(&mut self, track_id: &str, region_id: &str, new_start_beat: f64) {
        self.push_undo_snapshot("Move Region");
        let Some(region) = self.region_mut(track_id, region_id) else {
            return;
        };
        region.start_beat = js_max(0.0, new_start_beat);
        self.auto_extend_length();
        self.emit_state_change();
    }

    /// `moveRegionToTrack(from, to, regionId)` — undo 'Move Region to Track' (no type check, no
    /// auto-extend).
    pub fn move_region_to_track(
        &mut self,
        from_track_id: &str,
        to_track_id: &str,
        region_id: &str,
    ) {
        self.push_undo_snapshot("Move Region to Track");
        let (Some(from), Some(to)) = (
            self.track_index(from_track_id),
            self.track_index(to_track_id),
        ) else {
            return;
        };
        let Some(ri) = self.region_index(from, region_id) else {
            return;
        };
        let region = self.arrangement.tracks[from].regions.remove(ri);
        self.arrangement.tracks[to].regions.push(region);
        self.emit_state_change();
    }

    /// `splitRegion(t, r, beat)` — undo 'Split Region'. No-op outside `(0, len)`. Audio: the right
    /// half's offset moves by the split in seconds. MIDI: spanning notes are cut. " (L)" / " (R)".
    pub fn split_region(&mut self, track_id: &str, region_id: &str, split_beat: f64) {
        self.push_undo_snapshot("Split Region");
        let Some(ti) = self.track_index(track_id) else {
            return;
        };
        let Some(ri) = self.region_index(ti, region_id) else {
            return;
        };
        let bpm = self.arrangement.bpm;
        let region = &self.arrangement.tracks[ti].regions[ri];
        let local_split = split_beat - region.start_beat;
        if local_split <= 0.0 || local_split >= region.length_beats {
            return;
        }
        let right_name = truthy(&region.name).map(|n| format!("{n} (R)"));
        let left_name = truthy(&region.name).map(|n| format!("{n} (L)"));

        if let Some(audio) = &region.audio {
            let local_split_seconds = (local_split / bpm) * 60.0;
            let right = Region {
                id: String::new(),
                start_beat: split_beat,
                length_beats: region.length_beats - local_split,
                notes: Vec::new(),
                color: region.color.clone(),
                name: right_name,
                audio: Some(AudioRegionData {
                    offset_seconds: audio.offset_seconds + local_split_seconds,
                    ..audio.clone()
                }),
            };
            let right = Region {
                id: self.next_id(),
                ..right
            };
            let region = &mut self.arrangement.tracks[ti].regions[ri];
            region.length_beats = local_split;
            if left_name.is_some() {
                region.name = left_name;
            }
            self.arrangement.tracks[ti].regions.push(right);
            self.emit_state_change();
            return;
        }

        let mut left_notes = Vec::new();
        let mut right_notes = Vec::new();
        for note in &region.notes {
            if note.start_beat + note.duration_beats <= local_split {
                left_notes.push(note.clone());
            } else if note.start_beat >= local_split {
                right_notes.push(RegionNote {
                    start_beat: note.start_beat - local_split,
                    ..note.clone()
                });
            } else {
                left_notes.push(RegionNote {
                    duration_beats: local_split - note.start_beat,
                    ..note.clone()
                });
                right_notes.push(RegionNote {
                    start_beat: 0.0,
                    duration_beats: note.start_beat + note.duration_beats - local_split,
                    ..note.clone()
                });
            }
        }
        let right = Region {
            id: String::new(),
            start_beat: split_beat,
            length_beats: region.length_beats - local_split,
            notes: right_notes,
            color: region.color.clone(),
            name: right_name,
            audio: None,
        };
        let right = Region {
            id: self.next_id(),
            ..right
        };
        let region = &mut self.arrangement.tracks[ti].regions[ri];
        region.length_beats = local_split;
        region.notes = left_notes;
        if left_name.is_some() {
            region.name = left_name;
        }
        self.arrangement.tracks[ti].regions.push(right);
        self.emit_state_change();
    }

    /// `duplicateRegion` — undo 'Duplicate Region'. Placed right after, name " (copy)".
    pub fn duplicate_region(&mut self, track_id: &str, region_id: &str) {
        self.push_undo_snapshot("Duplicate Region");
        let Some(ti) = self.track_index(track_id) else {
            return;
        };
        let Some(ri) = self.region_index(ti, region_id) else {
            return;
        };
        let region = &self.arrangement.tracks[ti].regions[ri];
        let template = Region {
            start_beat: region.start_beat + region.length_beats,
            name: truthy(&region.name).map(|n| format!("{n} (copy)")),
            ..region.clone()
        };
        let duplicate = Region {
            id: self.next_id(),
            ..template
        };
        self.arrangement.tracks[ti].regions.push(duplicate);
        self.auto_extend_length();
        self.emit_state_change();
    }

    /// `addAudioRegion(trackId, sampleId, startBeat)` — undo 'Add Audio Region'. `sample` is
    /// `sampleManager.getSample(sampleId)` (None = not loaded). Audio tracks only.
    pub fn add_audio_region(
        &mut self,
        track_id: &str,
        sample: Option<&SampleRef>,
        start_beat: f64,
    ) -> Option<Region> {
        self.push_undo_snapshot("Add Audio Region");
        let ti = self.track_index(track_id)?;
        if self.arrangement.tracks[ti].instrument.kind != InstrumentType::Audio {
            return None;
        }
        let sample = sample?;
        let length_beats = (sample.duration_seconds / 60.0) * self.arrangement.bpm;
        let region = Region {
            id: self.next_id(),
            start_beat,
            length_beats,
            notes: Vec::new(),
            color: Some(self.arrangement.tracks[ti].color.clone()),
            name: Some(sample.name.clone()),
            audio: Some(AudioRegionData {
                sample_id: sample.id.clone(),
                sample_path: sample.path.clone(),
                offset_seconds: 0.0,
                gain_db: 0.0,
                original_duration: sample.duration_seconds,
            }),
        };
        self.arrangement.tracks[ti].regions.push(region.clone());
        self.auto_extend_length();
        self.emit_state_change();
        Some(region)
    }

    /// The per-file body of `importAudioFile()`: a new audio track named after the sample (renamed
    /// without its own undo entry) holding the sample at beat 0.
    pub fn import_audio_sample(&mut self, sample: &SampleRef) -> Option<Region> {
        let track = self.add_track(InstrumentType::Audio, 0);
        if let Some(ti) = self.track_index(&track.id) {
            self.arrangement.tracks[ti].name = sample.name.clone();
        }
        self.add_audio_region(&track.id, Some(sample), 0.0)
    }

    /// `getUnavailableAudioRegions()` with `isSampleUnavailable` injected.
    pub fn get_unavailable_audio_regions(
        &self,
        is_unavailable: impl Fn(&str) -> bool,
    ) -> Vec<UnavailableAudioRegion> {
        let mut out = Vec::new();
        for track in &self.arrangement.tracks {
            for region in &track.regions {
                if let Some(audio) = &region.audio
                    && !audio.sample_id.is_empty()
                    && is_unavailable(&audio.sample_id)
                {
                    out.push(UnavailableAudioRegion {
                        track_id: track.id.clone(),
                        region_id: region.id.clone(),
                        sample_id: audio.sample_id.clone(),
                    });
                }
            }
        }
        out
    }

    // --- Notes (piano roll) ---

    /// `updateRegionNotes` — undo 'Edit Notes' (no live reschedule).
    pub fn update_region_notes(&mut self, track_id: &str, region_id: &str, notes: Vec<RegionNote>) {
        self.push_undo_snapshot("Edit Notes");
        let Some(region) = self.region_mut(track_id, region_id) else {
            return;
        };
        region.notes = notes;
        self.emit_state_change();
    }

    /// `editRegionNotes(t, r, notes, label = 'Edit Notes', newLength?)`: one undo step (validated
    /// first); may only grow the region; reschedules the running Part.
    pub fn edit_region_notes(
        &mut self,
        track_id: &str,
        region_id: &str,
        notes: Vec<RegionNote>,
        label: Option<&str>,
        new_length_beats: Option<f64>,
    ) {
        if self.region_mut(track_id, region_id).is_none() {
            return;
        }
        self.push_undo_snapshot(label.unwrap_or("Edit Notes"));
        let region = self.region_mut(track_id, region_id).expect("checked above");
        region.notes = notes;
        if let Some(len) = new_length_beats
            && len > region.length_beats
        {
            region.length_beats = len;
            self.auto_extend_length();
        }
        self.emit_state_change();
        self.emit(DocEvent::RescheduleTrack(track_id.to_owned()));
    }

    /// `resizeRegion(t, r, start?, len?)` — undo 'Trim Region'. Audio: a left trim shifts the
    /// offset. MIDI: a left trim shifts / drops / clips notes, a right trim drops / clips. Min 0.25.
    pub fn resize_region(
        &mut self,
        track_id: &str,
        region_id: &str,
        new_start_beat: Option<f64>,
        new_length_beats: Option<f64>,
    ) {
        self.push_undo_snapshot("Trim Region");
        let bpm = self.arrangement.bpm;
        let Some(region) = self.region_mut(track_id, region_id) else {
            return;
        };
        if let Some(audio) = &mut region.audio {
            if let Some(start) = new_start_beat {
                let clamped = js_max(0.0, start);
                let trim = clamped - region.start_beat;
                if trim != 0.0 {
                    audio.offset_seconds += (trim / bpm) * 60.0;
                    region.start_beat = clamped;
                }
            }
            if let Some(len) = new_length_beats {
                region.length_beats = js_max(0.25, len);
            }
            self.auto_extend_length();
            self.emit_state_change();
            return;
        }
        if let Some(start) = new_start_beat {
            let clamped = js_max(0.0, start);
            let trim = clamped - region.start_beat;
            if trim != 0.0 {
                region.notes = region
                    .notes
                    .iter()
                    .map(|n| RegionNote {
                        start_beat: n.start_beat - trim,
                        ..n.clone()
                    })
                    .filter(|n| n.start_beat + n.duration_beats > 0.0)
                    .map(|n| {
                        if n.start_beat < 0.0 {
                            RegionNote {
                                duration_beats: n.duration_beats + n.start_beat,
                                start_beat: 0.0,
                                ..n
                            }
                        } else {
                            n
                        }
                    })
                    .collect();
                region.start_beat = clamped;
            }
        }
        if let Some(len) = new_length_beats {
            let new_len = js_max(0.25, len);
            region.notes = region
                .notes
                .iter()
                .filter(|n| n.start_beat < new_len)
                .map(|n| {
                    if n.start_beat + n.duration_beats > new_len {
                        RegionNote {
                            duration_beats: new_len - n.start_beat,
                            ..n.clone()
                        }
                    } else {
                        n.clone()
                    }
                })
                .collect();
            region.length_beats = new_len;
        }
        self.auto_extend_length();
        self.emit_state_change();
    }

    /// `quantizeRegionNotes(t, r, snap)` — undo 'Quantize Notes' (`quantizeNotes`).
    pub fn quantize_region_notes(&mut self, track_id: &str, region_id: &str, snap_value: f64) {
        self.push_undo_snapshot("Quantize Notes");
        let Some(region) = self.region_mut(track_id, region_id) else {
            return;
        };
        region.notes = quantize_notes(&region.notes, snap_value, &QuantizeOptions::default());
        self.emit_state_change();
    }

    /// `quantizeSelectedNotes(t, r, indices, snap)` — undo 'Quantize Selected Notes':
    /// `round(start / snap) * snap` on the selected notes.
    pub fn quantize_selected_notes(
        &mut self,
        track_id: &str,
        region_id: &str,
        note_indices: &[usize],
        snap_value: f64,
    ) {
        self.push_undo_snapshot("Quantize Selected Notes");
        let Some(region) = self.region_mut(track_id, region_id) else {
            return;
        };
        for (i, note) in region.notes.iter_mut().enumerate() {
            if note_indices.contains(&i) {
                note.start_beat = js_round(note.start_beat / snap_value) * snap_value;
            }
        }
        self.emit_state_change();
    }

    // --- Clipboard ---

    /// `copyRegion` (no undo).
    pub fn copy_region(&mut self, track_id: &str, region_id: &str) {
        let Some(ti) = self.track_index(track_id) else {
            return;
        };
        let Some(ri) = self.region_index(ti, region_id) else {
            return;
        };
        let region = self.arrangement.tracks[ti].regions[ri].clone();
        self.clipboard = Some(RegionClipboard {
            region,
            track_id: track_id.to_owned(),
        });
    }

    /// `pasteRegion(trackId, atBeat)` — undo 'Paste Region' (only with a clipboard).
    pub fn paste_region(&mut self, track_id: &str, at_beat: f64) {
        let Some(clip) = self.clipboard.clone() else {
            return;
        };
        self.push_undo_snapshot("Paste Region");
        let Some(ti) = self.track_index(track_id) else {
            return;
        };
        let pasted = Region {
            id: self.next_id(),
            start_beat: at_beat,
            ..clip.region
        };
        self.arrangement.tracks[ti].regions.push(pasted);
        self.auto_extend_length();
        self.emit_state_change();
    }

    pub fn has_clipboard(&self) -> bool {
        self.clipboard.is_some()
    }

    // --- Automation ---

    fn lane_mut(
        &mut self,
        track_id: &str,
        parameter: AutomationParameter,
    ) -> Option<&mut AutomationLane> {
        let ti = self.track_index(track_id)?;
        self.arrangement.tracks[ti]
            .automation
            .as_mut()?
            .iter_mut()
            .find(|l| l.parameter == parameter)
    }

    /// `addAutomationLane` — undo 'Add Automation Lane'; no duplicates.
    pub fn add_automation_lane(&mut self, track_id: &str, parameter: AutomationParameter) {
        self.push_undo_snapshot("Add Automation Lane");
        let Some(ti) = self.track_index(track_id) else {
            return;
        };
        let lanes = self.arrangement.tracks[ti]
            .automation
            .get_or_insert_with(Vec::new);
        if lanes.iter().any(|l| l.parameter == parameter) {
            return;
        }
        lanes.push(AutomationLane {
            parameter,
            points: Vec::new(),
            visible: true,
        });
        self.emit_state_change();
    }

    /// `removeAutomationLane` — undo 'Remove Automation Lane'; resets the effect nodes.
    pub fn remove_automation_lane(&mut self, track_id: &str, parameter: AutomationParameter) {
        self.push_undo_snapshot("Remove Automation Lane");
        let Some(ti) = self.track_index(track_id) else {
            return;
        };
        let Some(lanes) = &mut self.arrangement.tracks[ti].automation else {
            return;
        };
        lanes.retain(|l| l.parameter != parameter);
        self.emit(DocEvent::TrackEffectsReset(track_id.to_owned()));
        self.emit_state_change();
    }

    /// `setAutomationPoint(t, p, beat, value, snap)` — undo 'Set Automation Point'.
    pub fn set_automation_point(
        &mut self,
        track_id: &str,
        parameter: AutomationParameter,
        beat: f64,
        value: f64,
        snap_value: f64,
    ) {
        self.push_undo_snapshot("Set Automation Point");
        let Some(lane) = self.lane_mut(track_id, parameter) else {
            return;
        };
        lane.points = set_automation_point(&lane.points, beat, value, snap_value);
        self.emit_state_change();
    }

    /// `deleteAutomationPoint(t, p, index)` — undo 'Delete Automation Point'.
    pub fn delete_automation_point(
        &mut self,
        track_id: &str,
        parameter: AutomationParameter,
        point_index: usize,
    ) {
        self.push_undo_snapshot("Delete Automation Point");
        let Some(lane) = self.lane_mut(track_id, parameter) else {
            return;
        };
        lane.points = delete_automation_point(&lane.points, point_index);
        self.emit_state_change();
    }

    /// `toggleAutomationLaneVisibility` (no undo).
    pub fn toggle_automation_lane_visibility(
        &mut self,
        track_id: &str,
        parameter: AutomationParameter,
    ) {
        let Some(lane) = self.lane_mut(track_id, parameter) else {
            return;
        };
        lane.visible = !lane.visible;
        self.emit_state_change();
    }

    // --- Markers ---

    pub fn get_markers(&self) -> &[Marker] {
        self.arrangement.markers.as_deref().unwrap_or(&[])
    }

    /// `addMarker(name, beat, color?)` — undo 'Add Marker'. Sorted by beat; colour cycles.
    pub fn add_marker(&mut self, name: &str, beat: f64, color: Option<&str>) -> Marker {
        self.push_undo_snapshot("Add Marker");
        let count = self.arrangement.markers.get_or_insert_with(Vec::new).len();
        let color = color.map_or_else(
            || MARKER_COLORS[count % MARKER_COLORS.len()].to_owned(),
            str::to_owned,
        );
        let marker = Marker {
            id: self.next_id(),
            name: name.to_owned(),
            beat: js_max(0.0, beat),
            color,
        };
        let markers = self.arrangement.markers.get_or_insert_with(Vec::new);
        markers.push(marker.clone());
        sort_markers(markers);
        self.emit_state_change();
        marker
    }

    /// `removeMarker(id)` — undo 'Remove Marker'.
    pub fn remove_marker(&mut self, id: &str) {
        self.push_undo_snapshot("Remove Marker");
        let Some(markers) = &mut self.arrangement.markers else {
            return;
        };
        markers.retain(|m| m.id != id);
        self.emit_state_change();
    }

    /// `updateMarker(id, updates)` — undo 'Update Marker'. Beat ≥ 0; re-sorted.
    pub fn update_marker(&mut self, id: &str, updates: &MarkerUpdate) {
        self.push_undo_snapshot("Update Marker");
        let Some(markers) = &mut self.arrangement.markers else {
            return;
        };
        let Some(marker) = markers.iter_mut().find(|m| m.id == id) else {
            return;
        };
        if let Some(name) = &updates.name {
            marker.name = name.clone();
        }
        if let Some(beat) = updates.beat {
            marker.beat = js_max(0.0, beat);
        }
        if let Some(color) = &updates.color {
            marker.color = color.clone();
        }
        sort_markers(markers);
        self.emit_state_change();
    }

    /// `seekToMarker(id)` target: the marker's beat.
    pub fn marker_seek_target(&self, id: &str) -> Option<f64> {
        self.get_markers()
            .iter()
            .find(|m| m.id == id)
            .map(|m| m.beat)
    }

    /// `seekToNextMarker()` target from the current position: the first marker after it (+0.01).
    pub fn next_marker_target(&self, current_beat: f64) -> Option<f64> {
        self.get_markers()
            .iter()
            .find(|m| m.beat > current_beat + 0.01)
            .map(|m| m.beat)
    }

    /// `seekToPreviousMarker()` target: the last marker before the position (−0.01).
    pub fn previous_marker_target(&self, current_beat: f64) -> Option<f64> {
        self.get_markers()
            .iter()
            .rev()
            .find(|m| m.beat < current_beat - 0.01)
            .map(|m| m.beat)
    }

    // --- Song recording (data half of stopRecordingToTrack, AE:908-988) ---

    /// Commit a Song recording: undo 'Record' (pushed even when nothing was captured), then a new
    /// region of the captured notes (already closed by the engine). Punch (`punch` = the loop
    /// markers): the region covers the markers and replaces overlapping regions; otherwise it
    /// starts at `recording_start_beat` with length `max(1, ceil(end_beat))`.
    pub fn commit_song_recording(
        &mut self,
        track_id: &str,
        captured: Vec<RegionNote>,
        punch: Option<LoopMarkers>,
        recording_start_beat: f64,
        end_beat: f64,
    ) -> Option<Region> {
        self.push_undo_snapshot("Record");
        let ti = self.track_index(track_id)?;
        if captured.is_empty() {
            return None;
        }
        let (start, length) = match punch {
            Some(m) => {
                self.arrangement.tracks[ti].regions.retain(|r| {
                    r.start_beat + r.length_beats <= m.start_beat || r.start_beat >= m.end_beat
                });
                (m.start_beat, m.end_beat - m.start_beat)
            }
            None => (recording_start_beat, js_max(1.0, end_beat.ceil())),
        };
        let region = Region {
            id: self.next_id(),
            start_beat: start,
            length_beats: length,
            notes: captured,
            color: Some(self.arrangement.tracks[ti].color.clone()),
            name: None,
            audio: None,
        };
        self.arrangement.tracks[ti].regions.push(region.clone());
        self.auto_extend_length();
        self.emit_state_change();
        Some(region)
    }

    // --- Loop range ---

    /// `getNoteRangeLoopMarkers()`.
    pub(crate) fn get_note_range_loop_markers(&self) -> Option<LoopMarkers> {
        let mut min_beat = f64::INFINITY;
        let mut max_beat = f64::NEG_INFINITY;
        for track in &self.arrangement.tracks {
            for region in &track.regions {
                for note in &region.notes {
                    let start = region.start_beat + note.start_beat;
                    let end = start + js_max(0.125, note.duration_beats);
                    min_beat = js_min(min_beat, start);
                    max_beat = js_max(max_beat, end);
                }
            }
        }
        if !min_beat.is_finite() || !max_beat.is_finite() {
            return None;
        }
        let length = self.arrangement.length_beats;
        let max_start = js_max(0.0, length - 1.0);
        let start_beat = js_min(max_start, js_max(0.0, min_beat));
        let end_beat = js_min(length, js_max(start_beat + 1.0, max_beat));
        Some(LoopMarkers {
            start_beat,
            end_beat,
        })
    }

    /// `setLoopEnabled(enabled)` (no undo). Turning it on with no markers / markers covering the
    /// whole song sets them to the note range, else `[0, length)`.
    pub fn set_loop_enabled(&mut self, enabled: bool) {
        self.loop_enabled = enabled;
        if enabled {
            let mut markers = self.arrangement.loop_markers;
            let full = |m: &LoopMarkers| {
                m.start_beat <= 0.0 && m.end_beat >= self.arrangement.length_beats
            };
            if markers.as_ref().is_none_or(full)
                && let Some(range) = self.get_note_range_loop_markers()
            {
                markers = Some(range);
                self.arrangement.loop_markers = Some(range);
            }
            if markers.is_none() {
                self.arrangement.loop_markers = Some(LoopMarkers {
                    start_beat: 0.0,
                    end_beat: self.arrangement.length_beats,
                });
            }
        }
        self.emit(DocEvent::LoopRangeChanged);
        self.emit_state_change();
    }

    /// `setLoopMarkers(s, e)` — undo 'Set Loop Markers'. `start ≥ 0`, `end = min(len, max(s+1, e))`.
    pub fn set_loop_markers(&mut self, start_beat: f64, end_beat: f64) {
        self.push_undo_snapshot("Set Loop Markers");
        self.arrangement.loop_markers = Some(LoopMarkers {
            start_beat: js_max(0.0, start_beat),
            end_beat: js_min(
                self.arrangement.length_beats,
                js_max(start_beat + 1.0, end_beat),
            ),
        });
        self.emit(DocEvent::LoopRangeChanged);
        self.emit_state_change();
    }

    /// `setBpm(bpm)` (no undo).
    pub fn set_bpm(&mut self, bpm: f64) {
        self.arrangement.bpm = bpm;
        self.emit(DocEvent::BpmChanged(bpm));
        self.emit_state_change();
    }
}
