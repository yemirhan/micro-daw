import { describe, expect, it } from 'vitest';
import {
  isBlackKey,
  midiToFrequency,
  midiToNoteName,
  noteNameToMidi,
  velocityToNormalized,
} from './noteHelpers';

describe('noteHelpers', () => {
  it('converts MIDI numbers to note names', () => {
    expect(midiToNoteName(60)).toBe('C4');
    expect(midiToNoteName(69)).toBe('A4');
    expect(midiToNoteName(61)).toBe('C#4');
    expect(midiToNoteName(21)).toBe('A0');
  });

  it('round-trips note names', () => {
    for (let midi = 12; midi <= 119; midi++) {
      expect(noteNameToMidi(midiToNoteName(midi))).toBe(midi);
    }
    expect(noteNameToMidi('H4')).toBe(-1);
  });

  it('detects black keys', () => {
    expect([60, 61, 62, 63, 64, 65, 66].map(isBlackKey)).toEqual([
      false, true, false, true, false, false, true,
    ]);
  });

  it('computes equal-tempered frequencies', () => {
    expect(midiToFrequency(69)).toBeCloseTo(440);
    expect(midiToFrequency(81)).toBeCloseTo(880);
    expect(midiToFrequency(60)).toBeCloseTo(261.626, 2);
  });

  it('normalizes and clamps velocity', () => {
    expect(velocityToNormalized(127)).toBe(1);
    expect(velocityToNormalized(0)).toBe(0);
    expect(velocityToNormalized(200)).toBe(1);
  });
});
