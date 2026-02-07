export interface ChordInfo {
  root: string;        // e.g. "C", "F#"
  quality: string;     // e.g. "maj", "m", "dim", "aug", "7", "m7"
  display: string;     // e.g. "Cmaj", "Am7"
}

export interface ScaleDefinition {
  name: ScaleName;
  intervals: number[]; // semitones from root (pitch classes)
}

export type ScaleName =
  | 'Major'
  | 'Natural Minor'
  | 'Harmonic Minor'
  | 'Dorian'
  | 'Mixolydian'
  | 'Pentatonic Major'
  | 'Pentatonic Minor'
  | 'Blues'
  | 'Chromatic'
  | 'Melodic Minor';

export type NoteLetter = 'C' | 'C#' | 'D' | 'D#' | 'E' | 'F' | 'F#' | 'G' | 'G#' | 'A' | 'A#' | 'B';
