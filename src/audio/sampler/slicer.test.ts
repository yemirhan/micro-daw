import { describe, expect, it } from 'vitest';
import { detectTransients, equalSlices, slicesToRanges } from './slicer';

const SR = 44100;

/** Deterministic PRNG so the noise is the same on every run. */
function rng(seed: number) {
  let s = seed >>> 0;
  return () => {
    s = (s * 1664525 + 1013904223) >>> 0;
    return s / 0x100000000 - 0.5;
  };
}

/** Decaying noise bursts ("drum hits") at the given times over a quiet noise floor. */
function synth(durationSec: number, hits: { t: number; amp?: number }[], floor = 0.0005): Float32Array {
  const out = new Float32Array(Math.round(durationSec * SR));
  const r = rng(7);
  for (let i = 0; i < out.length; i++) out[i] = r() * 2 * floor;
  for (const { t, amp = 0.8 } of hits) {
    const start = Math.round(t * SR);
    for (let i = 0; i < 0.15 * SR && start + i < out.length; i++) {
      out[start + i] += amp * r() * 2 * Math.exp(-i / (0.03 * SR));
    }
  }
  return out;
}

const near = (actual: number[], expected: number[], tol = 0.005) => {
  expect(actual).toHaveLength(expected.length);
  actual.forEach((a, i) => expect(Math.abs(a - expected[i])).toBeLessThan(tol));
};

describe('detectTransients', () => {
  it('finds evenly spaced clicks', () => {
    const data = synth(2, [{ t: 0 }, { t: 0.5 }, { t: 1 }, { t: 1.5 }]);
    near(detectTransients(data, SR), [0, 0.5, 1, 1.5]);
  });

  it('always starts with 0 even when the first hit is later', () => {
    const data = synth(1.5, [{ t: 0.25 }, { t: 0.8 }]);
    near(detectTransients(data, SR), [0, 0.25, 0.8]);
  });

  it('accepts an AudioBuffer-like (mixes channels down)', () => {
    const data = synth(1, [{ t: 0 }, { t: 0.4 }]);
    const buffer = { numberOfChannels: 2, length: data.length, getChannelData: () => data };
    near(detectTransients(buffer as unknown as AudioBuffer, SR), [0, 0.4]);
  });

  it('sensitivity controls whether quiet hits count', () => {
    const data = synth(1.5, [{ t: 0 }, { t: 0.5, amp: 0.8 }, { t: 1, amp: 0.01 }]);
    near(detectTransients(data, SR, { sensitivity: 0 }), [0, 0.5]);
    near(detectTransients(data, SR, { sensitivity: 1 }), [0, 0.5, 1]);
  });

  it('keeps the strongest hits when capped by maxSlices', () => {
    const data = synth(2, [{ t: 0 }, { t: 0.4, amp: 0.1 }, { t: 0.8, amp: 0.9 }, { t: 1.2, amp: 0.2 }]);
    near(detectTransients(data, SR, { maxSlices: 2, sensitivity: 0.8 }), [0, 0.8]);
  });

  it('merges hits closer than minGapSec', () => {
    const data = synth(1, [{ t: 0 }, { t: 0.3 }, { t: 0.33 }]);
    expect(detectTransients(data, SR, { minGapSec: 0.1 })).toHaveLength(2);
  });

  it('returns [0] for silence and empty input', () => {
    expect(detectTransients(new Float32Array(SR), SR)).toEqual([0]);
    expect(detectTransients(new Float32Array(0), SR)).toEqual([0]);
  });
});

describe('equalSlices / slicesToRanges', () => {
  it('splits evenly', () => {
    expect(equalSlices(2, 4)).toEqual([0, 0.5, 1, 1.5]);
    expect(equalSlices(1, 0)).toEqual([0]);
  });

  it('turns starts into ranges ending at the next start', () => {
    expect(slicesToRanges([0, 0.5, 1], 2)).toEqual([
      { startSec: 0, endSec: 0.5 },
      { startSec: 0.5, endSec: 1 },
      { startSec: 1, endSec: 2 },
    ]);
  });
});
