//! `ArrangementEngine`, control side (UI thread): wires the [`ArrangementDoc`] (data + undo) to
//! the realtime [`Core`] (port of the audio/transport half of `src/services/ArrangementEngine.ts`,
//! `src/services/arrangement/LoopRecorder.ts` and `src/services/DrumEngine.ts`).
//!
//! **Driving it.** Every doc edit goes through [`ArrangementEngine::edit`] (or one of the
//! transport / recording methods), after which the doc's [`DocEvent`] outbox is turned into core
//! commands: instruments per track (a `SamplerKit` per drum track through its
//! [`KitController`]), schedules ([`TrackSchedule::from_track`]), gain / pan / mute / solo, the
//! loop range, the tempo, the metronome (`shouldClick`) and the live drum sink. The UI calls
//! [`ArrangementEngine::tick`] every frame: it reads the core's events (pad captures for the loop
//! / Song recorders, loop wraps for punch recording), finished pad loads, the count-in, and the
//! Song auto-stop. Time is the core's clock (context seconds = frames rendered / rate), so `tick`
//! takes no argument.
//!
//! **Live pads.** Pad hits go straight from the MIDI thread to the core ([`LiveInput`]), which
//! plays them on the live sink (the active drum track's kit, else drumEngine's fallback factory
//! kit) and stamps a capture event; the recorders read those captures here, so a hit is placed by
//! the frame it sounded at, whatever the UI latency.
//!
//! See `engine/transport.rs`, `engine/loop_rec.rs`, `engine/song_rec.rs`, `engine/tracks.rs`
//! and `engine/export_wav.rs` for the TS semantics kept and the deviations.

mod audio_regions;
mod export_wav;
mod loop_rec;
mod reverb_leak;
mod song_rec;
mod tracks;
mod transport;

#[cfg(test)]
mod test_rig;
#[cfg(test)]
mod tests_beat;
#[cfg(test)]
mod tests_export;
#[cfg(test)]
mod tests_loop;
#[cfg(test)]
mod tests_pads;
#[cfg(test)]
mod tests_transport;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use mdaw_dsp::buffer::SharedBuffer;
use mdaw_model::doc::{ArrangementDoc, DocEvent, KitSelectedEvent, UnavailableAudioRegion};
use mdaw_model::migrate::DroppedDrumNotes;

use crate::buffers::PadBufferCache;
use crate::core::{Core, CoreConfig, CoreEvent, CoreHandle, LiveInput, PadCapture};
use crate::sampler::control::KitController;

pub use audio_regions::{AudioRegions, RegionPlay, db_gain};
pub use export_wav::{ExportError, export_wav, render_export};
pub use song_rec::LIVE_REGION_ID;
pub use tracks::FALLBACK_GAIN_DB;

use loop_rec::LoopRec;
use song_rec::SongRec;
use tracks::{Fallback, TrackAudio};

/// The engine's transport state (`ArrangementTransportState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportState {
    Stopped,
    Playing,
    Recording,
}

/// Song audio-region samples (the SampleManager port provides them): the decoded buffer at the
/// device rate and the N5 availability flag.
pub trait AudioSamples: Send + Sync {
    fn buffer(&self, sample_id: &str) -> Option<SharedBuffer>;
    fn is_unavailable(&self, _sample_id: &str) -> bool {
        false
    }
}

/// No audio samples (beat-only setups, tests).
#[derive(Debug, Clone, Copy, Default)]
pub struct NoSamples;

impl AudioSamples for NoSamples {
    fn buffer(&self, _: &str) -> Option<SharedBuffer> {
        None
    }
}

/// A UI-facing event read from the core (pad flashes, clicks, transport).
#[derive(Debug, Clone, PartialEq)]
pub enum EngineEvent {
    /// A pattern drum note fired on a track (pad flash).
    PatternHit {
        track_id: String,
        pad: usize,
        time: f64,
    },
    /// A live (or repeat) pad hit sounded.
    PadHit {
        pad: usize,
        velocity: f64,
        time: f64,
    },
    MetronomeClick {
        time: f64,
        beat: f64,
        accent: bool,
    },
    CountInClick {
        time: f64,
        remaining: u32,
    },
}

/// Position override until the core applied a transport command (start / seek): the TS reads the
/// new position at once (`transportStart.beat`, `transport.seconds`).
#[derive(Debug, Clone, Copy)]
struct PendingPosition {
    generation: u64,
    beat: f64,
}

/// See the module docs.
pub struct ArrangementEngine {
    doc: ArrangementDoc,
    h: CoreHandle,
    sr: u32,
    cache: PadBufferCache,
    samples: Arc<dyn AudioSamples>,
    tracks: HashMap<String, TrackAudio>,
    free_slots: Vec<usize>,
    fallback: Fallback,
    sink_slot: Option<usize>,
    state: TransportState,
    pattern_playback: bool,
    metronome_enabled: bool,
    /// `transportStart`: (context time, beat) of the start this engine scheduled.
    transport_start: Option<(f64, f64)>,
    pending_position: Option<PendingPosition>,
    rec: Rc<RefCell<Option<LoopRec>>>,
    song: SongRec,
    kit_selected: Vec<KitSelectedEvent>,
    warnings: Vec<DroppedDrumNotes>,
    ui_events: Vec<EngineEvent>,
    last_count_in: u32,
    #[cfg(test)]
    fail_next_start: bool,
    #[cfg(test)]
    capture_log: Vec<PadCapture>,
}

impl ArrangementEngine {
    /// An engine at `sample_rate` with a fresh doc (default arrangement). Returns the engine (UI
    /// thread), the core (audio thread / offline) and the MIDI thread's pad input.
    pub fn new(
        sample_rate: u32,
        cache: PadBufferCache,
        samples: Arc<dyn AudioSamples>,
    ) -> (Self, Core, LiveInput) {
        Self::with_doc(ArrangementDoc::new(), sample_rate, cache, samples)
    }

    /// Same with a given doc (its arrangement gets its track audio built).
    pub fn with_doc(
        doc: ArrangementDoc,
        sample_rate: u32,
        cache: PadBufferCache,
        samples: Arc<dyn AudioSamples>,
    ) -> (Self, Core, LiveInput) {
        let (core, mut h, live) = Core::new(CoreConfig::new(sample_rate));
        h.set_master_bus(Some(1.0));
        let max = CoreConfig::new(sample_rate).max_tracks;
        let fallback = Fallback::new(&mut h, max - 1, sample_rate, &cache);
        let mut e = Self {
            doc,
            h,
            sr: sample_rate,
            cache,
            samples,
            tracks: HashMap::new(),
            free_slots: (0..max - 1).rev().collect(),
            fallback,
            sink_slot: None,
            state: TransportState::Stopped,
            pattern_playback: false,
            metronome_enabled: false,
            transport_start: None,
            pending_position: None,
            rec: Rc::new(RefCell::new(None)),
            song: SongRec::default(),
            kit_selected: Vec::new(),
            warnings: Vec::new(),
            ui_events: Vec::new(),
            last_count_in: 0,
            #[cfg(test)]
            fail_next_start: false,
            #[cfg(test)]
            capture_log: Vec::new(),
        };
        e.install_take_end_hook();
        e.doc.drain_events();
        e.h.set_bpm(e.doc.get_bpm());
        e.rebuild_all_audio();
        e.sync_drum_sink();
        e.apply_loop_to_transport();
        (e, core, live)
    }

    // --- Doc access -----------------------------------------------------------------------

    /// The document (read-only).
    pub fn doc(&self) -> &ArrangementDoc {
        &self.doc
    }

    /// Run a doc edit (any `ArrangementDoc` method: edits, undo/redo, kits, steps, loop
    /// settings, projects...), then apply its audio / transport side effects. Pending core events
    /// (pad captures) are read first, so a take ends with every hit already recorded.
    pub fn edit<R>(&mut self, f: impl FnOnce(&mut ArrangementDoc) -> R) -> R {
        self.pump();
        let r = f(&mut self.doc);
        self.flush_doc();
        r
    }

    /// The core handle (raw commands, e.g. tests or the live keyboard path).
    pub fn handle(&mut self) -> &mut CoreHandle {
        &mut self.h
    }

    pub fn sample_rate(&self) -> u32 {
        self.sr
    }

    /// The atomics the core publishes (playhead, meters, phase...).
    pub fn shared(&self) -> &crate::core::Shared {
        self.h.shared()
    }

    /// The core's clock (context seconds).
    pub fn now(&self) -> f64 {
        self.h.now()
    }

    /// `onKitSelected` events since the last call (N2).
    pub fn take_kit_selected(&mut self) -> Vec<KitSelectedEvent> {
        std::mem::take(&mut self.kit_selected)
    }

    /// Migration warnings since the last call.
    pub fn take_warnings(&mut self) -> Vec<DroppedDrumNotes> {
        std::mem::take(&mut self.warnings)
    }

    /// Pad flashes / clicks read from the core since the last call.
    pub fn take_ui_events(&mut self) -> Vec<EngineEvent> {
        std::mem::take(&mut self.ui_events)
    }

    // --- Sample status (N5) ---------------------------------------------------------------

    pub fn get_unavailable_audio_regions(&self) -> Vec<UnavailableAudioRegion> {
        let samples = self.samples.clone();
        self.doc
            .get_unavailable_audio_regions(|id| samples.is_unavailable(id))
    }

    pub fn is_sample_unavailable(&self, sample_id: &str) -> bool {
        self.samples.is_unavailable(sample_id)
    }

    // --- Frame driver ---------------------------------------------------------------------

    /// Call every UI frame: reads core events (recorders), finished pad loads, the count-in, and
    /// stops Song playback past the song end (`AE:1471-1486`). Returns the playhead.
    pub fn tick(&mut self) -> f64 {
        self.pump();
        self.count_in_tick();
        let beat = self.get_position();
        if self.state == TransportState::Playing
            && self.get_active_loop_range().is_none()
            && beat >= self.doc.get_arrangement().length_beats
        {
            self.stop();
            return self.get_position();
        }
        beat
    }

    /// Read everything the core sent, apply finished loads, free garbage.
    fn pump(&mut self) {
        while let Some(e) = self.h.poll_event() {
            self.on_core_event(e);
        }
        self.poll_kits();
        self.h.collect_garbage();
    }

    fn on_core_event(&mut self, e: CoreEvent) {
        match e {
            CoreEvent::Capture(c) => {
                #[cfg(test)]
                self.capture_log.push(c);
                if !matches!(c.kind, crate::core::CaptureKind::Release) {
                    self.ui_events.push(EngineEvent::PadHit {
                        pad: c.pad,
                        velocity: c.velocity,
                        time: c.time,
                    });
                }
                self.on_capture(&c);
            }
            CoreEvent::LoopWrap { time, .. } => self.song_on_wrap(time),
            CoreEvent::PatternHit { slot, pad, time } => {
                if let Some(id) = self.track_of_slot(slot) {
                    self.ui_events.push(EngineEvent::PatternHit {
                        track_id: id,
                        pad,
                        time,
                    });
                }
            }
            CoreEvent::MetronomeClick { time, beat, accent } => self
                .ui_events
                .push(EngineEvent::MetronomeClick { time, beat, accent }),
            CoreEvent::CountInClick {
                time, remaining, ..
            } => self
                .ui_events
                .push(EngineEvent::CountInClick { time, remaining }),
            CoreEvent::Started { .. } | CoreEvent::Stopped { .. } | CoreEvent::Seeked { .. } => {}
        }
    }

    /// A pad capture: the loop recorder during a take, else the Song recorder while recording.
    fn on_capture(&mut self, c: &PadCapture) {
        if self.doc.is_loop_recording() {
            self.loop_capture(c);
        } else if self.state == TransportState::Recording {
            self.song_capture(c);
        }
    }

    /// Apply the doc's pending side effects (in order; handlers may emit more).
    fn flush_doc(&mut self) {
        loop {
            let events = self.doc.drain_events();
            if events.is_empty() {
                break;
            }
            for e in events {
                self.on_doc_event(e);
            }
        }
    }

    fn on_doc_event(&mut self, e: DocEvent) {
        match e {
            DocEvent::TrackAdded(id) => {
                if let Some(t) = self.doc.get_tracks().iter().find(|t| t.id == id).cloned() {
                    self.create_track_audio(&t);
                }
            }
            DocEvent::TrackRemoved(id) => self.dispose_track_audio(&id),
            DocEvent::TrackAudioRebuilt(id) => self.with_playback_restart(|e| {
                e.dispose_track_audio(&id);
                if let Some(t) = e.doc.get_tracks().iter().find(|t| t.id == id).cloned() {
                    e.create_track_audio(&t);
                }
                e.sync_drum_sink();
            }),
            DocEvent::TrackVolumeChanged { track_id, db } => {
                if let Some(slot) = self.slot_of(&track_id) {
                    self.h.set_track_gain(slot, db_gain(db));
                }
            }
            DocEvent::TrackMuteChanged { track_id, muted } => {
                if let Some(slot) = self.slot_of(&track_id) {
                    self.h.set_track_mute(slot, muted);
                }
            }
            DocEvent::TrackPanChanged { track_id, pan } => {
                if let Some(slot) = self.slot_of(&track_id) {
                    self.h.set_track_pan(slot, pan as f32);
                }
            }
            DocEvent::SoloChanged => self.update_solo_state(),
            // Phase 4: effect parameters (the strip runs the chain at its disabled defaults).
            DocEvent::TrackEffectChanged { .. } | DocEvent::TrackEffectsReset(_) => {}
            DocEvent::RescheduleTrack(id) => self.refresh_playback_schedule(&id),
            DocEvent::KitApplied(id) => self.apply_kit(&id),
            DocEvent::KitSelected(k) => self.kit_selected.push(k),
            DocEvent::SyncDrumSink => self.sync_drum_sink(),
            DocEvent::LoopRangeChanged => self.apply_loop_to_transport(),
            DocEvent::BpmChanged(bpm) => {
                self.h.set_bpm(bpm);
                self.apply_loop_to_transport();
            }
            DocEvent::MetronomeUpdate => self.update_metronome(),
            DocEvent::SwingChanged => {}
            DocEvent::RebuildAll => self.with_playback_restart(|e| {
                e.rebuild_all_audio();
                e.sync_drum_sink();
            }),
            DocEvent::LoopTakeEnded { .. } => {
                self.stop_transport();
                self.doc.notify_state_change();
            }
            DocEvent::MigrationWarning(w) => self.warnings.push(w),
        }
    }

    /// `withPlaybackRestart` (the take was already ended by the doc): while playing, remember
    /// the beat and the pattern flag, stop, change, play from the beat.
    fn with_playback_restart(&mut self, change: impl FnOnce(&mut Self)) {
        if self.state != TransportState::Playing {
            change(self);
            return;
        }
        let beat = self.get_position();
        let pattern = self.pattern_playback;
        self.stop_transport();
        change(self);
        self.pattern_playback = pattern;
        self.play_from(beat);
    }

    fn track_of_slot(&self, slot: usize) -> Option<String> {
        self.tracks
            .iter()
            .find(|(_, t)| t.slot == slot)
            .map(|(id, _)| id.clone())
    }

    fn slot_of(&self, track_id: &str) -> Option<usize> {
        self.tracks.get(track_id).map(|t| t.slot)
    }

    /// The core slot a track plays in (tests / meters).
    pub fn track_slot(&self, track_id: &str) -> Option<usize> {
        self.slot_of(track_id)
    }

    /// `getTrackLevel`: dB of the track's output over the last render quantum (RMS).
    pub fn get_track_level(&self, track_id: &str) -> f64 {
        match self.slot_of(track_id) {
            Some(slot) => {
                let ms = self.h.shared().slot_mean_square(slot) as f64;
                if ms > 0.0 {
                    10.0 * ms.log10()
                } else {
                    f64::NEG_INFINITY
                }
            }
            None => f64::NEG_INFINITY,
        }
    }

    /// The pad buffer cache every kit of this engine loads through.
    pub fn cache(&self) -> &PadBufferCache {
        &self.cache
    }

    fn kit_controller(&mut self, track_id: &str) -> Option<&mut KitController> {
        self.tracks.get_mut(track_id).and_then(|t| t.kit.as_mut())
    }
}
