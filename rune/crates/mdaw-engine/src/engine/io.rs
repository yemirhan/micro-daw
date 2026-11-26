//! Live input, arming, hosted plugins and audio takes (control side).
//!
//! **Arming.** Every armed track is heard live and recorded by Song recording
//! (`ArrangementDoc::armed_track_ids`): an armed *audio* track gets its device input
//! ([`mdaw_model::TrackInput`]) mixed into its slot, through its insert plugins and its strip
//! (an amp simulator on a guitar input); the keys play every armed *synth* track's instrument
//! ([`SynthTargets`], read by the MIDI thread) instead of the live synth; an armed drum track is
//! the active drum track (the pads' sink, as before).
//!
//! **Plugins.** A track's insert plugins become an [`InsertChain`] in its slot and a synth
//! track's plugin instrument a [`PluginSynth`], both from the app's [`PluginFactory`]. A plugin
//! the factory cannot give (not installed) is skipped: the track plays without it and keeps
//! the slot in the document.
//!
//! **Audio takes.** Song recording records the dry input of every armed audio track
//! ([`crate::io::record_pair`]) from the moment the recording is started; on stop each take is
//! cut to the transport start (the core's `Started` time), shifted earlier by the round-trip
//! latency the app reports ([`ArrangementEngine::set_record_latency`]: what was played against
//! the click is heard and captured that much later), and handed to the app as a
//! [`RecordedTake`], which stores the audio and adds the region.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use mdaw_model::{InstrumentType, Track};

use super::{ArrangementEngine, TransportState};
use crate::core::{SlotInput, SynthNoteOp};
use crate::fx::track::StripOp;
use crate::io::{InputSource, InputStats, TakeReader, record_pair};
use crate::plugin::{Insert, InsertChain, PluginFactory};

/// Whether a track's strip keeps its two channels apart ([`StripOp::Stereo`]): tracks with
/// stereo sources of their own (hosted plugins, a stereo input). The built-in instruments keep
/// the Electron app's mono strip.
pub fn track_is_stereo(track: &Track) -> bool {
    !track.plugins.is_empty()
        || track.instrument.plugin.is_some()
        || track.input.is_some_and(|i| i.stereo)
}

/// Most synth tracks the keys play at once.
pub const MAX_KEY_TARGETS: usize = 8;

/// Where live keys go: the armed synth tracks' slots, else the first synth track's, else (a
/// project without one) the live synth. Written by the UI
/// thread, read by the MIDI thread per note (lock-free).
pub struct SynthTargets {
    live: usize,
    count: AtomicUsize,
    slots: [AtomicUsize; MAX_KEY_TARGETS],
}

impl SynthTargets {
    pub fn new(live_slot: usize) -> Self {
        Self {
            live: live_slot,
            count: AtomicUsize::new(0),
            slots: std::array::from_fn(|_| AtomicUsize::new(0)),
        }
    }

    /// The live synth's slot (the target while no synth track is armed).
    pub fn live_slot(&self) -> usize {
        self.live
    }

    /// The armed synth tracks' slots (empty: the live synth plays).
    pub fn armed(&self) -> Vec<usize> {
        let n = self.count.load(Ordering::Acquire).min(MAX_KEY_TARGETS);
        self.slots[..n]
            .iter()
            .map(|s| s.load(Ordering::Relaxed))
            .collect()
    }

    /// Call `f` with every slot a key event goes to.
    pub fn for_each(&self, mut f: impl FnMut(usize)) {
        let n = self.count.load(Ordering::Acquire).min(MAX_KEY_TARGETS);
        if n == 0 {
            f(self.live);
            return;
        }
        for s in &self.slots[..n] {
            f(s.load(Ordering::Relaxed));
        }
    }

    fn set(&self, slots: &[usize]) {
        // A reader between the two stores sees fewer targets, never a stale slot.
        self.count.store(0, Ordering::Release);
        let n = slots.len().min(MAX_KEY_TARGETS);
        for (cell, slot) in self.slots.iter().zip(&slots[..n]) {
            cell.store(*slot, Ordering::Relaxed);
        }
        self.count.store(n, Ordering::Release);
    }
}

/// A finished audio take (see the module docs): the app writes it to a file, registers the
/// sample and adds the region at `start_beat`.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordedTake {
    pub track_id: String,
    pub start_beat: f64,
    pub sample_rate: u32,
    /// One or two channels of the same length (never empty channels).
    pub channels: Vec<Vec<f32>>,
    /// Frames the take lost to a stalled UI thread (0: complete).
    pub dropped_frames: u64,
}

/// A take being recorded.
pub(super) struct AudioTake {
    track_id: String,
    slot: usize,
    reader: TakeReader,
}

/// The device input as the engine knows it.
#[derive(Default)]
pub(super) struct InputState {
    channels: usize,
    stats: Option<Arc<InputStats>>,
}

/// Seconds of audio a take's queue holds between two UI frames.
const TAKE_QUEUE_SECONDS: f64 = 4.0;

impl ArrangementEngine {
    // --- Plugins ---------------------------------------------------------------------------

    /// The app's plugin manager. Every track's plugins are rebuilt from it.
    pub fn set_plugin_factory(&mut self, factory: Box<dyn PluginFactory>) {
        self.plugins = factory;
        self.with_playback_restart(|e| {
            e.rebuild_all_audio();
            e.sync_drum_sink();
        });
    }

    /// The factory (the app reaches its plugin manager through it).
    pub fn plugin_factory(&mut self) -> &mut dyn PluginFactory {
        &mut *self.plugins
    }

    /// Rebuild a track's insert chain from the document.
    pub(super) fn sync_inserts(&mut self, track: &Track) {
        let Some(slot) = self.slot_of(&track.id) else {
            return;
        };
        let sr = self.sr;
        let inserts: Vec<Insert> = track
            .plugins
            .iter()
            .filter_map(|p| {
                self.plugins.processor(p, sr).map(|processor| Insert {
                    processor,
                    bypassed: p.bypassed,
                })
            })
            .collect();
        let chain = (!inserts.is_empty()).then(|| InsertChain::new(inserts));
        self.h.set_inserts(slot, chain);
        self.h
            .strip_op(slot, StripOp::Stereo(track_is_stereo(track)));
    }

    pub(super) fn sync_inserts_of(&mut self, track_id: &str) {
        if let Some(t) = self
            .doc
            .get_tracks()
            .iter()
            .find(|t| t.id == track_id)
            .cloned()
        {
            self.sync_inserts(&t);
        }
    }

    // --- Input and arming ------------------------------------------------------------------

    /// The device input the core reads (None: no input device). The armed audio tracks follow.
    pub fn set_input_source(&mut self, source: Option<InputSource>) {
        self.input = InputState {
            channels: source.as_ref().map_or(0, InputSource::channels),
            stats: source.as_ref().map(|s| s.stats().clone()),
        };
        self.h.set_input(source);
        self.h.flush();
    }

    /// Channels of the device input (0: none).
    pub fn input_channels(&self) -> usize {
        self.input.channels
    }

    /// The input path's counters and per-channel peaks (None: no input device).
    pub fn input_stats(&self) -> Option<&Arc<InputStats>> {
        self.input.stats.as_ref()
    }

    /// Where the MIDI thread sends live keys (see [`SynthTargets`]).
    pub fn synth_targets(&self) -> Arc<SynthTargets> {
        self.synth_targets.clone()
    }

    /// Apply the document's arming to the core: input monitoring per armed audio track, the
    /// keys' targets. Called after every change of the armed set, of a track's input, and after
    /// track audio was (re)built.
    pub(super) fn sync_arm(&mut self) {
        let armed: Vec<String> = self
            .doc
            .armed_track_ids()
            .into_iter()
            .map(str::to_owned)
            .collect();
        let mut keys = Vec::new();
        let mut inputs = Vec::new();
        for t in self.doc.get_tracks() {
            let Some(ta) = self.tracks.get(&t.id) else {
                continue;
            };
            let is_armed = armed.contains(&t.id);
            match t.instrument.kind {
                InstrumentType::Audio => {
                    let input = is_armed.then(|| {
                        let i = t.input_or_default();
                        SlotInput {
                            left: i.channel as usize,
                            right: i.stereo.then_some(i.channel as usize + 1),
                            monitor: true,
                        }
                    });
                    if ta.input != input {
                        inputs.push((t.id.clone(), ta.slot, input));
                    }
                }
                InstrumentType::Synth if is_armed => keys.push(ta.slot),
                _ => {}
            }
        }
        for (id, slot, input) in inputs {
            self.h.set_slot_input(slot, input);
            if let Some(ta) = self.tracks.get_mut(&id) {
                ta.input = input;
            }
        }
        // No synth track armed: the keys play the project's first synth track, so they are never
        // on a sound outside the project (the live synth, whose picker and Live FX strip left
        // the UI on 2026-11-26). It stays the target only while the project has no synth track.
        if keys.is_empty()
            && let Some(slot) = self
                .doc
                .get_tracks()
                .iter()
                .filter(|t| t.instrument.kind == InstrumentType::Synth)
                .find_map(|t| self.tracks.get(&t.id).map(|ta| ta.slot))
        {
            keys.push(slot);
        }
        keys.truncate(MAX_KEY_TARGETS);
        let old = self.synth_targets.armed();
        if old != keys {
            // Keys held on a target that goes away would never get their note-off there.
            if old.is_empty() {
                self.h
                    .synth_note(self.synth_targets.live_slot(), SynthNoteOp::ReleaseAll, 0.0);
            }
            for slot in old.iter().filter(|s| !keys.contains(s)) {
                self.h.synth_note(*slot, SynthNoteOp::ReleaseAll, 0.0);
            }
            self.synth_targets.set(&keys);
        }
        self.h.flush();
    }

    // --- Audio takes -----------------------------------------------------------------------

    /// Round-trip latency compensation of recorded audio, in seconds (output latency + input
    /// latency + the input queue's cushion): a take is placed that much earlier.
    pub fn set_record_latency(&mut self, seconds: f64) {
        self.record_latency = if seconds.is_finite() {
            seconds.clamp(0.0, 1.0)
        } else {
            0.0
        };
    }

    pub fn record_latency(&self) -> f64 {
        self.record_latency
    }

    /// The armed audio tracks (id, slot, stereo input) a Song recording would record.
    pub(super) fn armed_audio_tracks(&self) -> Vec<(String, usize, bool)> {
        self.doc
            .armed_track_ids()
            .into_iter()
            .filter_map(|id| {
                let t = self.doc.get_tracks().iter().find(|t| t.id == id)?;
                (t.instrument.kind == InstrumentType::Audio).then_some(())?;
                let slot = self.tracks.get(id)?.slot;
                Some((id.to_owned(), slot, t.input_or_default().stereo))
            })
            .collect()
    }

    /// Start a take on each of `tracks` (before the transport start command, so each take
    /// begins at or before the start).
    pub(super) fn begin_audio_takes(&mut self, tracks: Vec<(String, usize, bool)>) {
        self.takes.clear();
        self.take_start = None;
        for (track_id, slot, stereo) in tracks {
            let (tap, reader) = record_pair(stereo, self.sr, TAKE_QUEUE_SECONDS);
            self.h.set_slot_record(slot, Some(tap));
            self.takes.push(AudioTake {
                track_id,
                slot,
                reader,
            });
        }
    }

    /// The transport of the running recording started at context time `time`.
    pub(super) fn audio_takes_started(&mut self, time: f64) {
        if !self.takes.is_empty() && self.take_start.is_none() {
            self.take_start = Some(time);
        }
    }

    /// Drain what the core recorded (every UI frame).
    pub(super) fn pump_audio_takes(&mut self) {
        for t in &mut self.takes {
            t.reader.pump();
        }
    }

    /// End the running takes at the current frame and queue them for the app
    /// ([`Self::take_recorded_audio`]). `start_beat`: where the recording started.
    pub(super) fn finish_audio_takes(&mut self, start_beat: f64) {
        if self.takes.is_empty() {
            return;
        }
        // Everything up to "now" is in the queues before the taps are removed.
        let end_frame = self.h.shared().frames();
        for t in &self.takes {
            self.h.set_slot_record(t.slot, None);
        }
        self.h.flush();
        let sr = self.sr;
        let start_time = self.take_start.take();
        let latency = (self.record_latency * sr as f64).round() as i64;
        for t in std::mem::take(&mut self.takes) {
            let dropped = t.reader.dropped_frames();
            let first = t.reader.first_frame();
            let mut channels = t.reader.into_channels();
            let (Some(first), Some(start_time)) = (first, start_time) else {
                continue; // stopped before the transport (or the take) started
            };
            let start_frame = (start_time * sr as f64).round() as i64;
            // `skip`: frames of the take before the transport start as the player heard it
            // (what is played on the first beat is captured `latency` later). `keep`: the take
            // ends now (its last frame was played `latency` before the stop).
            let skip = start_frame - first as i64 + latency;
            let keep = (end_frame as i64 - first as i64).max(0) as usize;
            for c in &mut channels {
                c.truncate(keep.min(c.len()));
                if skip > 0 {
                    c.drain(..(skip as usize).min(c.len()));
                } else if skip < 0 {
                    c.splice(0..0, std::iter::repeat_n(0.0, (-skip) as usize));
                }
            }
            if channels.first().is_none_or(Vec::is_empty) {
                continue;
            }
            if dropped > 0 {
                self.recording_incomplete = true;
            }
            self.recorded.push(RecordedTake {
                track_id: t.track_id,
                start_beat,
                sample_rate: sr,
                channels,
                dropped_frames: dropped,
            });
        }
    }

    /// The audio takes finished since the last call (see [`RecordedTake`]).
    pub fn take_recorded_audio(&mut self) -> Vec<RecordedTake> {
        std::mem::take(&mut self.recorded)
    }

    /// The tracks recording audio right now, with the beat their takes start at.
    pub fn recording_audio_tracks(&self) -> Vec<(&str, f64)> {
        if self.state != TransportState::Recording {
            return Vec::new();
        }
        let start = self.song_start_beat();
        self.takes
            .iter()
            .map(|t| (t.track_id.as_str(), start))
            .collect()
    }

    /// The live waveform of the take being recorded on a track: one peak per
    /// [`crate::io::TAKE_PEAK_FRAMES`] frames since the take began.
    pub fn live_take_peaks(&self, track_id: &str) -> &[f32] {
        self.takes
            .iter()
            .find(|t| t.track_id == track_id)
            .map_or(&[], |t| t.reader.peaks())
    }
}
