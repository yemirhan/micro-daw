//! Linear (Song) recording (`startRecordingToTrack` / `stopRecordingToTrack` /
//! `captureNoteOn/Off` / `getLiveRegion`, `AE:386-426, 833-1055`).
//!
//! Kept from the TS: the track falls back to the armed one (no tracks at all: a synth track is
//! created and armed); only from 'stopped'; punch mode when the Song loop is on with markers (the
//! transport loops the markers, recording starts at the marker start, positions are stored as
//! `(beat − loopStart) % loopLen`; 50 ms before each loop end the notes still open are closed with
//! `max(0.125, loopLen − start)`); otherwise from beat 0. Only the global metronome toggle
//! clicks. Quirk kept: the other tracks are NOT scheduled while recording (`startRecordingToTrack`
//! never calls `scheduleAllTracks`). Drum notes get 0.25 beat; synth notes are open until their
//! note-off (`max(0.125, off − on)`). Stop pushes 'Record' (even when nothing was captured) and
//! adds the region (punch: the marker range, replacing overlapping regions; else from the start,
//! `max(1, ceil(endBeat))` long); punch appends the notes of every pass (quirk kept).
//!
//! Pad hits are read from the core's captures (frame-exact, transport beat from ticks); synth
//! notes come through [`ArrangementEngine::capture_note_on`] / `capture_note_off` with an
//! optional context time.

use mdaw_model::{LoopMarkers, Region, RegionNote, pad_to_note};

use super::{ArrangementEngine, TransportState};
use crate::contract::PPQ;
use crate::core::{CaptureKind, PadCapture};

/// Id of the synthetic region `getLiveRegion` returns.
pub const LIVE_REGION_ID: &str = "__live__";
/// The punch loop-close callback runs this long before the loop end (transport time).
const PUNCH_CLOSE_SEC: f64 = 0.05;

#[derive(Debug, Clone)]
struct Open {
    note: f64,
    start_beat: f64,
    velocity: f64,
    is_drum: bool,
    on_time: f64,
}

/// `recordingTrackId`, `recordingStartBeat`, `recordingOpenNotes`, `capturedNotes`, `punchRecording`.
#[derive(Debug, Clone, Default)]
pub(super) struct SongRec {
    pub track_id: Option<String>,
    start_beat: f64,
    punch: bool,
    generation: u64,
    /// Insertion-ordered like a JS Map.
    open: Vec<Open>,
    captured: Vec<RegionNote>,
}

impl ArrangementEngine {
    pub fn get_recording_track_id(&self) -> Option<&str> {
        self.song.track_id.as_deref()
    }

    pub fn is_punch_recording_active(&self) -> bool {
        self.song.punch
    }

    /// `startRecordingToTrack(trackId?)`.
    pub fn start_recording_to_track(&mut self, track_id: Option<&str>) {
        self.pump();
        let armed = self.doc.get_armed_track_id().map(str::to_owned);
        let resolved = track_id.map(str::to_owned).or(armed);
        if resolved.is_none() && self.doc.get_tracks().is_empty() {
            let id = self.edit(|d| {
                let t = d.add_track(mdaw_model::InstrumentType::Synth, 0);
                d.arm_track_for_recording(Some(&t.id));
                t.id
            });
            return self.start_recording_to_track(Some(&id));
        }
        let Some(id) = resolved else { return };
        if !self.doc.get_tracks().iter().any(|t| t.id == id)
            || self.state != TransportState::Stopped
            || self.check_track_limit().is_err()
        {
            return;
        }
        self.recording_incomplete = false;
        self.song = SongRec {
            track_id: Some(id),
            ..SongRec::default()
        };
        self.state = TransportState::Recording;
        self.h.set_bpm(self.doc.get_bpm());
        let markers = self.doc.get_loop_markers();
        match markers.filter(|_| self.doc.is_loop_enabled()) {
            Some(m) => {
                self.song.punch = true;
                self.song.start_beat = m.start_beat;
                self.h.set_loop(Some((m.start_beat, m.end_beat)));
                // `transport.schedule(close, max(0, loopEndSec − 0.05))`: Tone converts the
                // seconds to ticks at the tempo of now (`toTicks` rounds).
                let bpm = self.doc.get_bpm();
                let sec = (m.end_beat * 60.0 / bpm - PUNCH_CLOSE_SEC).max(0.0);
                let tick = crate::core::js_round(sec * bpm / 60.0 * PPQ as f64) as i64;
                self.h.set_punch_close(Some(tick));
            }
            None => {
                self.song.punch = false;
                self.song.start_beat = 0.0;
            }
        }
        if self.metronome_enabled {
            self.h.set_metronome(true);
        }
        let start = self.song.start_beat;
        self.song.generation = self.start_transport_at(start);
        self.doc.notify_state_change();
    }

    fn punch_markers(&self) -> Option<LoopMarkers> {
        self.doc.get_loop_markers().filter(|_| self.song.punch)
    }

    /// Recording-relative position of a transport beat.
    fn relative_beat(&self, beat: f64) -> f64 {
        match self.punch_markers() {
            Some(m) => (beat - m.start_beat) % (m.end_beat - m.start_beat),
            None => beat - self.song.start_beat,
        }
    }

    fn song_note_on(&mut self, note: f64, velocity: f64, is_drum: bool, beat: f64, time: f64) {
        let rel = self.relative_beat(beat);
        if is_drum {
            self.song.captured.push(RegionNote {
                note,
                velocity,
                start_beat: rel,
                duration_beats: 0.25,
                is_drum: true,
            });
            return;
        }
        let open = Open {
            note,
            start_beat: rel,
            velocity,
            is_drum: false,
            on_time: time,
        };
        match self.song.open.iter_mut().find(|o| o.note == note) {
            Some(o) => *o = open,
            None => self.song.open.push(open),
        }
    }

    fn song_note_off(&mut self, note: f64, beat: f64) {
        let Some(i) = self.song.open.iter().position(|o| o.note == note) else {
            return;
        };
        let rel = self.relative_beat(beat);
        let o = self.song.open.remove(i);
        self.song.captured.push(RegionNote {
            note,
            velocity: o.velocity,
            start_beat: o.start_beat,
            duration_beats: (rel - o.start_beat).max(0.125),
            is_drum: false,
        });
    }

    /// `captureNoteOn(note, velocity, false, time?)`: a synth note (pads are captured by the
    /// core). The loop recorder ignores synth notes.
    pub fn capture_note_on(&mut self, note: f64, velocity: f64, time: Option<f64>) {
        self.pump();
        if self.doc.is_loop_recording() || self.state != TransportState::Recording {
            return;
        }
        let at = time.unwrap_or_else(|| self.h.now());
        let beat = self.transport_beat_at(Some(at));
        self.song_note_on(note, velocity, false, beat, at);
    }

    /// `captureNoteOff(note, time?)`.
    pub fn capture_note_off(&mut self, note: f64, time: Option<f64>) {
        self.pump();
        if self.doc.is_loop_recording() || self.state != TransportState::Recording {
            return;
        }
        let beat = self.transport_beat_at(time);
        self.song_note_off(note, beat);
    }

    /// A pad capture while Song recording (drum notes; pad releases close nothing).
    pub(super) fn song_capture(&mut self, c: &PadCapture) {
        if self.song.track_id.is_none() || c.generation < self.song.generation {
            return;
        }
        let beat = match self.transport_start {
            Some((t, b)) if c.time < t => b,
            _ => c.beat,
        };
        let note = f64::from(pad_to_note(c.pad));
        match c.kind {
            CaptureKind::Hit | CaptureKind::RepeatHit => {
                self.song_note_on(note, c.velocity, true, beat, c.time)
            }
            CaptureKind::Release => self.song_note_off(note, beat),
        }
    }

    /// Punch: the transport callback 50 ms before the loop end (`CoreEvent::PunchClose`,
    /// emitted by the core at that tick on every pass, in order with the pad captures) closes the
    /// notes open by then with `max(0.125, loopLen − start)`. A note-off read after it (even one
    /// before the loop end) finds its note already closed, as in the TS.
    pub(super) fn song_on_punch_close(&mut self, close_at: f64, generation: u64) {
        if self.state != TransportState::Recording
            || !self.song.punch
            || generation < self.song.generation
        {
            return;
        }
        let Some(m) = self.punch_markers() else {
            return;
        };
        let len = m.end_beat - m.start_beat;
        let (closing, keep): (Vec<Open>, Vec<Open>) = std::mem::take(&mut self.song.open)
            .into_iter()
            .partition(|o| o.on_time <= close_at);
        self.song.open = keep;
        for o in closing {
            self.song.captured.push(RegionNote {
                note: o.note,
                velocity: o.velocity,
                start_beat: o.start_beat,
                duration_beats: (len - o.start_beat).max(0.125),
                is_drum: o.is_drum,
            });
        }
    }

    /// `stopRecordingToTrack()`: the new region (None when nothing was captured).
    pub fn stop_recording_to_track(&mut self) -> Option<Region> {
        self.pump();
        if self.state != TransportState::Recording {
            return None;
        }
        let track_id = self.song.track_id.clone()?;
        let end_beat = self.transport_beat_at(None);
        let punch = self.punch_markers();
        let open = std::mem::take(&mut self.song.open);
        for o in open {
            let dur = match punch {
                Some(m) => (m.end_beat - m.start_beat - o.start_beat).max(0.125),
                None => ((end_beat - self.song.start_beat) - o.start_beat).max(0.125),
            };
            self.song.captured.push(RegionNote {
                note: o.note,
                velocity: o.velocity,
                start_beat: o.start_beat,
                duration_beats: dur,
                is_drum: o.is_drum,
            });
        }
        let captured = std::mem::take(&mut self.song.captured);
        let start = self.song.start_beat;
        self.song = SongRec::default();
        self.stop_transport();
        let region =
            self.edit(|d| d.commit_song_recording(&track_id, captured, punch, start, end_beat));
        self.doc.notify_state_change();
        region
    }

    /// `getLiveRegion()`: the captured + held notes so far, as a synthetic region.
    pub fn get_live_region(&self) -> Option<Region> {
        if self.state != TransportState::Recording {
            return None;
        }
        let track_id = self.song.track_id.as_ref()?;
        let beat = self.transport_beat_at(None);
        let punch = self.punch_markers();
        let (start, length) = match punch {
            Some(m) => (m.start_beat, m.end_beat - m.start_beat),
            None => (
                self.song.start_beat,
                (beat - self.song.start_beat).max(0.25),
            ),
        };
        let mut notes = self.song.captured.clone();
        for o in &self.song.open {
            let elapsed = match punch {
                Some(m) => {
                    ((beat - m.start_beat) % (m.end_beat - m.start_beat) - o.start_beat).max(0.125)
                }
                None => ((beat - self.song.start_beat) - o.start_beat).max(0.125),
            };
            notes.push(RegionNote {
                note: o.note,
                velocity: o.velocity,
                start_beat: o.start_beat,
                duration_beats: elapsed,
                is_drum: o.is_drum,
            });
        }
        let color = self
            .doc
            .get_tracks()
            .iter()
            .find(|t| &t.id == track_id)
            .map(|t| t.color.clone());
        Some(Region {
            id: LIVE_REGION_ID.to_owned(),
            start_beat: start,
            length_beats: length,
            notes,
            color,
            name: None,
            audio: None,
            extra: Default::default(),
        })
    }
}
