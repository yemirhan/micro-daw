//! `getArrangementEndSeconds`, `effectsTailSeconds`, `drumPlayBeat` (ExportService.ts).

use mdaw_model::arrangement_helpers::{loop_swing_grid, swing_offset_beats};
use mdaw_model::js;
use mdaw_model::types::{
    Arrangement, InstrumentType, PadPlayMode, PadSettings, Track, note_to_pad,
};

use crate::sampler::{PadParams, compute_slice, voice_seconds};

/// Seconds rendered after the last audible sound so synth releases and reverb tails ring out.
pub const EXPORT_TAIL_SECONDS: f64 = 3.0;
/// Longest feedback-delay tail rendered (s).
pub const MAX_DELAY_TAIL_SECONDS: f64 = 12.0;

/// `drumPlayBeat`: the beat a drum-track note sounds at (patterns are stored straight, swing is
/// applied at playback; live, WAV and MIDI export all use this).
pub fn drum_play_beat(arrangement: &Arrangement, beat: f64) -> f64 {
    let lp = arrangement
        .beat
        .as_ref()
        .map(|b| b.r#loop)
        .unwrap_or_default();
    beat + swing_offset_beats(beat, loop_swing_grid(Some(lp.swing_grid)), lp.swing)
}

/// `drumNoteAudibleSeconds`: one-shots play their whole slice; gate pads stop at note end + decay
/// (never past the slice); unknown samples fall back to the note length.
fn drum_note_audible_seconds(
    pad: Option<&PadSettings>,
    note_seconds: f64,
    pad_duration: &mut impl FnMut(&PadSettings) -> Option<f64>,
) -> f64 {
    let Some(pad) = pad.filter(|p| p.sample.is_some()) else {
        return note_seconds;
    };
    let Some(buffer_duration) = pad_duration(pad).filter(|d| *d > 0.0) else {
        return note_seconds;
    };
    let Some(slice) = compute_slice(&PadParams::from(pad), buffer_duration) else {
        return 0.0;
    };
    // Shaping can shorten or lengthen the hit (pitch envelope, silent sustain); a neutral pad
    // gets `slice.real_duration` back exactly.
    let sounding = voice_seconds(&pad.shape, &slice);
    if pad.mode == PadPlayMode::Gate {
        return js::min(sounding, note_seconds + js::max(0.0, pad.decay));
    }
    sounding
}

/// `effectsTailSeconds`: extra render time for a track's feedback delay (repeats until −60 dB, at
/// least one echo, capped at [`MAX_DELAY_TAIL_SECONDS`]). Reverb is covered by the base tail.
pub fn effects_tail_seconds(track: &Track) -> f64 {
    let Some(delay) = track.effects.as_ref().map(|e| &e.delay) else {
        return 0.0;
    };
    #[allow(clippy::neg_cmp_op_on_partial_ord)] // NaN → no tail, as in the TS
    if !delay.enabled || !(delay.wet > 0.0) || !(delay.time > 0.0) {
        return 0.0;
    }
    let fb = js::min(0.99, js::max(0.0, delay.feedback));
    let repeats = if fb > 0.0 {
        js::max(1.0, js::log(1e-3) / js::log(fb))
    } else {
        1.0
    };
    js::min(MAX_DELAY_TAIL_SECONDS, delay.time * repeats)
}

/// `getArrangementEndSeconds(arrangement, padDuration)`: seconds from the start until the last
/// unmuted sound ends — audio region ends, synth note ends, drum hits at their swung start plus
/// their audible length — plus each track's delay tail. `pad_duration` returns the decoded buffer
/// length (s) of a pad's sample, or None when unknown / not loaded. The WAV render adds
/// [`EXPORT_TAIL_SECONDS`] on top (see [`export_duration_seconds`]).
pub fn arrangement_end_seconds(
    arrangement: &Arrangement,
    mut pad_duration: impl FnMut(&PadSettings) -> Option<f64>,
) -> f64 {
    let sec_per_beat = 60.0 / arrangement.bpm;
    let mut end = 0.0;
    for track in &arrangement.tracks {
        if track.muted {
            continue;
        }
        let mut track_end = 0.0;
        let is_drums = track.instrument.kind == InstrumentType::Drums;
        for region in &track.regions {
            if region.audio.is_some() {
                track_end = js::max(
                    track_end,
                    (region.start_beat + region.length_beats) * sec_per_beat,
                );
                continue;
            }
            for note in &region.notes {
                let beat = region.start_beat + note.start_beat;
                let note_seconds = js::max(0.0, note.duration_beats) * sec_per_beat;
                if is_drums {
                    let settings = note_to_pad(js::round(note.note))
                        .and_then(|p| track.instrument.kit.as_ref()?.pads.get(p));
                    let start = drum_play_beat(arrangement, beat) * sec_per_beat;
                    track_end = js::max(
                        track_end,
                        start
                            + drum_note_audible_seconds(settings, note_seconds, &mut pad_duration),
                    );
                } else {
                    track_end = js::max(track_end, beat * sec_per_beat + note_seconds);
                }
            }
        }
        if track_end > 0.0 {
            end = js::max(end, track_end + effects_tail_seconds(track));
        }
    }
    end
}

/// Length of the offline WAV render: [`arrangement_end_seconds`] + [`EXPORT_TAIL_SECONDS`].
pub fn export_duration_seconds(
    arrangement: &Arrangement,
    pad_duration: impl FnMut(&PadSettings) -> Option<f64>,
) -> f64 {
    arrangement_end_seconds(arrangement, pad_duration) + EXPORT_TAIL_SECONDS
}
