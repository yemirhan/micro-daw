import type { ScaleDefinition, ScaleName } from '@/types/music';

export const SCALE_DEFINITIONS: ScaleDefinition[] = [
  { name: 'Major', intervals: [0, 2, 4, 5, 7, 9, 11] },
  { name: 'Natural Minor', intervals: [0, 2, 3, 5, 7, 8, 10] },
  { name: 'Harmonic Minor', intervals: [0, 2, 3, 5, 7, 8, 11] },
  { name: 'Dorian', intervals: [0, 2, 3, 5, 7, 9, 10] },
  { name: 'Mixolydian', intervals: [0, 2, 4, 5, 7, 9, 10] },
  { name: 'Pentatonic Major', intervals: [0, 2, 4, 7, 9] },
  { name: 'Pentatonic Minor', intervals: [0, 3, 5, 7, 10] },
  { name: 'Blues', intervals: [0, 3, 5, 6, 7, 10] },
  { name: 'Chromatic', intervals: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11] },
  { name: 'Melodic Minor', intervals: [0, 2, 3, 5, 7, 9, 11] },
];

const NOTE_NAMES = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'] as const;

export function getScaleNotes(rootIndex: number, scale: ScaleDefinition): Set<number> {
  return new Set(scale.intervals.map((i) => (rootIndex + i) % 12));
}

export function isNoteInScale(midiNote: number, scaleNotes: Set<number>): boolean {
  return scaleNotes.has(midiNote % 12);
}

export function getRootNoteOptions(): { value: number; label: string }[] {
  return NOTE_NAMES.map((name, i) => ({ value: i, label: name }));
}

export function getScaleOptions(): { value: ScaleName; label: string }[] {
  return SCALE_DEFINITIONS.map((s) => ({ value: s.name, label: s.name }));
}
