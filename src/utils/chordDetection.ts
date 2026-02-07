import type { ChordInfo } from '@/types/music';

const NOTE_NAMES = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'] as const;

// Interval patterns (semitones from root) for chord qualities
const CHORD_PATTERNS: [number[], string][] = [
  // Triads
  [[0, 4, 7], 'maj'],
  [[0, 3, 7], 'm'],
  [[0, 3, 6], 'dim'],
  [[0, 4, 8], 'aug'],
  [[0, 2, 7], 'sus2'],
  [[0, 5, 7], 'sus4'],
  [[0, 7], '5'],
  // Seventh chords
  [[0, 4, 7, 10], '7'],
  [[0, 3, 7, 10], 'm7'],
  [[0, 4, 7, 11], 'maj7'],
  [[0, 3, 6, 9], 'dim7'],
  [[0, 3, 6, 10], 'm7b5'],
];

export function detectChord(midiNotes: number[]): ChordInfo | null {
  if (midiNotes.length < 2) return null;

  // Get unique pitch classes (0-11)
  const pitchClasses = [...new Set(midiNotes.map((n) => n % 12))].sort((a, b) => a - b);
  if (pitchClasses.length < 2) return null;

  // Try each pitch class as a potential root
  for (const root of pitchClasses) {
    const intervals = pitchClasses.map((pc) => (pc - root + 12) % 12).sort((a, b) => a - b);

    for (const [pattern, quality] of CHORD_PATTERNS) {
      if (intervalsMatch(intervals, pattern)) {
        return {
          root: NOTE_NAMES[root],
          quality,
          display: `${NOTE_NAMES[root]}${quality === 'maj' ? '' : quality}`,
        };
      }
    }
  }

  return null;
}

function intervalsMatch(intervals: number[], pattern: number[]): boolean {
  if (intervals.length !== pattern.length) return false;
  return intervals.every((v, i) => v === pattern[i]);
}
