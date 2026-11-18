//! WAV export (`ExportService.exportWav`, `ExportService.ts:100-188`).
//!
//! - "Nothing to export" when the end time (without buffer knowledge) is ≤ 0.
//! - Every unmuted drum track's kit is preloaded, then the length is the end of the last sound
//!   (`getArrangementEndSeconds`: audio region ends, synth note ends, drum notes at their swung
//!   start + audible length — one-shot: the whole slice; gate: `min(slice, note + decay)`;
//!   unknown buffer: the note — plus each track's feedback-delay tail) + 3 s. The render has
//!   `floor(seconds · rate)` frames (the `OfflineAudioContext` length conversion), 2 channels.
//! - A fresh offline [`Core`] at the given rate, BPM fixed, transport started at beat 0 at time 0, no loop (the Song loop is ignored),
//!   **solo ignored, only `muted` skips a track**, no automation. Tracks go through the same strip
//!   as live playback (neutral FX chain + panner + volume); drums through `SamplerKit` with swing
//!   and gate releases (the core's pattern path = `playDrumNote`); audio regions through their
//!   players.
//! - The disabled effects' RoomReverb leak (−56 dB, `core/strip.rs`) is added from the core's
//!   reverb sends by an FFT convolution, then the master bus at a 0 dB fader (+2 dB drive, soft
//!   clip) shapes the sum.
//! - Synth tracks are silent until phase 4 (instruments), but still count for the length.
//! - Encoding: `mdaw_dsp::audio_io::encode_wav` (16-bit PCM, `audioBufferToWav`).
//!
//! `end_seconds` is a local copy of `getArrangementEndSeconds`; `crate::export` gets the shared
//! port (`arrangement_end_seconds`), which this helper can be swapped for.

use std::sync::Arc;

use mdaw_dsp::audio_io::encode_wav;
use mdaw_dsp::buffer::AudioBuffer;
use mdaw_model::arrangement_helpers::{DEFAULT_LOOP_SETTINGS, swing_offset_beats};
use mdaw_model::kit::create_factory_kit;
use mdaw_model::{Arrangement, InstrumentType, PadPlayMode, PadSettings, Track, note_to_pad};

use mdaw_dsp::shaping::{master_bus_sample, master_clip_curve_shared};

use super::AudioSamples;
use super::audio_regions::{AudioRegions, RegionPlay, db_gain};
use super::reverb_leak::reverb_leak;
use crate::buffers::PadBufferCache;
use crate::contract::PPQ;
use crate::core::{Core, CoreConfig, ScheduledNote, TrackInstrument, TrackSchedule};
use crate::sampler::control::KitController;
use crate::sampler::{PadParams, SamplerKit, compute_slice};

/// Seconds rendered after the last audible sound.
pub const EXPORT_TAIL_SECONDS: f64 = 3.0;
/// Longest feedback-delay tail rendered (s).
const MAX_DELAY_TAIL_SECONDS: f64 = 12.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportError {
    /// "Nothing to export".
    NothingToExport,
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Nothing to export")
    }
}

impl std::error::Error for ExportError {}

/// `drumPlayBeat`: the swung beat a drum note sounds at.
fn drum_play_beat(arr: &Arrangement, beat: f64) -> f64 {
    let lp = arr
        .beat
        .as_ref()
        .map_or(DEFAULT_LOOP_SETTINGS, |b| b.r#loop);
    beat + swing_offset_beats(beat, lp.swing_grid, lp.swing)
}

fn drum_note_audible_seconds(
    pad: Option<&PadSettings>,
    note_seconds: f64,
    pad_duration: &dyn Fn(&PadSettings) -> Option<f64>,
) -> f64 {
    let Some(pad) = pad.filter(|p| p.sample.is_some()) else {
        return note_seconds;
    };
    let Some(dur) = pad_duration(pad).filter(|d| *d > 0.0) else {
        return note_seconds;
    };
    let Some(slice) = compute_slice(&PadParams::from(pad), dur) else {
        return 0.0;
    };
    if pad.mode == PadPlayMode::Gate {
        slice.real_duration.min(note_seconds + pad.decay.max(0.0))
    } else {
        slice.real_duration
    }
}

/// `effectsTailSeconds(track)`.
fn effects_tail_seconds(track: &Track) -> f64 {
    let Some(d) = track.effects.as_ref().map(|e| &e.delay) else {
        return 0.0;
    };
    if !d.enabled || d.wet.is_nan() || d.wet <= 0.0 || d.time.is_nan() || d.time <= 0.0 {
        return 0.0;
    }
    let fb = d.feedback.clamp(0.0, 0.99);
    let repeats = if fb > 0.0 {
        (1e-3f64.ln() / fb.ln()).max(1.0)
    } else {
        1.0
    };
    (d.time * repeats).min(MAX_DELAY_TAIL_SECONDS)
}

/// `getArrangementEndSeconds(arrangement, padDuration)` (local helper, see the module docs).
fn end_seconds(arr: &Arrangement, pad_duration: &dyn Fn(&PadSettings) -> Option<f64>) -> f64 {
    let spb = 60.0 / arr.bpm;
    let mut end: f64 = 0.0;
    for track in arr.tracks.iter().filter(|t| !t.muted) {
        let mut track_end: f64 = 0.0;
        let drums = track.instrument.kind == InstrumentType::Drums;
        for region in &track.regions {
            if region.audio.is_some() {
                track_end = track_end.max((region.start_beat + region.length_beats) * spb);
                continue;
            }
            for n in &region.notes {
                let beat = region.start_beat + n.start_beat;
                let note_seconds = n.duration_beats.max(0.0) * spb;
                if drums {
                    let pad = note_to_pad(n.note.round())
                        .and_then(|p| track.instrument.kit.as_ref()?.pads.get(p));
                    let start = drum_play_beat(arr, beat) * spb;
                    track_end = track_end
                        .max(start + drum_note_audible_seconds(pad, note_seconds, pad_duration));
                } else {
                    track_end = track_end.max(beat * spb + note_seconds);
                }
            }
        }
        if track_end > 0.0 {
            end = end.max(track_end + effects_tail_seconds(track));
        }
    }
    end
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
    let duration = end_seconds(arr, &|p| {
        p.sample
            .as_ref()
            .and_then(|s| cache.get_cached(&s.source))
            .map(|b| b.duration())
    }) + EXPORT_TAIL_SECONDS;
    let frames = (duration * sample_rate as f64).floor().max(0.0) as usize;

    let mut config = CoreConfig::new(sample_rate);
    config.max_tracks = arr.tracks.len().clamp(1, 64);
    // The master bus is applied after the reverb leak is added (below).
    let (mut core, mut h, _live) = Core::new(config);
    h.set_bpm(arr.bpm);
    let lp = arr
        .beat
        .as_ref()
        .map_or(DEFAULT_LOOP_SETTINGS, |b| b.r#loop);
    let spb = 60.0 / arr.bpm;
    for (slot, track) in arr.tracks.iter().enumerate().take(64) {
        if track.muted {
            continue;
        }
        h.set_track_strip(slot, true);
        h.set_track_gain(slot, db_gain(track.volume));
        h.set_track_pan(slot, track.pan as f32);
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
            // Phase 4: synth instruments (silent here).
            InstrumentType::Synth => {}
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
    let (mut buf, sends) = core.render_offline_with_reverb_send(frames);
    drop(core);
    h.collect_garbage();
    let leak = reverb_leak(&sends, frames, sample_rate);
    let curve = master_clip_curve_shared();
    for (ch, leak) in buf.channels.iter_mut().zip(leak.iter()) {
        for (x, l) in ch.iter_mut().zip(leak.iter()) {
            *x = master_bus_sample(curve, (*x as f64 + l) as f32);
        }
    }
    Ok(buf)
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
pub(super) fn effects_tail_for_tests(track: &Track) -> f64 {
    effects_tail_seconds(track)
}
