import { describe, expect, it } from 'vitest';
import { detectChord } from './chordDetection';

describe('detectChord', () => {
  it('returns null for fewer than two pitch classes', () => {
    expect(detectChord([])).toBeNull();
    expect(detectChord([60])).toBeNull();
    expect(detectChord([60, 72])).toBeNull();
  });

  it('detects triads regardless of voicing', () => {
    expect(detectChord([60, 64, 67])?.display).toBe('C');
    expect(detectChord([64, 67, 72])?.display).toBe('C'); // first inversion
    expect(detectChord([57, 60, 64])?.display).toBe('Am');
    expect(detectChord([59, 62, 65])?.display).toBe('Bdim');
  });

  it('detects seventh chords', () => {
    expect(detectChord([67, 71, 74, 77])?.display).toBe('G7');
    expect(detectChord([60, 64, 67, 71])?.display).toBe('Cmaj7');
  });
});
