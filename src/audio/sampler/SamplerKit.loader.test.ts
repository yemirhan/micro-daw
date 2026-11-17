import { describe, expect, it, vi } from 'vitest';
import { createFakeAudioBuffer, instancesOf, type MockToneNode } from '@/test/toneMock';

const loadFromPath = vi.fn(async (path: string) => ({ id: `id:${path}`, name: 'x', path, durationSeconds: 1, sampleRate: 44100, channels: 1 }));
vi.mock('@/services/SampleManager', () => ({
  sampleManager: {
    loadFromPath: (p: string) => loadFromPath(p),
    getBuffer: () => createFakeAudioBuffer(1, 44100),
    getSample: () => undefined,
    getAllSamples: () => [],
    whenReloaded: async () => undefined,
  },
}));

import { SamplerKit } from './SamplerKit';
import { createFactoryKit } from './factoryKit';
import { getCachedPadBuffer } from './bufferCache';

const sources = () => instancesOf('ToneBufferSource');
const opts = (n: MockToneNode) => n.__args[0] as Record<string, unknown>;

/** SamplerKitOptions.loadBuffer: kit previews of packs that are not installed (review M3). */
describe('SamplerKit with an injected buffer loader', () => {
  it('loads pads through an injected loader instead of the buffer cache (review M3)', async () => {
    const kit = createFactoryKit();
    kit.pads[2].sample = { name: 'remote', source: { kind: 'file', path: 'remote:https://x/y.wav' } };
    const remote = createFakeAudioBuffer(1, 4410) as unknown as AudioBuffer;
    const loader = vi.fn(async (pad: number) => (pad === 2 ? remote : null));
    loadFromPath.mockClear();
    const s = new SamplerKit(kit, { loadBuffer: loader });
    await s.ready;
    expect(loadFromPath).not.toHaveBeenCalled();
    expect(loader).toHaveBeenCalledWith(2, kit.pads[2].sample);
    expect(getCachedPadBuffer({ kind: 'file', path: 'remote:https://x/y.wav' })).toBeFalsy();
    expect(s.isPadReady(2)).toBe(true);
    expect(s.isPadReady(0)).toBe(false); // loader said "no sound"
    s.trigger(2, 1, 0);
    expect(opts(sources()[0]).url).toBe(remote);
    s.dispose();
  });

  it('a rejecting injected loader leaves the pad silent and still settles ready', async () => {
    const s = new SamplerKit(createFactoryKit(), { loadBuffer: () => Promise.reject(new Error('offline')) });
    await expect(s.ready).resolves.toBeUndefined();
    expect(s.isPadReady(0)).toBe(false);
    s.dispose();
  });

});
