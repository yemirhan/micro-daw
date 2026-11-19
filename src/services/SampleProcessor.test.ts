import { describe, expect, it } from 'vitest';
import '@/test/toneMock';
import { createFakeAudioBuffer } from '@/test/toneMock';
import { processAudio } from './SampleProcessor';
import { DEFAULT_PROCESSING } from '@/types/samples';

const params = (over: Partial<typeof DEFAULT_PROCESSING>) => ({
  ...structuredClone(DEFAULT_PROCESSING),
  ...over,
});

describe('processAudio', () => {
  it('normalize-only returns a scaled copy and leaves the source sample untouched', async () => {
    const src = createFakeAudioBuffer(1, 2, 48000);
    src.getChannelData(0).set([0.25, -0.5]);
    const out = await processAudio(src as unknown as AudioBuffer, params({ normalize: { enabled: true } }));
    expect(Array.from(out.getChannelData(0))).toEqual([0.5, -1]);
    // Before the fix the library's original became [0.5, -1] too (scaled in place).
    expect(Array.from(src.getChannelData(0))).toEqual([0.25, -0.5]);
    expect(out).not.toBe(src);
  });

  it('reverse + normalize keeps the source too', async () => {
    const src = createFakeAudioBuffer(2, 2, 44100);
    src.getChannelData(0).set([0.1, 0.2]);
    src.getChannelData(1).set([-0.4, 0]);
    await processAudio(
      src as unknown as AudioBuffer,
      params({ reverse: { enabled: true }, normalize: { enabled: true } }),
    );
    expect(Array.from(src.getChannelData(0))).toEqual([Math.fround(0.1), Math.fround(0.2)]);
    expect(Array.from(src.getChannelData(1))).toEqual([Math.fround(-0.4), 0]);
  });
});
