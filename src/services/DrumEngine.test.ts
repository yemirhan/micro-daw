import { describe, expect, it, vi } from 'vitest';
import { instancesOf } from '@/test/toneMock';
import { drumEngine } from './DrumEngine';
import { kitStore } from './KitStore';
import { createFactoryKit } from '@/audio/sampler/factoryKit';
import { preloadKitBuffers } from '@/audio/sampler/bufferCache';

describe('drumEngine', () => {
  it('routes hits/releases to the sink with normalised velocity', async () => {
    await drumEngine.start();
    const sink = { hit: vi.fn(), release: vi.fn() };
    drumEngine.setSink(sink);
    drumEngine.hit(3, 127);
    drumEngine.release(3);
    expect(sink.hit).toHaveBeenCalledWith(3, 1);
    expect(sink.release).toHaveBeenCalledWith(3);
    drumEngine.mute();
    drumEngine.hit(3, 1);
    expect(sink.hit).toHaveBeenCalledTimes(1);
    drumEngine.unmute();
    drumEngine.setSink(null);
  });

  it('plays the fallback kit without a sink', async () => {
    await drumEngine.start();
    await preloadKitBuffers(createFactoryKit());
    const before = instancesOf('ToneBufferSource').length;
    drumEngine.hit(0, 0.8);
    expect(instancesOf('ToneBufferSource').length).toBe(before + 1);
    expect(drumEngine.hitByMidiNote(38, 100)).toBe(true);
    expect(drumEngine.hitByMidiNote(60, 100)).toBe(false);
  });

  it('previews a sample and stops the previous preview', async () => {
    await drumEngine.preview({ kind: 'factory', voice: 'snare' });
    const first = instancesOf('ToneBufferSource').at(-1)!;
    expect(first.start).toHaveBeenCalled();
    await drumEngine.preview({ kind: 'factory', voice: 'clap' });
    expect(first.stop).toHaveBeenCalled();
    drumEngine.stopPreview();
  });
});

describe('kitStore', () => {
  it('saves, lists, replaces and removes kits; strips runtime sample ids', () => {
    const kit = { ...createFactoryKit(), id: 'mine', name: 'Mine' };
    kit.pads[0].sample = { name: 'hit', source: { kind: 'file', path: '/a/hit.wav', sampleId: 'abc' } };
    const cb = vi.fn();
    const off = kitStore.subscribe(cb);
    kitStore.save(kit);
    expect(kitStore.list()).toHaveLength(1);
    expect(kitStore.list()[0].pads[0].sample?.source).toEqual({ kind: 'file', path: '/a/hit.wav' });
    kitStore.save({ ...kit, name: 'Renamed' });
    expect(kitStore.list().map((k) => k.name)).toEqual(['Renamed']);
    kitStore.remove('mine');
    expect(kitStore.list()).toEqual([]);
    expect(cb).toHaveBeenCalledTimes(3);
    off();
  });

  it('survives corrupt storage', () => {
    localStorage.setItem('micro-daw:kits', '{nope');
    expect(kitStore.list()).toEqual([]);
  });
});

describe('drumEngine scheduled hits', () => {
  it('passes the audio time to the sink / fallback kit (late times play now)', async () => {
    const { toneMock } = await import('@/test/toneMock');
    await drumEngine.start();
    const sink = { hit: vi.fn(), release: vi.fn() };
    drumEngine.setSink(sink);
    toneMock.clock.now = 2;
    drumEngine.hit(1, 0.5, 2.1);
    expect(sink.hit).toHaveBeenLastCalledWith(1, 0.5, 2.1);
    drumEngine.hit(1, 0.5, 1.9);
    expect(sink.hit).toHaveBeenLastCalledWith(1, 0.5, 2);
    drumEngine.hit(1, 0.5);
    expect(sink.hit).toHaveBeenLastCalledWith(1, 0.5);
    drumEngine.setSink(null);
  });
});
