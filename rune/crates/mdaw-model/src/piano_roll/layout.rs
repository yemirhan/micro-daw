//! Row layouts (pitch <-> row). The TS `RowLayout` is an object of closures; here it is a struct
//! with the same operations as methods.

use super::math::{clamp, js_num, js_round};
use super::types::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutKind {
    Piano,
    Drum,
}

impl LayoutKind {
    pub fn as_str(self) -> &'static str {
        match self {
            LayoutKind::Piano => "piano",
            LayoutKind::Drum => "drum",
        }
    }
}

/// Maps MIDI pitches to vertical rows (row 0 = top).
#[derive(Debug, Clone, PartialEq)]
pub struct RowLayout {
    pub kind: LayoutKind,
    pub row_count: usize,
    /// Piano: lowest/highest pitch.
    min: f64,
    max: f64,
    /// Drum: pitch of each row.
    pitches: Vec<f64>,
}

/// `createPianoLayout()` with the default full MIDI range.
pub fn create_piano_layout_default() -> RowLayout {
    create_piano_layout(PR_NOTE_MIN, PR_NOTE_MAX)
}

/// `createPianoLayout(min, max)`.
pub fn create_piano_layout(min: i32, max: i32) -> RowLayout {
    RowLayout {
        kind: LayoutKind::Piano,
        row_count: (max - min + 1).max(0) as usize,
        min: f64::from(min),
        max: f64::from(max),
        pitches: Vec::new(),
    }
}

/// Drum layout: one row per GM pad, plus extra rows for any other pitches that appear in the
/// region (so no note is ever hidden). Extra rows go below the pads, sorted ascending.
pub fn create_drum_layout(extra_pitches: &[f64]) -> RowLayout {
    let mut pitches: Vec<f64> = DRUM_ROWS.iter().map(|d| d.midi_note).collect();
    let mut extras: Vec<f64> = Vec::new();
    for &p in extra_pitches {
        if !extras.contains(&p) && !pitches.contains(&p) {
            extras.push(p);
        }
    }
    extras.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    pitches.extend(extras);
    RowLayout { kind: LayoutKind::Drum, row_count: pitches.len(), min: 0.0, max: 0.0, pitches }
}

/// `midiToNoteName` (src/utils/noteHelpers.ts), C4 = 60. A fractional pitch indexes past the
/// name table in JS and prints `undefined`, kept as-is.
pub fn midi_to_note_name(midi: f64) -> String {
    const NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    let octave = (midi / 12.0).floor() - 1.0;
    let idx = ((midi % 12.0) + 12.0) % 12.0;
    let name = if idx.fract() == 0.0 && (0.0..12.0).contains(&idx) { NOTE_NAMES[idx as usize] } else { "undefined" };
    format!("{name}{}", js_num(octave))
}

/// `isBlackKey` (src/utils/noteHelpers.ts).
pub fn is_black_key(midi: f64) -> bool {
    let n = midi % 12.0;
    n == 1.0 || n == 3.0 || n == 6.0 || n == 8.0 || n == 10.0
}

impl RowLayout {
    /// Drum: the pitch of each row (pads then extras). Piano: empty.
    pub fn pitches(&self) -> &[f64] {
        &self.pitches
    }

    /// Row index for a pitch, or -1 if the pitch has no row.
    pub fn pitch_to_row(&self, pitch: f64) -> f64 {
        match self.kind {
            LayoutKind::Piano => {
                if pitch < self.min || pitch > self.max {
                    -1.0
                } else {
                    self.max - pitch
                }
            }
            LayoutKind::Drum => self.pitches.iter().position(|&p| p == pitch).map_or(-1.0, |i| i as f64),
        }
    }

    /// Pitch for a row index (rounded and clamped into range).
    pub fn row_to_pitch(&self, row: f64) -> f64 {
        let last = self.row_count as f64 - 1.0;
        let r = clamp(js_round(row), 0.0, last);
        match self.kind {
            LayoutKind::Piano => self.max - r,
            LayoutKind::Drum => self.pitches.get(r as usize).copied().unwrap_or(f64::NAN),
        }
    }

    /// Label for a row (note name / drum pad name).
    pub fn row_label(&self, row: i64) -> String {
        match self.kind {
            LayoutKind::Piano => midi_to_note_name(self.max - row as f64),
            LayoutKind::Drum => {
                let at = usize::try_from(row).ok();
                if let Some(d) = at.and_then(|i| DRUM_ROWS.get(i)) {
                    return d.full_name.into();
                }
                midi_to_note_name(at.and_then(|i| self.pitches.get(i)).copied().unwrap_or(36.0))
            }
        }
    }

    /// Optional per-row accent color (drum pads). Piano layouts have none.
    pub fn row_color(&self, row: i64) -> Option<&'static str> {
        match self.kind {
            LayoutKind::Piano => None,
            LayoutKind::Drum => usize::try_from(row).ok().and_then(|i| DRUM_ROWS.get(i)).map(|d| d.color),
        }
    }
}

/// Whether a row should get "black key" shading.
pub fn is_shaded_row(layout: &RowLayout, row: f64) -> bool {
    if layout.kind == LayoutKind::Drum {
        return row % 2.0 == 1.0;
    }
    is_black_key(layout.row_to_pitch(row))
}
