import { afterEach, describe, expect, it } from 'vitest';
import { createFakeAudioBuffer, instancesOf } from '@/test/toneMock';
import { createFactoryKit, defaultPadSettings } from '@/audio/sampler/factoryKit';
import type { KitState } from '@/types/sampler';
import { classifyPadName, demoPattern, DEMO_STEPS, kitPreview, kitRoles, padsAt } from './kitPreview';

function kitOf(names: (string | null)[]): KitState {
  return {
    id: 'k',
    name: 'K',
    pads: Array.from({ length: 16 }, (_, i) => ({
      ...defaultPadSettings(),
      sample: names[i] ? { source: { kind: 'file' as const, path: `/x/${names[i]}.wav` }, name: names[i]! } : null,
    })),
  };
}

describe('kitPreview', () => {
  it('classifies common drum names and abbreviations', () => {
    expect(classifyPadName('Kick')).toBe('kick');
    expect(classifyPadName('BD0010')).toBe('kick');
    expect(classifyPadName('808 bd long')).toBe('kick');
    expect(classifyPadName('SD0025')).toBe('snare');
    expect(classifyPadName('Clap')).toBe('clap');
    expect(classifyPadName('Closed HH')).toBe('closedHat');
    expect(classifyPadName('CH')).toBe('closedHat');
    expect(classifyPadName('Open HH')).toBe('openHat');
    expect(classifyPadName('OH_02')).toBe('openHat');
    expect(classifyPadName('Cowbell')).toBe('perc');
    expect(classifyPadName('Crash')).toBeNull();
  });

  it('finds roles by name, falling back to the usual pad layout', () => {
    expect(kitRoles(createFactoryKit())).toMatchObject({ kick: 0, snare: 1, closedHat: 2, openHat: 3, clap: 4 });
    // Names in an odd order.
    expect(kitRoles(kitOf(['Snare', 'Hat', 'Kick']))).toMatchObject({ kick: 2, snare: 0, closedHat: 1 });
    // Unknown names: pad 0 = kick, 1 = snare.
    expect(kitRoles(kitOf(['Thing A', 'Thing B']))).toEqual({ kick: 0, snare: 1 });
  });

  it('builds a 2-bar groove within range, using only pads with sounds', () => {
    const kit = createFactoryKit();
    const hits = demoPattern(kit);
    expect(hits.length).toBeGreaterThan(20);
    for (const h of hits) {
      expect(h.step).toBeGreaterThanOrEqual(0);
      expect(h.step).toBeLessThan(DEMO_STEPS);
      expect(kit.pads[h.pad].sample).not.toBeNull();
    }
    expect(hits.filter((h) => h.pad === 0 && h.step === 0)).toHaveLength(1);
    // Open hat replaces the closed hat on its steps.
    expect(hits.some((h) => h.pad === 2 && h.step === 14)).toBe(false);
    expect(hits.some((h) => h.pad === 3 && h.step === 14)).toBe(true);
  });

  it('walks the pads when no roles are found, and is empty for an empty kit', () => {
    expect(demoPattern(kitOf([null, null, null, null, null, 'Zap', 'Zip']))).toEqual([
      { step: 0, pad: 5, velocity: 0.85 },
      { step: 4, pad: 6, velocity: 0.85 },
    ]);
    expect(demoPattern(kitOf([]))).toEqual([]);
  });

  it('reports which pads sound at a given time', () => {
    const session = { id: 'x', t0: 10, stepDur: 0.1, hits: [{ step: 0, pad: 0, velocity: 1 }, { step: 2, pad: 1, velocity: 1 }, { step: 2, pad: 2, velocity: 1 }] };
    expect([...padsAt(session, 9.9)]).toEqual([]);
    expect([...padsAt(session, 10.05)]).toEqual([0]);
    expect([...padsAt(session, 10.25)].sort()).toEqual([1, 2]);
  });
});

describe('kitPreview player', () => {
  afterEach(() => kitPreview.stop());
  const live = (cls: string) => instancesOf(cls).filter((n) => !n.disposed);
  const fakeBuffer = () => createFakeAudioBuffer(1, 4410) as unknown as AudioBuffer;

  it('two plays before Tone.start resolves leave exactly one preview alive (no leaked kit)', async () => {
    const kit = kitOf(['Kick', 'Snare', 'Hat']);
    const a = kitPreview.play('a', kit);
    const b = kitPreview.play('b', kit);
    await Promise.all([a, b]);
    expect(kitPreview.playing).toBe('b');
    expect(live('Volume')).toHaveLength(1);
    kitPreview.stop();
    expect(live('Volume')).toHaveLength(0);
    // Every pad node of every kit that was built is released.
    expect(live('Panner')).toHaveLength(0);
  });

  it('stop() while a preview is loading releases it and the late load does not start playing', async () => {
    const kit = kitOf(['Kick']);
    let resolve!: (b: AudioBuffer | null) => void;
    const p = kitPreview.play('a', kit, { loadBuffer: () => new Promise((r) => (resolve = r)) });
    await Promise.resolve();
    await Promise.resolve();
    kitPreview.stop();
    resolve(fakeBuffer());
    expect(await p).toBeNull();
    expect(kitPreview.playing).toBeNull();
    expect(live('Volume')).toHaveLength(0);
    expect(instancesOf('ToneBufferSource')).toHaveLength(0);
  });

  it('pre-install previews play loaded buffers without a SamplerKit', async () => {
    const kit = kitOf(['Kick', 'Snare']);
    const loaded: number[] = [];
    const err = await kitPreview.play('remote/kit', kit, {
      loadBuffer: async (pad) => {
        loaded.push(pad);
        return fakeBuffer();
      },
    });
    expect(err).toBeNull();
    expect(loaded.sort()).toEqual([0, 1]);
    expect(kitPreview.playing).toBe('remote/kit');
    expect(instancesOf('Panner')).toHaveLength(0);
    const sources = instancesOf('ToneBufferSource');
    expect(sources.length).toBe(demoPattern(kit).length);
    expect(sources.every((s) => s.start.mock.calls.length === 1)).toBe(true);
    kitPreview.stop();
    expect(sources.every((s) => s.disposed)).toBe(true);
  });

  it('reports an error when no sound of a pre-install preview loads', async () => {
    const err = await kitPreview.play('remote/kit', kitOf(['Kick']), { loadBuffer: () => Promise.reject(new Error('offline')) });
    expect(err).toMatch(/couldn't load/i);
    expect(kitPreview.playing).toBeNull();
    expect(live('Volume')).toHaveLength(0);
  });

  it('plays the sounds that loaded and reports the ones that did not', async () => {
    const err = await kitPreview.play('remote/kit', kitOf(['Kick', 'Snare']), {
      loadBuffer: (pad) => (pad === 0 ? Promise.resolve(fakeBuffer()) : Promise.reject(new Error('HTTP 404'))),
    });
    expect(err).toBe("1 of 2 sounds couldn't load (HTTP 404)");
    expect(kitPreview.playing).toBe('remote/kit');
    expect(instancesOf('ToneBufferSource').length).toBeGreaterThan(0);
  });
});
