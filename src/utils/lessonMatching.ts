/**
 * Pure matching logic for lesson steps and practice exercises.
 * Kept free of React so it can be unit-tested and reused by Learn + Practice.
 */
import { detectChord } from '@/utils/chordDetection';
import type { ExpectedChord, LessonStep } from '@/types/appMode';
import type { DrumPadId } from '@/types/drums';

export const pitchClass = (midi: number): number => ((midi % 12) + 12) % 12;

const NOTE_INDEX: Record<string, number> = {
  C: 0, 'B#': 0, 'C#': 1, Db: 1, D: 2, 'D#': 3, Eb: 3, E: 4, Fb: 4, 'E#': 5, F: 5,
  'F#': 6, Gb: 6, G: 7, 'G#': 8, Ab: 8, A: 9, 'A#': 10, Bb: 10, B: 11, Cb: 11,
};

/** Pitch class (0-11) of a note name like "C", "F#", "Bb"; undefined if unknown. */
export function noteNameToPitchClass(name: string): number | undefined {
  const n = name.trim();
  if (!n) return undefined;
  const normalized = n[0].toUpperCase() + n.slice(1);
  return NOTE_INDEX[normalized];
}

/** Semitone intervals (from the root) of every chord quality lessons/exercises use. */
export const CHORD_QUALITY_INTERVALS: Record<string, number[]> = {
  maj: [0, 4, 7],
  m: [0, 3, 7],
  dim: [0, 3, 6],
  aug: [0, 4, 8],
  sus2: [0, 2, 7],
  sus4: [0, 5, 7],
  '5': [0, 7],
  '7': [0, 4, 7, 10],
  m7: [0, 3, 7, 10],
  maj7: [0, 4, 7, 11],
  dim7: [0, 3, 6, 9],
  m7b5: [0, 3, 6, 10],
};

const QUALITY_ALIASES: Record<string, string> = {
  '': 'maj', M: 'maj', major: 'maj', min: 'm', minor: 'm', '-': 'm',
  dom7: '7', M7: 'maj7', min7: 'm7', '-7': 'm7', '°': 'dim', o: 'dim', '+': 'aug',
};

export function normalizeChordQuality(quality: string): string {
  return QUALITY_ALIASES[quality] ?? quality;
}

/** Pitch classes of an expected chord, or null if root/quality are unknown. */
export function chordPitchClasses(chord: ExpectedChord): Set<number> | null {
  const root = noteNameToPitchClass(chord.root);
  const intervals = CHORD_QUALITY_INTERVALS[normalizeChordQuality(chord.quality)];
  if (root === undefined || !intervals) return null;
  return new Set(intervals.map((i) => (root + i) % 12));
}

/**
 * True when `notes` sound exactly the expected chord: same set of pitch
 * classes (any voicing / inversion / octave doubling), nothing extra.
 */
export function chordMatches(expected: ExpectedChord, notes: Iterable<number>): boolean {
  const target = chordPitchClasses(expected);
  if (!target) return false;
  const played = new Set([...notes].map(pitchClass));
  if (played.size !== target.size) return false;
  for (const pc of target) if (!played.has(pc)) return false;
  return true;
}

/** Display name of the chord formed by `notes` (for feedback), or null. */
export function describeChord(notes: Iterable<number>): string | null {
  return detectChord([...notes])?.display ?? null;
}

/** True when every expected note is in `notes` (exact MIDI, or pitch class when anyOctave). */
export function notesSatisfied(expected: number[], notes: Iterable<number>, anyOctave = false): boolean {
  const played = [...notes];
  if (expected.length === 0) return false;
  if (anyOctave) {
    const pcs = new Set(played.map(pitchClass));
    return expected.every((n) => pcs.has(pitchClass(n)));
  }
  const set = new Set(played);
  return expected.every((n) => set.has(n));
}

/**
 * How far into `expected` the player currently is: the length of the longest
 * prefix of `expected` that is a suffix of `played`. A wrong note therefore
 * restarts the sequence (but a wrong note that happens to be the first
 * expected note counts as a fresh start). `played.length` may be long; only
 * the tail matters.
 */
export function sequenceProgress(
  expected: number[],
  played: number[],
  equals: (a: number, b: number) => boolean = (a, b) => a === b,
): number {
  const max = Math.min(expected.length, played.length);
  for (let len = max; len > 0; len--) {
    let ok = true;
    const offset = played.length - len;
    for (let i = 0; i < len; i++) {
      if (!equals(expected[i], played[offset + i])) {
        ok = false;
        break;
      }
    }
    if (ok) return len;
  }
  return 0;
}

export const samePitchClass = (a: number, b: number) => pitchClass(a) === pitchClass(b);

/** Collapse immediately repeated values (a double-tapped key in a scale). */
export function collapseRepeats(values: number[]): number[] {
  const out: number[] = [];
  for (const v of values) if (out[out.length - 1] !== v) out.push(v);
  return out;
}

/**
 * Scale progress: pitch classes in order, any octave. Accidental double taps
 * of the same key are ignored (no scale repeats a degree consecutively).
 */
export function scaleProgress(scalePitchClasses: number[], playedNotes: number[]): number {
  const played = collapseRepeats(playedNotes.map(pitchClass));
  return sequenceProgress(scalePitchClasses.map(pitchClass), played);
}

export interface DrumHit {
  padId: DrumPadId;
  time: number;
}

/**
 * True when every expected pad was hit within `windowMs` of the latest hit.
 * ("Together" for MIDI pads is a few ms; the window also lets mouse users
 * click two on-screen pads in quick succession.)
 */
export function drumPadsSatisfied(expected: DrumPadId[], hits: DrumHit[], windowMs: number): boolean {
  if (expected.length === 0 || hits.length === 0) return false;
  const latest = hits[hits.length - 1].time;
  const recent = new Set(hits.filter((h) => latest - h.time <= windowMs).map((h) => h.padId));
  return expected.every((p) => recent.has(p));
}

/** How a play-notes step is judged. */
export function playNotesMode(step: LessonStep): 'together' | 'sequence' {
  return step.noteMode ?? 'together';
}

/** The ordered list of notes a step expects (MIDI numbers). */
export function expectedMidi(step: LessonStep): number[] {
  return (step.expectedNotes ?? []).map((n) => n.midi);
}
