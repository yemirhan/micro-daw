//! WAV export (`ExportService.exportWav`, `ExportService.ts:100-188`).
//!
//! - "Nothing to export" when the end time (without buffer knowledge) is ≤ 0.
//! - The piano samples are decoded first when an unmuted track uses a sampler preset (a failed
//!   decode leaves the FM fallback, like the TS `.catch`); every unmuted drum track's kit is
//!   preloaded; then the length is the end of the last sound
//!   (`getArrangementEndSeconds`: audio region ends, synth note ends, drum notes at their swung
//!   start + audible length — one-shot: the whole slice; gate: `min(slice, note + decay)`;
//!   unknown buffer: the note — plus each track's feedback-delay tail) + 3 s. The render has
//!   `floor(seconds · rate)` frames (the `OfflineAudioContext` length conversion), 2 channels.
//! - A fresh offline [`Core`] at the given rate, BPM fixed, transport started at beat 0 at time
//!   0, no loop (the Song loop is ignored), **solo ignored, only `muted` skips a track**, no
//!   automation. Muted tracks take no slot; more than [`super::MAX_TRACKS`] unmuted tracks is an
//!   explicit error ([`ExportError::TooManyTracks`]). Tracks go through the same track strip
//!   as live playback (`fx/track.rs`: the track's effects as `createTrackAudioNodes` builds them,
//!   panner, volume); drums through `SamplerKit` with swing and gate releases (the core's pattern
//!   path = `playDrumNote`); synth tracks through their [`SynthPlayer`] (the same instrument and
//!   chain as live playback, built at time 0, `triggerAttackRelease` with the length at the fixed
//!   BPM; noise offsets from [`DEFAULT_NOISE_SEED`] per instrument) in `Tone.Offline`'s clock
//!   model ([`SynthPlayer::new_offline`]: Tone clamps the voices' source starts to the next
//!   128-frame block time, so an export note's oscillators start up to one block after the
//!   note, unlike live playback); audio regions through their players.
//! - The master bus at a 0 dB fader (+2 dB drive, soft clip) shapes the sum.
//! - Encoding: `mdaw_dsp::audio_io::encode_wav` (16-bit PCM, `audioBufferToWav`).
//!
//! `end_seconds` delegates to the shared golden-tested `crate::export::arrangement_end_seconds`.

use std::sync::Arc;

use mdaw_dsp::audio_io::encode_wav;
use mdaw_dsp::buffer::AudioBuffer;
use mdaw_model::arrangement_helpers::DEFAULT_LOOP_SETTINGS;
use mdaw_model::kit::create_factory_kit;
use mdaw_model::{Arrangement, InstrumentType, PadSettings};

use super::AudioSamples;
use super::audio_regions::{AudioRegions, RegionPlay, db_gain};
use crate::buffers::PadBufferCache;
use crate::contract::PPQ;
use crate::core::{Core, CoreConfig, ScheduledNote, TrackInstrument, TrackSchedule};
use crate::fx::state::FxState;
use crate::piano::PianoSlot;
use crate::piano::samples::piano_samples;
use crate::player::SynthPlayer;
use crate::sampler::SamplerKit;
use crate::sampler::control::KitController;
use crate::synth::DEFAULT_NOISE_SEED;
use crate::synth::presets::{SynthPreset, preset_at, resolve_preset};

/// Seconds rendered after the last audible sound.
pub const EXPORT_TAIL_SECONDS: f64 = 3.0;
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportError {
    /// "Nothing to export".
    NothingToExport,
    /// More unmuted tracks than the engine renders ([`super::MAX_TRACKS`]).
    TooManyTracks { tracks: usize, max: usize },
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NothingToExport => f.write_str("Nothing to export"),
            Self::TooManyTracks { tracks, max } => write!(
                f,
                "{tracks} unmuted tracks: at most {max} tracks can be exported"
            ),
        }
    }
}

impl std::error::Error for ExportError {}

/// `getArrangementEndSeconds(arrangement, padDuration)`: the shared, golden-tested port in
/// `crate::export` (byte-for-byte and value-for-value against the TS, see export/tests.rs).
fn end_seconds(arr: &Arrangement, pad_duration: &dyn Fn(&PadSettings) -> Option<f64>) -> f64 {
    crate::export::arrangement_end_seconds(arr, pad_duration)
}

/// The export render (before encoding): 2 channels at `sample_rate`. `cache` should run at that
/// rate (the app's context rate); `samples` gives the audio regions' buffers.
pub fn render_export(
    arr: &Arrangement,
    sample_rate: u32,
    cache: &PadBufferCache,
    samples: &dyn AudioSamples,
) -> Result<AudioBuffer, ExportError> {
    if end_seconds(arr, &|_| None) <= 0.0 {
        return Err(ExportError::NothingToExport);
    }
    let kits: Vec<_> = arr
        .tracks
        .iter()
        .filter(|t| !t.muted && t.instrument.kind == InstrumentType::Drums)
        .map(|t| t.instrument.kit.clone().unwrap_or_else(create_factory_kit))
        .collect();
    for k in &kits {
        // Missing files are logged by the TS and stay silent.
        let _ = cache.preload_kit_buffers(k);
    }
    // `loadPianoSamples()` if any unmuted sampler preset is used.
    let needs_piano = arr.tracks.iter().any(|t| {
        !t.muted
            && t.instrument.kind == InstrumentType::Synth
            && matches!(
                preset_at(t.instrument.preset_index as f64),
                SynthPreset::Sampler(_)
            )
    });
    let piano = match needs_piano.then(|| piano_samples(sample_rate)) {
        Some(Ok(s)) => PianoSlot::ready(s),
        Some(Err(e)) => {
            eprintln!("[export] piano samples failed to load, using synthesized piano: {e}");
            PianoSlot::empty()
        }
        None => PianoSlot::empty(),
    };
    let duration = end_seconds(arr, &|p| {
        p.sample
            .as_ref()
            .and_then(|s| cache.get_cached(&s.source))
            .map(|b| b.duration())
    }) + EXPORT_TAIL_SECONDS;
    let frames = (duration * sample_rate as f64).floor().max(0.0) as usize;

    // Muted tracks are skipped BEFORE slots are assigned: only the audible ones count.
    let audible: Vec<_> = arr.tracks.iter().filter(|t| !t.muted).collect();
    if audible.len() > super::MAX_TRACKS {
        return Err(ExportError::TooManyTracks {
            tracks: audible.len(),
            max: super::MAX_TRACKS,
        });
    }
    let mut config = CoreConfig::new(sample_rate);
    config.max_tracks = audible.len().max(1);
    // Every setup command fits in the ring at once (the core is not running yet).
    config.command_capacity = config.command_capacity.max(8 * audible.len() + 16);
    let (mut core, mut h, _live) = Core::new(config);
    h.set_bpm(arr.bpm);
    h.set_master_bus(Some(0.0));
    let lp = arr
        .beat
        .as_ref()
        .map_or(DEFAULT_LOOP_SETTINGS, |b| b.r#loop);
    let spb = 60.0 / arr.bpm;
    for (slot, track) in audible.into_iter().enumerate() {
        let fx = FxState::from(&track.effects.clone().unwrap_or_default());
        let strip = h.new_track_strip(&fx, track.pan, track.volume);
        h.set_track_strip(slot, Some(strip));
        match track.instrument.kind {
            InstrumentType::Drums => {
                let kit = track
                    .instrument
                    .kit
                    .clone()
                    .unwrap_or_else(create_factory_kit);
                let (mut ctrl, cmds) = KitController::new(&kit, cache.clone(), None);
                let mut sk = SamplerKit::new(sample_rate, ctrl.pad_params(), 0);
                for c in cmds.into_iter().chain(ctrl.wait_ready()) {
                    sk.apply(c);
                }
                h.set_instrument(slot, Some(TrackInstrument::Drum(Box::new(sk))));
                h.set_schedule(slot, TrackSchedule::from_track(track, &lp).map(Arc::new));
            }
            InstrumentType::Synth => {
                let overrides = track
                    .instrument
                    .synth_overrides
                    .clone()
                    .map(serde_json::Value::Object);
                let preset =
                    resolve_preset(track.instrument.preset_index as f64, overrides.as_ref());
                let mut synth =
                    SynthPlayer::new_offline(&preset, sample_rate, &piano, DEFAULT_NOISE_SEED);
                let schedule = TrackSchedule::from_track(track, &lp);
                if let Some(s) = &schedule {
                    let notes: Vec<_> = s
                        .events()
                        .iter()
                        .map(|e| {
                            let time = e.tick * spb / PPQ as f64;
                            (
                                e.note.round() as i32,
                                e.duration_beats * spb,
                                time,
                                e.velocity,
                            )
                        })
                        .collect();
                    let dispatch: Vec<_> = s
                        .events()
                        .iter()
                        .filter_map(|e| {
                            let fired = tone_fired_tick(e.tick, arr.bpm);
                            (fired < e.tick)
                                .then(|| (e.tick * spb / PPQ as f64, fired * spb / PPQ as f64))
                        })
                        .collect();
                    synth.prepare_offline(&notes, &dispatch);
                }
                h.set_instrument(slot, Some(TrackInstrument::Synth(Box::new(synth))));
                h.set_schedule(slot, schedule.map(Arc::new));
            }
            InstrumentType::Audio => {
                let mut plays = Vec::new();
                let mut events = Vec::new();
                for region in &track.regions {
                    let Some(audio) = &region.audio else { continue };
                    let Some(buffer) = samples.buffer(&audio.sample_id) else {
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
                }
                h.set_instrument(
                    slot,
                    Some(TrackInstrument::Audio(Box::new(AudioRegions::new(
                        sample_rate,
                        plays,
                    )))),
                );
                h.set_schedule(slot, Some(Arc::new(TrackSchedule::new(false, events))));
            }
        }
    }
    h.start(Some(0.0), 0.0);
    let mut l = vec![0f32; frames];
    let mut r = vec![0f32; frames];
    core.process(&mut l, &mut r);
    drop(core);
    h.collect_garbage();
    Ok(AudioBuffer {
        sample_rate,
        channels: vec![l, r],
    })
}

/// The transport tick that fires a Part event at integer tick `tick` in Tone: `ToneEvent`
/// schedules `Ticks(tick)`, which `Transport.schedule` converts to seconds and back
/// (`TransportTime.toTicks()`, f64) and stores as `floor` + remainder. A round trip landing a
/// hair below the tick (tick 1440 at 112 BPM → 1439.9999999999998) fires it a tick early.
fn tone_fired_tick(tick: f64, bpm: f64) -> f64 {
    let ppq = PPQ as f64;
    let seconds = (tick / ppq) * (60.0 / bpm);
    (seconds / (60.0 / bpm) * ppq).floor()
}

/// `exportWav(arrangement)`: the WAV file bytes.
pub fn export_wav(
    arr: &Arrangement,
    sample_rate: u32,
    cache: &PadBufferCache,
    samples: &dyn AudioSamples,
) -> Result<Vec<u8>, ExportError> {
    render_export(arr, sample_rate, cache, samples).map(|b| encode_wav(&b))
}

#[cfg(test)]
pub(super) fn end_seconds_for_tests(
    arr: &Arrangement,
    pad_duration: &dyn Fn(&PadSettings) -> Option<f64>,
) -> f64 {
    end_seconds(arr, pad_duration)
}

#[cfg(test)]
pub(super) fn effects_tail_for_tests(track: &mdaw_model::Track) -> f64 {
    crate::export::effects_tail_seconds(track)
}
