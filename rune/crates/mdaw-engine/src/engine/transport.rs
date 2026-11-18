//! Transport (`AE:1121-1299, 2306-2314, 2558-2569`).
//!
//! Kept from the TS: `play()` always starts at beat 0; `playFrom` clamps to ≥ 0; `seek` only while
//! playing; `stop()` ends a loop take / a Song recording / playback; stopping clears every
//! schedule and audio player, fades the drum voices (`stopAll`), silences the count-in, ends
//! pattern playback and re-applies the Song loop; `playPattern` loops the pattern range
//! transiently (U6: markers and the loop toggle untouched); the metronome clicks when
//! `metronomeEnabled || ((loopRec || patternPlayback) && beat.loop.metronome)`; Song playback with
//! no loop range stops past `lengthBeats` (checked in [`ArrangementEngine::tick`]).
//!
//! Deviations (timing model of the core, see `core.rs`): a start / seek takes effect at the next
//! render quantum instead of 100 ms later; so R5-N4 (a seek before the scheduled start) needs no
//! stop + restart: the core moves the pending start beat, with the same audible result. The
//! position reads the new beat at once (as the TS `transportStart.beat` / `seconds` do) until the
//! core has applied the command.

use mdaw_model::LoopMarkers;

use super::{ArrangementEngine, PendingPosition, TransportState};
use crate::core::{ClockSnapshot, Phase};

impl ArrangementEngine {
    pub fn get_state(&self) -> TransportState {
        self.state
    }

    pub fn is_pattern_playback(&self) -> bool {
        self.pattern_playback
    }

    pub fn is_metronome_enabled(&self) -> bool {
        self.metronome_enabled
    }

    /// `getPosition()`: 0 when stopped, else the transport beat (from the core's ticks).
    pub fn get_position(&self) -> f64 {
        if self.state == TransportState::Stopped {
            return 0.0;
        }
        self.transport_beat_now()
    }

    /// The transport beat in `snap`: a start / seek the core has not applied yet reads its beat.
    fn transport_beat_in(&self, snap: &ClockSnapshot) -> f64 {
        if let Some(p) = self.pending_position
            && snap.generation < p.generation
        {
            return p.beat;
        }
        snap.beat
    }

    /// The transport beat now (one consistent clock snapshot).
    fn transport_beat_now(&self) -> f64 {
        self.transport_beat_in(&self.h.shared().snapshot())
    }

    /// `transportBeatAt(time)`: before the scheduled start, the start beat (B1); else the beat at
    /// `time` extrapolated from the playhead at the core's clock (live note times are within a
    /// frame of it). Frames, beat, phase, tempo and generation come from ONE snapshot.
    pub(super) fn transport_beat_at(&self, time: Option<f64>) -> f64 {
        let snap = self.h.shared().snapshot();
        let now = snap.now();
        let at = time.filter(|t| t.is_finite()).unwrap_or(now);
        if let Some((t, beat)) = self.transport_start
            && self.state != TransportState::Stopped
            && at < t
        {
            return beat;
        }
        let beat = self.transport_beat_in(&snap);
        if snap.phase == Phase::Started {
            beat + (at - now) * snap.bpm / 60.0
        } else {
            beat
        }
    }

    /// The documented track limit: an explicit error (and a UI event) instead of silently
    /// dropping tracks. Checked before playback, recording and export.
    pub fn check_track_limit(&mut self) -> Result<(), super::TrackLimitError> {
        let tracks = self.doc.get_tracks().len();
        if tracks <= super::MAX_TRACKS {
            return Ok(());
        }
        let max = super::MAX_TRACKS;
        self.ui_events
            .push(super::EngineEvent::TrackLimitExceeded { tracks, max });
        Err(super::TrackLimitError { tracks, max })
    }

    /// `getActiveLoopRange()`.
    pub fn get_active_loop_range(&self) -> Option<LoopMarkers> {
        self.doc.get_active_loop_range(self.pattern_playback)
    }

    /// `applyLoopToTransport()` (in beats: the core keeps it in ticks across tempo changes).
    pub(super) fn apply_loop_to_transport(&mut self) {
        let range = self
            .get_active_loop_range()
            .map(|m| (m.start_beat, m.end_beat));
        self.h.set_loop(range);
    }

    /// `play()`: always from beat 0.
    pub fn play(&mut self) {
        self.pump();
        self.start_playback(0.0);
    }

    /// `playFrom(beat)`.
    pub fn play_from(&mut self, beat: f64) {
        self.start_playback(if beat.is_finite() { beat.max(0.0) } else { 0.0 });
    }

    pub(super) fn start_playback(&mut self, from_beat: f64) {
        if self.state != TransportState::Stopped || self.check_track_limit().is_err() {
            return;
        }
        self.state = TransportState::Playing;
        self.h.set_bpm(self.doc.get_bpm());
        self.schedule_all_tracks();
        self.apply_loop_to_transport();
        if self.should_click() {
            self.h.set_metronome(true);
        }
        self.start_transport_at(from_beat);
        self.doc.notify_state_change();
    }

    /// Start at `beat` at the next render quantum; remember it (`transportStart`).
    pub(super) fn start_transport_at(&mut self, beat: f64) -> u64 {
        let generation = self.h.start(None, beat);
        self.transport_start = Some((self.h.now(), beat));
        self.pending_position = Some(PendingPosition { generation, beat });
        generation
    }

    /// `stop()`.
    pub fn stop(&mut self) {
        self.pump();
        if self.doc.is_loop_recording() {
            self.stop_loop_recording();
            return;
        }
        if self.state == TransportState::Recording {
            self.stop_recording_to_track();
            return;
        }
        self.stop_transport();
        self.doc.notify_state_change();
    }

    /// `stopTransport()`.
    pub(super) fn stop_transport(&mut self) {
        self.h.stop();
        self.transport_start = None;
        self.pending_position = None;
        self.clear_schedules();
        self.h.set_metronome(false);
        self.state = TransportState::Stopped;
        self.pattern_playback = false;
        self.apply_loop_to_transport();
    }

    /// `setPosition(beat)`.
    pub fn set_position(&mut self, beat: f64) {
        if !beat.is_finite() {
            return;
        }
        let generation = self.h.seek(beat);
        self.pending_position = Some(PendingPosition { generation, beat });
        let now = self.h.now();
        if let Some((t, b)) = self.transport_start.as_mut()
            && now < *t
        {
            *b = beat;
        }
    }

    /// `seek(beat)`: only while playing.
    pub fn seek(&mut self, beat: f64) {
        if self.state != TransportState::Playing {
            return;
        }
        self.set_position(beat.max(0.0));
    }

    /// `setMetronomeEnabled(enabled)`.
    pub fn set_metronome_enabled(&mut self, enabled: bool) {
        if self.metronome_enabled == enabled {
            return;
        }
        self.metronome_enabled = enabled;
        self.update_metronome();
        self.doc.notify_state_change();
    }

    /// `shouldClick()`.
    pub(super) fn should_click(&self) -> bool {
        if self.metronome_enabled {
            return true;
        }
        (self.doc.is_loop_recording() || self.pattern_playback)
            && self.doc.get_loop_settings().metronome
    }

    /// `updateMetronome()`.
    pub(super) fn update_metronome(&mut self) {
        if !self.should_click() {
            self.h.set_metronome(false);
        } else if self.state != TransportState::Stopped {
            self.h.set_metronome(true);
        }
    }

    /// `playPattern()`: Beat Play loops the pattern range transiently (U6).
    pub fn play_pattern(&mut self) {
        self.pump();
        if self.state != TransportState::Stopped || self.doc.is_loop_recording() {
            return;
        }
        let range = self.doc.prepare_play_pattern();
        self.pattern_playback = true;
        self.play_from(range.start_beat);
        if self.state == TransportState::Stopped {
            self.pattern_playback = false;
            self.apply_loop_to_transport();
        }
    }
}
