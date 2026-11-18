//! Chord detection. Port of `src/utils/chordDetection.ts` (`ChordInfo` from `src/types/music.ts`).

use crate::notes::note_name_at;

/// `ChordInfo`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ChordInfo {
    /// e.g. "C", "F#"
    pub root: String,
    /// e.g. "maj", "m", "dim", "aug", "7", "m7"
    pub quality: String,
    /// e.g. "C", "Am7" ("maj" is dropped from the display)
    pub display: String,
}

/// Interval patterns (semitones from root) for chord qualities, in match order.
pub const CHORD_PATTERNS: [(&[i32], &str); 12] = [
    // Triads
    (&[0, 4, 7], "maj"),
    (&[0, 3, 7], "m"),
    (&[0, 3, 6], "dim"),
    (&[0, 4, 8], "aug"),
    (&[0, 2, 7], "sus2"),
    (&[0, 5, 7], "sus4"),
    (&[0, 7], "5"),
    // Seventh chords
    (&[0, 4, 7, 10], "7"),
    (&[0, 3, 7, 10], "m7"),
    (&[0, 4, 7, 11], "maj7"),
    (&[0, 3, 6, 9], "dim7"),
    (&[0, 3, 6, 10], "m7b5"),
];

/// `detectChord(midiNotes)`: the first root (lowest pitch class first) whose interval set matches a
/// pattern, or None. Pitch classes use JS `%` (negative notes give negative classes, whose root
/// name is "undefined", as in TS).
pub fn detect_chord(midi_notes: &[i32]) -> Option<ChordInfo> {
    if midi_notes.len() < 2 {
        return None;
    }
    let mut pitch_classes: Vec<i32> = Vec::new();
    for n in midi_notes {
        let pc = n % 12;
        if !pitch_classes.contains(&pc) {
            pitch_classes.push(pc);
        }
    }
    pitch_classes.sort();
    if pitch_classes.len() < 2 {
        return None;
    }
    for &root in &pitch_classes {
        let mut intervals: Vec<i32> = pitch_classes
            .iter()
            .map(|pc| (pc - root + 12) % 12)
            .collect();
        intervals.sort();
        for (pattern, quality) in CHORD_PATTERNS {
            if intervals == pattern {
                let name = note_name_at(root as f64);
                return Some(ChordInfo {
                    root: name.to_string(),
                    quality: quality.to_string(),
                    display: format!("{name}{}", if quality == "maj" { "" } else { quality }),
                });
            }
        }
    }
    None
}

#[cfg(test)]
#[path = "chords_tests.rs"]
mod tests;
