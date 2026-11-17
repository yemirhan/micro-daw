import { describe, expect, it } from 'vitest';
import {
  chordMatches,
  drumPadsSatisfied,
  noteNameToPitchClass,
  notesSatisfied,
  scaleProgress,
  sequenceProgress,
} from './lessonMatching';

describe('lessonMatching', () => {
  it('parses note names with sharps and flats', () => {
    expect(noteNameToPitchClass('C')).toBe(0);
    expect(noteNameToPitchClass('Eb')).toBe(3);
    expect(noteNameToPitchClass('D#')).toBe(3);
    expect(noteNameToPitchClass('bb')).toBe(10);
    expect(noteNameToPitchClass('H')).toBeUndefined();
  });

  it('matches chords in any voicing, with doublings, but nothing extra', () => {
    const C = { root: 'C', quality: 'maj', display: 'C' };
    expect(chordMatches(C, [60, 64, 67])).toBe(true);
    expect(chordMatches(C, [52, 55, 60, 72])).toBe(true);
    expect(chordMatches(C, [60, 64, 67, 70])).toBe(false);
    expect(chordMatches(C, [60, 64])).toBe(false);
    expect(chordMatches({ root: 'Bb', quality: 'm7', display: 'Bbm7' }, [58, 61, 65, 68])).toBe(true);
    expect(chordMatches({ root: 'C', quality: 'weird', display: '?' }, [60])).toBe(false);
  });

  it('checks held notes exactly or by pitch class', () => {
    expect(notesSatisfied([60, 64], [64, 60, 67])).toBe(true);
    expect(notesSatisfied([60, 64], [72, 64])).toBe(false);
    expect(notesSatisfied([60, 64], [72, 76], true)).toBe(true);
    expect(notesSatisfied([], [60])).toBe(false);
  });

  it('measures sequence progress by the matching tail', () => {
    expect(sequenceProgress([1, 2, 3], [9, 1, 2])).toBe(2);
    expect(sequenceProgress([1, 2, 3], [1, 2, 3])).toBe(3);
    expect(sequenceProgress([1, 1, 2], [1, 1, 1])).toBe(2);
    expect(sequenceProgress([1, 2, 3], [])).toBe(0);
  });

  it('scale progress is octave-agnostic and forgives double taps', () => {
    expect(scaleProgress([0, 2, 4], [48, 62, 62, 76])).toBe(3);
    expect(scaleProgress([0, 2, 4], [60, 64])).toBe(0);
  });

  it('drum pads count as together within the window only', () => {
    expect(drumPadsSatisfied([0, 2], [{ padId: 0, time: 0 }, { padId: 2, time: 300 }], 600)).toBe(true);
    expect(drumPadsSatisfied([0, 2], [{ padId: 0, time: 0 }, { padId: 2, time: 900 }], 600)).toBe(false);
  });
});
