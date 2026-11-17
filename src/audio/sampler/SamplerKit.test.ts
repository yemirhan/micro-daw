import { beforeEach, describe, expect, it, vi } from 'vitest';
import * as Tone from 'tone';
import { createFakeAudioBuffer, instancesOf, toneMock, type MockToneNode } from '@/test/toneMock';
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

import { SamplerKit, computeSlice, voiceEnvelope, CHOKE_FADE, MAX_VOICES_PER_PAD } from './SamplerKit';
import { createFactoryKit, cloneKit } from './factoryKit';
import {
  LruCache,
  PREVIEW_CACHE_SIZE,
  clearPadBufferCache,
  getCachedPadBuffer,
  getReversedBuffer,
  loadPadBuffer,
  loadPreviewBuffer,
  previewCacheKeys,
  trimTail,
} from './bufferCache';

const sources = () => instancesOf('ToneBufferSource');
const opts = (n: MockToneNode) => n.__args[0] as Record<string, unknown>;
/** The per-voice envelope Gain a source is connected to. */
const envOf = (src: MockToneNode) => src.connect.mock.calls[0][0] as MockToneNode;

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

describe('preview cache', () => {
  it('LruCache evicts the least recently used entry', () => {
    const lru = new LruCache<string, number>(2);
    lru.set('a', 1);
    lru.set('b', 2);
    expect(lru.get('a')).toBe(1); // a is now the most recent
    lru.set('c', 3);
    expect(lru.keys()).toEqual(['a', 'c']);
    expect(lru.has('b')).toBe(false);
  });

  it('auditioned files are decoded into a bounded LRU, never registered with SampleManager', async () => {
    const sampleReadFile = vi.fn(async () => new ArrayBuffer(8));
    const w = window as unknown as { electronAPI?: unknown };
    const prev = w.electronAPI;
    w.electronAPI = { sampleReadFile };
    try {
      loadFromPath.mockClear();
      for (let i = 0; i < PREVIEW_CACHE_SIZE + 4; i++) await loadPreviewBuffer({ kind: 'file', path: `/lib/${i}.wav` });
      expect(loadFromPath).not.toHaveBeenCalled();
      expect(previewCacheKeys()).toHaveLength(PREVIEW_CACHE_SIZE);
      expect(previewCacheKeys()[0]).toBe('file:/lib/4.wav');
      expect(getCachedPadBuffer({ kind: 'file', path: '/lib/11.wav' })).toBeNull();
      // Re-auditioning a cached preview does not decode again.
      sampleReadFile.mockClear();
      await loadPreviewBuffer({ kind: 'file', path: '/lib/11.wav' });
      expect(sampleReadFile).not.toHaveBeenCalled();
      // A file loaded as a pad sample is reused for previews and leaves the preview LRU.
      await loadPadBuffer({ kind: 'file', path: '/lib/11.wav' });
      expect(previewCacheKeys()).not.toContain('file:/lib/11.wav');
      const padBuf = getCachedPadBuffer({ kind: 'file', path: '/lib/11.wav' });
      expect(await loadPreviewBuffer({ kind: 'file', path: '/lib/11.wav' })).toBe(padBuf);
    } finally {
      w.electronAPI = prev;
    }
  });
});

describe('voiceEnvelope', () => {
  it('holds the gain, then fades linearly to silence at the end', () => {
    expect(voiceEnvelope(1, 2, 0.8, 0, 0.5)).toEqual([
      { kind: 'set', value: 0.8, time: 1 },
      { kind: 'set', value: 0.8, time: 2.5 },
      { kind: 'ramp', value: 0, time: 3 },
    ]);
  });
  it('fades in first and never starts the fade-out before the fade-in is done', () => {
    expect(voiceEnvelope(0, 1, 1, 0.002, 1)).toEqual([
      { kind: 'set', value: 0, time: 0 },
      { kind: 'ramp', value: 1, time: 0.002 },
      { kind: 'ramp', value: 0, time: 1 },
    ]);
  });
  it('points are in time order', () => {
    const pts = voiceEnvelope(3, 0.01, 1, 0.002, 4);
    for (let i = 1; i < pts.length; i++) expect(pts[i].time).toBeGreaterThanOrEqual(pts[i - 1].time);
    expect(pts.at(-1)).toMatchObject({ value: 0, time: 3.01 });
  });
});

describe('SamplerKit', () => {
  it('plays a pad with velocity gain (on its own envelope), pitch and one scheduled stop', async () => {
    const kit = createFactoryKit();
    kit.pads[0].pitch = 12;
    const s = await readyKit(kit);
    s.trigger(0, 127, 1);
    const [src] = sources();
    expect(opts(src).playbackRate).toBeCloseTo(2);
    expect(opts(src).fadeOut).toBe(0);
    const [time, offset, , gain] = src.start.mock.calls[0];
    expect([time, offset, gain]).toEqual([1, 0, 1]);
    expect(src.stop).toHaveBeenCalledTimes(1);
    const env = envOf(src);
    expect(env.__className).toBe('Gain');
    expect(env.gain.setValueAtTime).toHaveBeenCalledWith(1, 1);
    s.trigger(0, 0.5, 2);
    const softEnv = envOf(sources()[1]);
    expect(softEnv.gain.setValueAtTime.mock.calls[0][0]).toBeLessThan(0.5);
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

  it('chokes other pads in the same group by fading their envelope from the held level', async () => {
    const s = await readyKit();
    s.trigger(3, 1, 0); // open hat (choke group 1)
    const open = sources()[0];
    const env = envOf(open);
    s.trigger(2, 1, 0.001); // closed hat
    // Never a second stop() (Tone would cancel the running fade and jump back to full level).
    expect(open.stop).toHaveBeenCalledTimes(1);
    expect(env.gain.cancelAndHoldAtTime).toHaveBeenCalledWith(0.001);
    expect(env.gain.linearRampToValueAtTime).toHaveBeenLastCalledWith(0, 0.001 + CHOKE_FADE);
  });

  it('a choke does not silence a voice scheduled after the choking hit', async () => {
    const kit = createFactoryKit();
    kit.pads[3].decay = 0.005;
    const s = await readyKit(kit);
    s.trigger(3, 1, 0.5); // open hat scheduled ahead (pattern look-ahead)
    const env = envOf(sources()[0]);
    s.trigger(2, 1, 0.4); // closed hat before it
    expect(env.gain.cancelAndHoldAtTime).not.toHaveBeenCalled();
    s.trigger(2, 1, 0.502); // closed hat while it sounds (mock buffers are 10 ms): chokes it
    expect(env.gain.cancelAndHoldAtTime).toHaveBeenCalledWith(0.502);
  });

  it('a second choke during a fade never re-stops the source', async () => {
    const kit = createFactoryKit();
    kit.pads[3].decay = 4;
    const s = await readyKit(kit);
    s.trigger(3, 1, 0);
    const open = sources()[0];
    const env = envOf(open);
    s.trigger(2, 1, 0.001);
    s.trigger(2, 1, 0.0012); // still fading: nothing to do
    expect(env.gain.cancelAndHoldAtTime).toHaveBeenCalledTimes(1);
    expect(open.stop).toHaveBeenCalledTimes(1);
  });

  it('caps polyphony per pad by fading out the oldest voice', async () => {
    const kit = createFactoryKit();
    const s = await readyKit(kit);
    // Factory buffers in the mock are tiny (10 ms); trigger faster than that.
    for (let i = 0; i <= MAX_VOICES_PER_PAD; i++) s.trigger(0, 1, i * 0.0001);
    const first = sources()[0];
    expect(first.stop).toHaveBeenCalledTimes(1); // only the scheduled end
    expect(envOf(first).gain.cancelAndHoldAtTime).toHaveBeenCalledWith(MAX_VOICES_PER_PAD * 0.0001);
  });

  it('gate mode releases on release(), one-shot ignores it', async () => {
    const kit = createFactoryKit();
    kit.pads[0].mode = 'gate';
    kit.pads[0].decay = 0.002;
    const s = await readyKit(kit);
    s.trigger(0, 1, 0);
    s.trigger(1, 1, 0);
    const [gate, oneshot] = sources();
    s.release(0, 0.001);
    s.release(1, 0.001);
    expect(envOf(gate).gain.cancelAndHoldAtTime).toHaveBeenCalledWith(0.001);
    expect(envOf(oneshot).gain.cancelAndHoldAtTime).not.toHaveBeenCalled();
    expect(gate.stop).toHaveBeenCalledTimes(1);
  });

  it('release cancels gate voices scheduled after it (note-repeat look-ahead)', async () => {
    const kit = createFactoryKit();
    kit.pads[0].mode = 'gate';
    const s = await readyKit(kit);
    s.trigger(0, 1, 0.2);
    const future = sources()[0];
    s.release(0, 0.1);
    expect(future.disposed).toBe(true);
  });

  it('stopAll cancels voices that have not started yet', async () => {
    const s = await readyKit();
    s.trigger(0, 1, 0);
    s.trigger(1, 1, 5);
    const [now, later] = sources();
    s.stopAll(0.001);
    expect(envOf(now).gain.cancelAndHoldAtTime).toHaveBeenCalledWith(0.001);
    expect(later.disposed).toBe(true);
  });

  it('offline renders (WAV export) keep voice nodes: Tone fires onended/timeouts before rendering', async () => {
    class FakeOfflineAudioContext {}
    vi.stubGlobal('OfflineAudioContext', FakeOfflineAudioContext);
    // The mock's raw context becomes an "OfflineAudioContext" (keeps its own methods).
    const raw = (toneMock.context as unknown as { rawContext: object }).rawContext;
    const prevProto = Object.getPrototypeOf(raw);
    Object.setPrototypeOf(raw, FakeOfflineAudioContext.prototype);
    try {
      const s = await readyKit();
      s.trigger(0, 1, 0);
      s.trigger(1, 1, 5);
      const [played, future] = sources();
      // Tone's offline clock runs the source's end callback before any audio is rendered.
      (played as unknown as { onended: () => void }).onended();
      expect(played.disposed).toBe(false);
      expect(envOf(played).disposed).toBe(false);
      // A cancelled future voice is muted instead of disposed.
      s.stopAll(0.001);
      expect(future.disposed).toBe(false);
      expect(envOf(future).gain.setValueAtTime).toHaveBeenLastCalledWith(0, 0);
    } finally {
      Object.setPrototypeOf(raw, prevProto);
      vi.unstubAllGlobals();
    }
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
