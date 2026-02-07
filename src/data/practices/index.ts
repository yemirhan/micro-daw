import { PIANO_BASICS } from './piano-basics';
import { SCALES } from './scales';
import { CHORDS } from './chords';
import { RHYTHM } from './rhythm';
import { DRUM_PATTERNS } from './drum-patterns';
import type { PracticeCategory, PracticeExercise } from '@/types/appMode';

export const ALL_PRACTICES: PracticeExercise[] = [
  ...PIANO_BASICS,
  ...SCALES,
  ...CHORDS,
  ...RHYTHM,
  ...DRUM_PATTERNS,
];

export function getPracticesByCategory(category?: PracticeCategory | null): PracticeExercise[] {
  if (!category) return ALL_PRACTICES;
  return ALL_PRACTICES.filter((e) => e.category === category);
}

export function getPracticeById(id: string): PracticeExercise | undefined {
  return ALL_PRACTICES.find((e) => e.id === id);
}
