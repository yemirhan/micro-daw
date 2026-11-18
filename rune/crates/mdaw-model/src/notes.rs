//! MIDI note helpers and buffer peaks. Port of `src/utils/noteHelpers.ts` and
//! `src/utils/waveformHelpers.ts`.

use crate::js::number_to_string as js_number_to_string;

/// `NOTE_NAMES` (sharps).
pub const NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/// `NOTE_NAMES[i]` with JS array semantics: out of range or fractional → "undefined".
pub(crate) fn note_name_at(i: f64) -> &'static str {
    if i.fract() == 0.0 && (0.0..12.0).contains(&i) { NOTE_NAMES[i as usize] } else { "undefined" }
}

/// `midiToNoteName(midi)`: C4 = 60 (the app-wide convention; pad note 36 = C2). Takes a JS number:
/// a fractional note prints "undefined<octave>" exactly like TS.
pub fn midi_to_note_name(midi: f64) -> String {
    let octave = (midi / 12.0).floor() - 1.0;
    let note = note_name_at(((midi % 12.0) + 12.0) % 12.0);
    format!("{note}{}", js_number_to_string(octave))
}

/// `noteNameToMidi(name)`: `/^([A-G]#?)(-?\d)$/` (one octave digit), else -1.
pub fn note_name_to_midi(name: &str) -> i32 {
    let b = name.as_bytes();
    let mut i = 0;
    if !(b.first().is_some_and(|c| (b'A'..=b'G').contains(c))) {
        return -1;
    }
    i += 1;
    if b.get(i) == Some(&b'#') {
        i += 1;
    }
    let note = &name[..i];
    let neg = b.get(i) == Some(&b'-');
    if neg {
        i += 1;
    }
    let Some(d) = b.get(i).filter(|c| c.is_ascii_digit()) else { return -1 };
    if i + 1 != b.len() {
        return -1;
    }
    let Some(note_index) = NOTE_NAMES.iter().position(|n| *n == note) else { return -1 };
    let octave = (d - b'0') as i32 * if neg { -1 } else { 1 };
    (octave + 1) * 12 + note_index as i32
}

/// `isBlackKey(midi)`: JS `%` keeps the sign, so negative notes are never black keys.
pub fn is_black_key(midi: i32) -> bool {
    matches!(midi % 12, 1 | 3 | 6 | 8 | 10)
}

/// `midiToFrequency(midi)`: equal temperament, A4 = 440 Hz.
pub fn midi_to_frequency(midi: f64) -> f64 {
    440.0 * 2f64.powf((midi - 69.0) / 12.0)
}

/// `velocityToNormalized(velocity)`: 0..127 → 0..1, clamped.
pub fn velocity_to_normalized(velocity: f64) -> f64 {
    let v = velocity / 127.0;
    if v.is_nan() { f64::NAN } else { v.clamp(0.0, 1.0) }
}

/// `computePeaksFromBuffer(audioBuffer, numPeaks)` (waveformHelpers.ts): absolute max per bucket
/// across all channels (`f32` like the TS `Float32Array`). `channels` are the channel data,
/// `length` the buffer length in frames.
pub fn compute_peaks_from_buffer(channels: &[&[f32]], length: usize, num_peaks: usize) -> Vec<f32> {
    let mut peaks = vec![0f32; num_peaks];
    let samples_per_peak = length as f64 / num_peaks as f64;
    for data in channels {
        for (i, peak) in peaks.iter_mut().enumerate() {
            let start = (i as f64 * samples_per_peak).floor() as usize;
            let end = ((((i + 1) as f64) * samples_per_peak).floor() as usize).min(length);
            let mut max = 0f64;
            for j in start..end {
                // Reading past the channel data gives `undefined` → NaN, which never wins.
                let abs = data.get(j).map_or(f64::NAN, |v| (*v as f64).abs());
                if abs > max {
                    max = abs;
                }
            }
            if max > *peak as f64 {
                *peak = max as f32;
            }
        }
    }
    peaks
}

#[cfg(test)]
#[path = "notes_tests.rs"]
mod tests;
