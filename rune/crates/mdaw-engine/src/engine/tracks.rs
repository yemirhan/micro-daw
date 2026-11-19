//! Per-track audio (≙ `createTrackAudio` / `disposeTrackAudio` / `scheduleAllTracks` /
//! `refreshPlaybackSchedule` / `updateSoloState` / `syncDrumSink` / `applyKit` and drumEngine's
//! fallback kit).
//!
//! - Every track owns a core slot. Drum tracks play a [`SamplerKit`] built from their kit through
//!   a [`KitController`] (loads on worker threads; `setKit` diffs applied in place). Synth tracks
//!   play a [`SynthPlayer`] (`createSynthFromPreset(resolvePreset(presetIndex, synthOverrides))`:
//!   voices → CharacterChain, or the sampled piano on the engine's shared [`PianoSlot`], which
//!   starts decoding the first time a sampler preset is built); `setTrackInstrument` /
//!   `setTrackSynthOverrides` rebuild it (`TrackAudioRebuilt`, with `withPlaybackRestart`).
//!   Pattern notes play with `triggerAttackRelease`, the length at the tempo at event time.
//!   Audio tracks get an [`AudioRegions`] instrument when playback schedules them (players exist
//!   only while playing, as in the TS).
//! - Each slot runs the track strip (`fx/track.rs`: the track's effects, panner and volume, built
//!   like `createTrackAudioNodes`), then its mute (`track.muted` at build, solo logic on solo
//!   changes). Effect edits apply `applyTrackEffect`; every play start resets the strip
//!   (`resetTrackEffects`) and schedules the track's automation (`scheduleAutomation`) with its
//!   notes.
//! - drumEngine's fallback kit (factory kit at −6 dB, straight to the master) lives in its own
//!   slot after the [`MAX_TRACKS`] track slots, marked live-only (transport Stop does not touch
//!   it); the live synth (`AudioEngine`'s bus, `fx/live.rs`) in the next one.
//! - **Track limit.** The core preallocates [`MAX_TRACKS`] track slots (documented limit; the
//!   Electron app has none, but 256 tracks is far beyond what it can play either). A document
//!   with more tracks makes playback, recording and export refuse with an explicit error
//!   (`check_track_limit`, `EngineEvent::TrackLimitExceeded`) instead of silently dropping
//!   tracks.

use std::sync::Arc;

use mdaw_model::kit::create_factory_kit;
use mdaw_model::{InstrumentType, Track};

use super::ArrangementEngine;
use super::audio_regions::{AudioRegions, RegionPlay, db_gain};
use crate::buffers::PadBufferCache;
use crate::contract::PPQ;
use crate::core::{CoreHandle, ScheduledNote, TrackInstrument, TrackSchedule};
use crate::fx::automation::{self, AutoEvent};
use crate::fx::state::FxState;
use crate::fx::track::StripOp;
use crate::piano::PianoSlot;
use crate::player::SynthPlayer;
use crate::sampler::SamplerKit;
use crate::sampler::control::KitController;
use crate::synth::DEFAULT_NOISE_SEED;
use crate::synth::presets::{SynthPreset, resolve_preset};

/// drumEngine's `Volume(-6)` on the fallback kit.
pub const FALLBACK_GAIN_DB: f64 = -6.0;

/// Most document tracks the engine plays (live and export); see the module docs.
pub const MAX_TRACKS: usize = crate::core::MAX_SLOTS - 4;

pub(super) struct TrackAudio {
    pub slot: usize,
    pub kind: InstrumentType,
    pub kit: Option<KitController>,
    /// Audio tracks while scheduled: (region id, sample id) per region index of the instrument.
    pub regions: Vec<(String, String)>,
    /// The automation scheduled at the last play start (a running schedule rebuilt by
    /// `refreshPlaybackSchedule` keeps it: the TS only touches the Part).
    pub automation: Vec<AutoEvent>,
}

/// A track's strip settings.
pub(super) fn fx_of(track: &Track) -> FxState {
    FxState::from(&automation::track_effects(track))
}

/// drumEngine's fallback `SamplerKit(createFactoryKit())`.
pub(super) struct Fallback {
    pub slot: usize,
    pub kit: KitController,
}

/// A SamplerKit for `kit`, with the buffers already cached handed over (`new SamplerKit(kit)`).
fn build_kit(
    h: &CoreHandle,
    kit: &mdaw_model::KitState,
    sr: u32,
    cache: &PadBufferCache,
) -> (KitController, SamplerKit) {
    let (ctrl, cmds) = KitController::new(kit, cache.clone(), None);
    let mut sk = SamplerKit::new(sr, ctrl.pad_params(), h.shared().frames());
    for c in cmds {
        sk.apply(c);
    }
    (ctrl, sk)
}

impl Fallback {
    pub fn new(h: &mut CoreHandle, slot: usize, sr: u32, cache: &PadBufferCache) -> Self {
        let (kit, sk) = build_kit(h, &create_factory_kit(), sr, cache);
        h.set_instrument(slot, Some(TrackInstrument::Drum(Box::new(sk))));
        h.set_track_gain(slot, db_gain(FALLBACK_GAIN_DB));
        h.set_live_only(slot, true);
        Self { slot, kit }
    }
}

impl ArrangementEngine {
    /// `createTrackAudio(track)`.
    pub(super) fn create_track_audio(&mut self, track: &Track) {
        if self.tracks.contains_key(&track.id) {
            self.dispose_track_audio(&track.id);
        }
        let Some(slot) = self.free_slots.pop() else {
            return;
        };
        let kit = if track.instrument.kind == InstrumentType::Drums {
            let kit = track
                .instrument
                .kit
                .clone()
                .unwrap_or_else(create_factory_kit);
            let (ctrl, sk) = build_kit(&self.h, &kit, self.sr, &self.cache);
            self.h
                .set_instrument(slot, Some(TrackInstrument::Drum(Box::new(sk))));
            Some(ctrl)
        } else if track.instrument.kind == InstrumentType::Synth {
            let synth = self.build_track_synth(track);
            self.h
                .set_instrument(slot, Some(TrackInstrument::Synth(Box::new(synth))));
            None
        } else {
            // Audio players: built when scheduled.
            self.h.set_instrument(slot, None);
            None
        };
        self.h.set_schedule(slot, None);
        let strip = self
            .h
            .new_track_strip(&fx_of(track), track.pan, track.volume);
        self.h.set_track_strip(slot, Some(strip));
        self.h.set_track_gain(slot, 1.0);
        self.h.set_track_mute(slot, track.muted);
        self.h.set_live_only(slot, false);
        self.tracks.insert(
            track.id.clone(),
            TrackAudio {
                slot,
                kind: track.instrument.kind,
                kit,
                regions: Vec::new(),
                automation: Vec::new(),
            },
        );
    }

    /// The engine's shared piano samples (`loadPianoSamples()`: decoding starts on a worker
    /// thread the first time they are asked for).
    pub(super) fn piano_slot(&mut self) -> PianoSlot {
        let sr = self.sr;
        self.piano
            .get_or_insert_with(|| PianoSlot::load(sr))
            .clone()
    }

    /// A synth track's instrument at the core's current time.
    fn build_track_synth(&mut self, track: &Track) -> SynthPlayer {
        let overrides = track
            .instrument
            .synth_overrides
            .clone()
            .map(serde_json::Value::Object);
        let preset = resolve_preset(track.instrument.preset_index as f64, overrides.as_ref());
        let piano = if matches!(preset, SynthPreset::Sampler(_)) {
            self.piano_slot()
        } else {
            PianoSlot::empty()
        };
        SynthPlayer::new(&preset, self.sr, self.h.now(), &piano, DEFAULT_NOISE_SEED)
    }

    /// `disposeTrackAudio(id)`.
    pub(super) fn dispose_track_audio(&mut self, track_id: &str) {
        let Some(t) = self.tracks.remove(track_id) else {
            return;
        };
        self.h.set_instrument(t.slot, None);
        self.h.set_schedule(t.slot, None);
        self.h.set_track_strip(t.slot, None);
        self.h.set_track_gain(t.slot, 1.0);
        self.h.set_track_mute(t.slot, false);
        self.free_slots.push(t.slot);
    }

    /// Dispose every track's audio and build it again from the arrangement.
    pub(super) fn rebuild_all_audio(&mut self) {
        let ids: Vec<String> = self.tracks.keys().cloned().collect();
        for id in ids {
            self.dispose_track_audio(&id);
        }
        let tracks = self.doc.get_tracks().to_vec();
        for t in &tracks {
            self.create_track_audio(t);
        }
    }

    /// `applyKit` → `sampler.setKit(kit)`.
    pub(super) fn apply_kit(&mut self, track_id: &str) {
        let Some(kit) = self.doc.kit(track_id).cloned() else {
            return;
        };
        let Some(slot) = self.slot_of(track_id) else {
            return;
        };
        if let Some(ctrl) = self.kit_controller(track_id) {
            let cmd = ctrl.set_kit(&kit);
            self.h.kit_command(slot, cmd);
        }
    }

    /// Hand finished pad loads (and pack reloads) to the kits.
    pub(super) fn poll_kits(&mut self) {
        let mut out = Vec::new();
        for t in self.tracks.values_mut() {
            if let Some(k) = t.kit.as_mut() {
                for c in k.poll() {
                    out.push((t.slot, c));
                }
            }
        }
        for c in self.fallback.kit.poll() {
            out.push((self.fallback.slot, c));
        }
        for (slot, c) in out {
            self.h.kit_command(slot, c);
        }
    }

    /// `await kit.ready` for every kit: blocks until the loads in flight settled.
    pub fn wait_kits_ready(&mut self) {
        let mut out = Vec::new();
        for t in self.tracks.values_mut() {
            if let Some(k) = t.kit.as_mut() {
                for c in k.wait_ready() {
                    out.push((t.slot, c));
                }
            }
        }
        for c in self.fallback.kit.wait_ready() {
            out.push((self.fallback.slot, c));
        }
        for (slot, c) in out {
            self.h.kit_command(slot, c);
        }
    }

    /// `syncDrumSink()`: live pads play the active drum track's kit, else the fallback kit.
    /// A change stops the fallback kit's voices (`drumEngine.setSink` → `kit.stopAll()`); the
    /// core releases the pads still held on the old sink (R2-3).
    pub(super) fn sync_drum_sink(&mut self) {
        let target = self
            .doc
            .get_active_drum_track_id()
            .and_then(|id| self.tracks.get(&id))
            .filter(|t| t.kit.is_some())
            .map_or(self.fallback.slot, |t| t.slot);
        if self.sink_slot == Some(target) {
            return;
        }
        if self.sink_slot.is_some() {
            self.h.stop_voices(self.fallback.slot);
        }
        self.sink_slot = Some(target);
        self.h.set_live_sink(Some(target));
    }

    /// The live sink's slot (the active drum track's, or the fallback kit's).
    pub fn live_sink_slot(&self) -> Option<usize> {
        self.sink_slot
    }

    /// The fallback kit's slot.
    pub fn fallback_slot(&self) -> usize {
        self.fallback.slot
    }

    /// `updateSoloState()`: with any solo, every non-solo track is muted; else each track's own
    /// mute applies.
    pub(super) fn update_solo_state(&mut self) {
        let tracks = self.doc.get_tracks();
        let has_solo = tracks.iter().any(|t| t.solo);
        let mutes: Vec<(usize, bool)> = tracks
            .iter()
            .filter_map(|t| {
                let slot = self.tracks.get(&t.id)?.slot;
                Some((slot, if has_solo { !t.solo } else { t.muted }))
            })
            .collect();
        for (slot, muted) in mutes {
            self.h.set_track_mute(slot, muted);
        }
    }

    fn should_play(&self, track: &Track) -> bool {
        let has_solo = self.doc.get_tracks().iter().any(|t| t.solo);
        if has_solo { track.solo } else { !track.muted }
    }

    /// `scheduleAllTracks()`: every track's strip is reset to its settings
    /// (`resetTrackEffects`); then a schedule per playing MIDI / drum track, region players per
    /// playing audio track, each with the track's automation (`scheduleAutomation`); tracks that
    /// should not play (solo / mute) get nothing.
    pub(super) fn schedule_all_tracks(&mut self) {
        let tracks = self.doc.get_tracks().to_vec();
        let bpm = self.doc.get_bpm();
        for t in &tracks {
            let Some(slot) = self.slot_of(&t.id) else {
                continue;
            };
            self.reset_track_effects(t);
            let auto = if self.should_play(t) {
                for op in automation::pre_ops(t) {
                    self.h.strip_op(slot, op);
                }
                automation::events(t, bpm)
            } else {
                Vec::new()
            };
            if let Some(ta) = self.tracks.get_mut(&t.id) {
                ta.automation = auto;
            }
            self.schedule_track(t);
        }
    }

    /// `resetTrackEffects(track, audio)`.
    pub(super) fn reset_track_effects(&mut self, track: &Track) {
        if let Some(slot) = self.slot_of(&track.id) {
            let op = StripOp::Reset {
                fx: fx_of(track),
                pan: track.pan,
                volume_db: track.volume,
            };
            self.h.strip_op(slot, op);
        }
    }

    fn schedule_track(&mut self, track: &Track) {
        let Some(slot) = self.slot_of(&track.id) else {
            return;
        };
        let playing = self.should_play(track);
        let auto = self
            .tracks
            .get(&track.id)
            .map(|t| t.automation.clone())
            .unwrap_or_default();
        if track.instrument.kind == InstrumentType::Audio {
            self.schedule_audio_track(slot, track, playing, auto);
            return;
        }
        let schedule = playing
            .then(|| TrackSchedule::from_track(track, &self.doc.get_loop_settings()))
            .flatten()
            .map(|s| Arc::new(s.with_automation(auto)));
        self.h.set_schedule(slot, schedule);
    }

    fn schedule_audio_track(
        &mut self,
        slot: usize,
        track: &Track,
        playing: bool,
        auto: Vec<AutoEvent>,
    ) {
        let mut plays = Vec::new();
        let mut ids = Vec::new();
        let mut events = Vec::new();
        if playing {
            let spb = 60.0 / self.doc.get_bpm();
            for region in &track.regions {
                let Some(audio) = &region.audio else { continue };
                let Some(buffer) = self.samples.buffer(&audio.sample_id) else {
                    continue;
                };
                events.push(ScheduledNote {
                    tick: region.start_beat * PPQ as f64,
                    beat: region.start_beat,
                    note: 0.0,
                    velocity: 1.0,
                    duration_beats: region.length_beats,
                    pad: Some(plays.len()),
                });
                plays.push(RegionPlay {
                    buffer: Some(buffer),
                    offset: audio.offset_seconds,
                    duration: region.length_beats * spb,
                    gain: if audio.gain_db != 0.0 {
                        db_gain(audio.gain_db)
                    } else {
                        1.0
                    },
                });
                ids.push((region.id.clone(), audio.sample_id.clone()));
            }
        }
        let instrument = (!plays.is_empty())
            .then(|| TrackInstrument::Audio(Box::new(AudioRegions::new(self.sr, plays))));
        let schedule = (instrument.is_some() || !auto.is_empty())
            .then(|| Arc::new(TrackSchedule::new(false, events).with_automation(auto)));
        self.h.set_instrument(slot, instrument);
        self.h.set_schedule(slot, schedule);
        if let Some(t) = self.tracks.get_mut(&track.id) {
            t.regions = ids;
        }
    }

    /// Stop: every schedule goes, audio players are disposed.
    pub(super) fn clear_schedules(&mut self) {
        let slots: Vec<(usize, bool)> = self
            .tracks
            .values()
            .map(|t| (t.slot, t.kind == InstrumentType::Audio))
            .collect();
        for (slot, audio) in slots {
            self.h.set_schedule(slot, None);
            if audio {
                self.h.set_instrument(slot, None);
            }
        }
        for t in self.tracks.values_mut() {
            t.regions.clear();
            t.automation.clear();
        }
    }

    /// `refreshPlaybackSchedule(trackId)`: rebuild one track's schedule while playing or loop
    /// recording (the TS adds a plain new loop hit to the running Part instead; swapping an
    /// immutable schedule is the same thing here, R2-13).
    pub(super) fn refresh_playback_schedule(&mut self, track_id: &str) {
        if self.state != super::TransportState::Playing && !self.doc.is_loop_recording() {
            return;
        }
        let Some(track) = self
            .doc
            .get_tracks()
            .iter()
            .find(|t| t.id == track_id)
            .cloned()
        else {
            return;
        };
        if track.instrument.kind == InstrumentType::Audio {
            return;
        }
        self.schedule_track(&track);
    }

    /// B3 (`refreshAudioPlayers`): samples reloaded in place reach the scheduled region players
    /// (their next start plays the new audio). Stopped: nothing to do, the next play reads them.
    pub fn refresh_audio_players(&mut self, sample_ids: &[String]) {
        let mut out = Vec::new();
        for t in self.tracks.values() {
            for (i, (_, sid)) in t.regions.iter().enumerate() {
                if sample_ids.iter().any(|s| s == sid)
                    && let Some(b) = self.samples.buffer(sid)
                {
                    out.push((t.slot, i, b));
                }
            }
        }
        for (slot, i, b) in out {
            self.h.set_region_buffer(slot, i, Some(b));
        }
    }
}
