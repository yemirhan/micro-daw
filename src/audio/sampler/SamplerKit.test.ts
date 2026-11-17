import { beforeEach, describe, expect, it, vi } from 'vitest';
import * as Tone from 'tone';
import { createFakeAudioBuffer, instancesOf, type MockToneNode } from '@/test/toneMock';
import type { KitState } from '@/types/sampler';

const loadFromPath = vi.fn(async (path: string) => ({ id: `id:${path}`, name: 'x', path, durationSeconds: 1, sampleRate: 44100, channels: 1 }));
const getBuffer = vi.fn(() => createFakeAudioBuffer(1, 44100));
vi.mock('@/services/SampleManager', () => ({
  sampleManager: {
    loadFromPath: (p: string) => loadFromPath(p),
    getBuffer: () => getBuffer(),
    getSample: () => undefined,
    getAllSamples: () => [],
  },
}));

import { SamplerKit, computeSlice, MAX_VOICES_PER_PAD } from './SamplerKit';
import { createFactoryKit, cloneKit } from './factoryKit';
import { clearPadBufferCache, getCachedPadBuffer, getReversedBuffer, loadPadBuffer, trimTail } from './bufferCache';

const sources = () => instancesOf('ToneBufferSource');
const opts = (n: MockToneNode) => n.__args[0] as Record<string, unknown>;

async function readyKit(kit: KitState = createFactoryKit()) {
  const s = new SamplerKit(kit);
  await s.ready;
  return s;
}

beforeEach(() => {
  clearPadBufferCache();
});

describe('computeSlice', () => {
  const base = { startSec: 0, endSec: null, pitch: 0, reverse: false };
  it('applies trim and pitch', () => {
    expect(computeSlice({ ...base, startSec: 0.25, endSec: 0.75 }, 1)).toEqual({ offset: 0.25, length: 0.5, rate: 1, realDuration: 0.5 });
    const up = computeSlice({ ...base, pitch: 12 }, 1)!;
    expect(up.rate).toBeCloseTo(2);
    expect(up.realDuration).toBeCloseTo(0.5);
  });
  it('mirrors the offset when reversed', () => {
    expect(computeSlice({ ...base, startSec: 0.1, endSec: 0.4, reverse: true }, 1)!.offset).toBeCloseTo(0.6);
  });
  it('rejects empty slices', () => {
    expect(computeSlice({ ...base, startSec: 0.5, endSec: 0.5 }, 1)).toBeNull();
  });
});

describe('bufferCache', () => {
  it('renders each factory voice once via Tone.Offline and caches it', async () => {
    const offline = vi.mocked(Tone.Offline);
    offline.mockClear();
    const a = await loadPadBuffer({ kind: 'factory', voice: 'kick' });
    const b = await loadPadBuffer({ kind: 'factory', voice: 'kick' });
    expect(a).toBe(b);
    expect(offline).toHaveBeenCalledTimes(1);
    expect(getCachedPadBuffer({ kind: 'factory', voice: 'kick' })).toBe(a);
    expect(getCachedPadBuffer({ kind: 'factory', voice: 'snare' })).toBeNull();
  });

  it('loads files through SampleManager and records the sampleId', async () => {
    const source: { kind: 'file'; path: string; sampleId?: string } = { kind: 'file', path: '/s/hit.wav' };
    await loadPadBuffer(source);
    expect(loadFromPath).toHaveBeenCalledWith('/s/hit.wav');
  });

  it('trimTail cuts silence, fades out and limits peaks', () => {
    const sr = 1000;
    const ch = new Float32Array(1000);
    ch[0] = 2;
    ch[100] = 0.5;
    const [out] = trimTail([ch], sr, { padSec: 0.01, fadeSec: 0.005 });
    expect(out.length).toBe(111);
    expect(out[0]).toBeCloseTo(0.98);
    expect(out[100]).toBeCloseTo(0.245);
    expect(out[out.length - 1]).toBe(0);
  });

  it('reverses buffers once', () => {
    const buf = createFakeAudioBuffer(1, 4) as unknown as AudioBuffer;
    buf.getChannelData(0).set([1, 2, 3, 4]);
    const r = getReversedBuffer(buf);
    expect(Array.from(r.getChannelData(0))).toEqual([4, 3, 2, 1]);
    expect(getReversedBuffer(buf)).toBe(r);
  });
});

describe('SamplerKit', () => {
  it('plays a pad with velocity gain, pitch and a scheduled stop', async () => {
    const kit = createFactoryKit();
    kit.pads[0].pitch = 12;
    const s = await readyKit(kit);
    s.trigger(0, 127, 1);
    const [src] = sources();
    expect(opts(src).playbackRate).toBeCloseTo(2);
    const [time, offset, , gain] = src.start.mock.calls[0];
    expect([time, offset, gain]).toEqual([1, 0, 1]);
    expect(src.stop).toHaveBeenCalledTimes(1);
    s.trigger(0, 0.5, 2);
    expect(sources()[1].start.mock.calls[0][3]).toBeLessThan(0.5);
  });

  it('ignores invalid pads, zero velocity and empty pads', async () => {
    const kit = createFactoryKit();
    kit.pads[4].sample = null;
    const s = await readyKit(kit);
    s.trigger(4, 1, 0);
    s.trigger(16, 1, 0);
    s.trigger(0, 0, 0);
    expect(sources()).toHaveLength(0);
  });

  it('chokes other pads in the same group', async () => {
    const s = await readyKit();
    s.trigger(3, 1, 0); // open hat (choke group 1)
    const open = sources()[0];
    open.stop.mockClear();
    s.trigger(2, 1, 0.001); // closed hat
    expect(open.stop).toHaveBeenCalledWith(0.001);
    expect(open.fadeOut).toBeCloseTo(0.005);
  });

  it('caps polyphony per pad by stealing the oldest voice', async () => {
    const kit = createFactoryKit();
    const s = await readyKit(kit);
    // Factory buffers in the mock are tiny (10 ms); trigger faster than that.
    for (let i = 0; i <= MAX_VOICES_PER_PAD; i++) s.trigger(0, 1, i * 0.0001);
    const first = sources()[0];
    expect(first.stop).toHaveBeenCalledTimes(2); // scheduled end + steal
  });

  it('gate mode releases on release(), one-shot ignores it', async () => {
    const kit = createFactoryKit();
    kit.pads[0].mode = 'gate';
    kit.pads[0].decay = 0.002;
    const s = await readyKit(kit);
    s.trigger(0, 1, 0);
    s.trigger(1, 1, 0);
    const [gate, oneshot] = sources();
    gate.stop.mockClear();
    oneshot.stop.mockClear();
    s.release(0, 0.001);
    s.release(1, 0.001);
    expect(gate.stop).toHaveBeenCalledWith(0.001);
    expect(oneshot.stop).not.toHaveBeenCalled();
  });

  it('bypasses the filter when open and inserts it when closed', async () => {
    const s = await readyKit();
    // (the factory renders create DrumKit filters too: count only new ones)
    const before = instancesOf('Filter').length;
    const kit = cloneKit(s.getKit());
    kit.pads[0].filterCutoff = 800;
    s.setKit(kit);
    const filters = instancesOf('Filter').slice(before);
    expect(filters).toHaveLength(1);
    expect(filters[0].frequency.value).toBe(800);
    kit.pads[1].filterCutoff = 20000;
    s.setKit(kit);
    expect(instancesOf('Filter').length).toBe(before + 1);
  });

  it('reloads only pads whose sample changed', async () => {
    const s = await readyKit();
    const kit = cloneKit(s.getKit());
    kit.pads[5].sample = { name: 'hit', source: { kind: 'file', path: '/x/hit.wav' } };
    kit.pads[0].gainDb = -6;
    loadFromPath.mockClear();
    s.setKit(kit);
    await s.ready;
    expect(loadFromPath).toHaveBeenCalledTimes(1);
    expect(s.isPadReady(5)).toBe(true);
  });

  it('plays reversed slices from a reversed buffer', async () => {
    const kit = createFactoryKit();
    kit.pads[0].reverse = true;
    const s = await readyKit(kit);
    s.trigger(0, 1, 0);
    const buf = getCachedPadBuffer({ kind: 'factory', voice: 'kick' })!;
    expect(opts(sources()[0]).url).toBe(getReversedBuffer(buf));
  });

  it('stopAll and dispose are safe', async () => {
    const s = await readyKit();
    s.trigger(0, 1, 0);
    expect(() => s.stopAll(0.001)).not.toThrow();
    s.dispose();
    expect(() => s.trigger(0, 1, 1)).not.toThrow();
    expect(sources()).toHaveLength(1);
  });
});
