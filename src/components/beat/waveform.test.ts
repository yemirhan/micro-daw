import { describe, expect, it } from 'vitest';
import { computePeaks, getPeaks } from './waveform';

const buffer = (channels: number[][]) => ({
  numberOfChannels: channels.length,
  length: channels[0].length,
  getChannelData: (c: number) => Float32Array.from(channels[c]),
});

describe('waveform peaks', () => {
  it('computes min/max per column across channels', () => {
    const p = computePeaks(buffer([[0, 0.5, -0.25, 0], [0.1, 0, 0, -0.75]]), 2);
    expect([...p.max]).toEqual([0.5, 0]);
    expect([...p.min]).toEqual([0, -0.75]);
  });

  it('handles empty buffers', () => {
    const p = computePeaks({ numberOfChannels: 1, length: 0, getChannelData: () => new Float32Array() }, 4);
    expect([...p.max]).toEqual([0, 0, 0, 0]);
  });

  it('caches by buffer and resolution', () => {
    const b = buffer([[0, 1, 0, -1]]);
    expect(getPeaks(b, 2)).toBe(getPeaks(b, 2));
    expect(getPeaks(b, 4)).not.toBe(getPeaks(b, 2));
  });
});
