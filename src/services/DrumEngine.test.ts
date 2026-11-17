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

describe('drumEngine held pads (review round 2: #3, #4)', () => {
  it('a held pad is released on the sink that played it, even after the sink changed', async () => {
    await drumEngine.start();
    const a = { hit: vi.fn(), release: vi.fn(), releaseAllLive: vi.fn() };
    const b = { hit: vi.fn(), release: vi.fn() };
    drumEngine.setSink(a);
    drumEngine.hit(4, 0.8); // gate pad goes down on track A
    drumEngine.setSink(b); // active drum track switches while it is held
    expect(a.releaseAllLive).toHaveBeenCalledTimes(1);
    drumEngine.release(4);
    expect(a.release).toHaveBeenCalledWith(4);
    expect(b.release).not.toHaveBeenCalled();
    // The next press/release cycle belongs to B only.
    drumEngine.hit(4, 0.8);
    drumEngine.release(4);
    expect(b.release).toHaveBeenCalledWith(4);
    expect(a.release).toHaveBeenCalledTimes(1);
    // A release with no hit since the last one still reaches the current sink.
    drumEngine.release(9);
    expect(b.release).toHaveBeenCalledWith(9);
    drumEngine.setSink(null);
  });

  it('release cancels note-repeat hits handed out ahead of time that have not started (one-shot pads too)', async () => {
    const { toneMock } = await import('@/test/toneMock');
    await drumEngine.start();
    await preloadKitBuffers(createFactoryKit());
    drumEngine.setSink(null); // fallback kit (factory: one-shot pads)
    toneMock.clock.now = 10;
    const before = instancesOf('ToneBufferSource').length;
    drumEngine.hit(0, 0.8); // the finger hit
    drumEngine.hit(0, 0.8, 10.0); // a repeat due now (already sounding)
    drumEngine.hit(0, 0.8, 10.03); // a repeat inside the look-ahead
    const [hit, due, ahead] = instancesOf('ToneBufferSource').slice(before);
    drumEngine.release(0);
    expect(ahead.disposed).toBe(true);
    expect(hit.disposed).toBeFalsy();
    expect(due.disposed).toBeFalsy();
  });
});
