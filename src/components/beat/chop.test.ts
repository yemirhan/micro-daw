import { describe, expect, it } from 'vitest';
import { chopRanges } from './chop';

/** Fake mono buffer with clicks at the given seconds. */
function clicks(durationSec: number, at: number[], sampleRate = 8000) {
  const data = new Float32Array(Math.round(durationSec * sampleRate));
  for (const t of at) {
    const i0 = Math.round(t * sampleRate);
    for (let i = 0; i < 400 && i0 + i < data.length; i++) data[i0 + i] = 0.9 * Math.exp(-i / 80);
  }
  return { numberOfChannels: 1, length: data.length, sampleRate, getChannelData: () => data };
}

describe('chopRanges', () => {
  it('equal slices cover the trimmed part in absolute seconds', () => {
    const buf = clicks(4, []);
    const r = chopRanges(buf, { startSec: 1, endSec: 3 }, { kind: 'equal', count: 4 });
    expect(r).toEqual([
      { startSec: 1, endSec: 1.5 },
      { startSec: 1.5, endSec: 2 },
      { startSec: 2, endSec: 2.5 },
      { startSec: 2.5, endSec: 3 },
    ]);
  });

  it('never makes more slices than pads left from the start pad', () => {
    const buf = clicks(4, []);
    expect(chopRanges(buf, { startSec: 0, endSec: null }, { kind: 'equal', count: 16 }, 12)).toHaveLength(4);
  });

  it('finds hits inside the trim range', () => {
    const buf = clicks(2, [0.1, 0.6, 1.1, 1.6]);
    const r = chopRanges(buf, { startSec: 0.5, endSec: null }, { kind: 'transients', sensitivity: 0.5 });
    expect(r[0].startSec).toBe(0.5);
    expect(r.at(-1)!.endSec).toBe(2);
    // Slice 1 starts at the trim start; then one per hit at 0.6, 1.1 and 1.6.
    expect(r.length).toBe(4);
    expect(r[1].startSec).toBeCloseTo(0.6, 1);
    expect(r[3].startSec).toBeCloseTo(1.6, 1);
  });

  it('returns nothing for an empty trim range', () => {
    expect(chopRanges(clicks(1, []), { startSec: 0.5, endSec: 0.5 }, { kind: 'equal', count: 4 })).toEqual([]);
  });
});
