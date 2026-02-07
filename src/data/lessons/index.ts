import cMajorScale from './piano/c-major-scale.json';
import findingNotes from './piano/finding-notes.json';
import twoHandNotes from './piano/two-hand-notes.json';
import majorChords from './piano/major-chords.json';
import minorChords from './piano/minor-chords.json';
import gMajorScale from './piano/g-major-scale.json';
import dMajorScale from './piano/d-major-scale.json';
import naturalMinorScale from './piano/natural-minor-scale.json';
import pentatonicScales from './piano/pentatonic-scales.json';
import bluesScale from './piano/blues-scale.json';
import seventhChords from './piano/seventh-chords.json';
import susAndDimChords from './piano/sus-and-dim-chords.json';
import chordProgressions from './piano/chord-progressions.json';

import basicRockBeat from './drums/basic-rock-beat.json';
import knowYourKit from './drums/know-your-kit.json';
import kickSnarePatterns from './drums/kick-snare-patterns.json';
import hiHatVariations from './drums/hi-hat-variations.json';
import rideCymbal from './drums/ride-cymbal.json';
import crashAccents from './drums/crash-accents.json';
import tomFills from './drums/tom-fills.json';
import popBeat from './drums/pop-beat.json';
import threePieceCombos from './drums/three-piece-combos.json';
import genreBeats from './drums/genre-beats.json';
import dynamicsAndLayers from './drums/dynamics-and-layers.json';

import intervals from './theory/intervals.json';
import semitonesAndTones from './theory/semitones-and-tones.json';
import majorScaleFormula from './theory/major-scale-formula.json';
import minorScaleFormula from './theory/minor-scale-formula.json';
import chordConstruction from './theory/chord-construction.json';
import keySignatures from './theory/key-signatures.json';
import relativeMajorMinor from './theory/relative-major-minor.json';
import diatonicChords from './theory/diatonic-chords.json';
import rhythmBasics from './theory/rhythm-basics.json';
import octavesAndPitch from './theory/octaves-and-pitch.json';
import harmonicMinorModes from './theory/harmonic-minor-modes.json';

import type { Lesson, LessonCategory } from '@/types/appMode';

export const ALL_LESSONS: Lesson[] = [
  // Piano — beginner
  cMajorScale as Lesson,
  findingNotes as Lesson,
  twoHandNotes as Lesson,
  majorChords as Lesson,
  minorChords as Lesson,
  gMajorScale as Lesson,
  // Piano — intermediate
  dMajorScale as Lesson,
  naturalMinorScale as Lesson,
  pentatonicScales as Lesson,
  bluesScale as Lesson,
  seventhChords as Lesson,
  susAndDimChords as Lesson,
  chordProgressions as Lesson,

  // Drums — beginner
  basicRockBeat as Lesson,
  knowYourKit as Lesson,
  kickSnarePatterns as Lesson,
  hiHatVariations as Lesson,
  rideCymbal as Lesson,
  crashAccents as Lesson,
  // Drums — intermediate
  tomFills as Lesson,
  popBeat as Lesson,
  threePieceCombos as Lesson,
  genreBeats as Lesson,
  dynamicsAndLayers as Lesson,

  // Theory — beginner
  intervals as Lesson,
  semitonesAndTones as Lesson,
  majorScaleFormula as Lesson,
  minorScaleFormula as Lesson,
  chordConstruction as Lesson,
  rhythmBasics as Lesson,
  octavesAndPitch as Lesson,
  // Theory — intermediate
  keySignatures as Lesson,
  relativeMajorMinor as Lesson,
  diatonicChords as Lesson,
  harmonicMinorModes as Lesson,
];

export function getLessonsByCategory(category?: LessonCategory): Lesson[] {
  if (!category) return ALL_LESSONS;
  return ALL_LESSONS.filter((l) => l.category === category);
}

export function getLessonById(id: string): Lesson | undefined {
  return ALL_LESSONS.find((l) => l.id === id);
}
