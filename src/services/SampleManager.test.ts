/**
 * B3: a pack change reloads SampleManager samples in place (same sampleId), so
 * Song audio regions keep playing (and exporting) instead of going silent.
 */
import * as Tone from 'tone';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ElectronAPI } from '@/types/electron';
import { createFakeAudioBuffer } from '@/test/toneMock';
import { sampleManager } from './SampleManager';
import { invalidatePackBuffers, clearPadBufferCache } from '@/audio/sampler/bufferCache';
import { setPacksDirForTests } from '@/audio/sampler/factoryKit';

const PACKS = '/data/packs';
const FILE = `${PACKS}/uzu-drumkit/kick/10_bd.wav`;
let read: ReturnType<typeof vi.fn>;
let decode: ReturnType<typeof vi.fn>;

beforeEach(() => {
  setPacksDirForTests(PACKS);
  read = vi.fn(async (_p: string) => new ArrayBuffer(8));
  window.electronAPI = { sampleReadFile: read } as unknown as ElectronAPI;
  const raw = Tone.getContext().rawContext as unknown as { decodeAudioData: ReturnType<typeof vi.fn> };
  decode = raw.decodeAudioData;
  decode.mockImplementation(async () => createFakeAudioBuffer(1, 44100, 44100));
});

afterEach(() => {
  sampleManager.dispose();
  clearPadBufferCache();
  setPacksDirForTests(undefined);
  delete window.electronAPI;
});

describe('SampleManager in-place reload (B3)', () => {
  it('a pack update keeps the sampleId and swaps in the new audio; listeners get the ids', async () => {
    const ref = await sampleManager.loadFromPath(FILE);
    const other = await sampleManager.loadFromPath('/Users/me/other.wav');
    const before = sampleManager.getBuffer(ref.id);
    const fresh = createFakeAudioBuffer(2, 88200, 44100);
    decode.mockImplementationOnce(async () => fresh);
    const seen: string[][] = [];
    const off = sampleManager.onSamplesReloaded((ids) => seen.push(ids));

    invalidatePackBuffers('uzu-drumkit');
    await sampleManager.whenReloaded(FILE);
    off();

    expect(sampleManager.getSample(ref.id)).toMatchObject({ id: ref.id, path: FILE, channels: 2, durationSeconds: 2 });
    expect(sampleManager.getBuffer(ref.id)).toBe(fresh);
    expect(sampleManager.getBuffer(ref.id)).not.toBe(before);
    expect(sampleManager.getToneBuffer(ref.id)).toBeDefined();
    expect(sampleManager.getSample(other.id)).toBeDefined(); // other samples untouched
    expect(seen).toEqual([[ref.id]]);
    expect(sampleManager.isUnavailable(ref.id)).toBe(false);
  });

  it('a removed pack keeps the loaded audio (never silently dropped) and marks it unavailable', async () => {
    const ref = await sampleManager.loadFromPath(FILE);
    const buf = sampleManager.getBuffer(ref.id);
    read.mockRejectedValueOnce(new Error('ENOENT'));
    const seen: string[][] = [];
    const off = sampleManager.onSamplesReloaded((ids) => seen.push(ids));
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);

    invalidatePackBuffers('uzu-drumkit');
    await sampleManager.whenReloaded(FILE);
    off();

    expect(sampleManager.getBuffer(ref.id)).toBe(buf);
    expect(sampleManager.getToneBuffer(ref.id)).toBeDefined();
    expect(sampleManager.isUnavailable(ref.id)).toBe(true);
    expect(seen).toEqual([]);
    expect(warn).toHaveBeenCalled();
  });

  it('a load of a reloading path waits for the reload and returns the same id', async () => {
    const ref = await sampleManager.loadFromPath(FILE);
    let finish!: (b: ArrayBuffer) => void;
    read.mockImplementationOnce(() => new Promise<ArrayBuffer>((r) => (finish = r)));
    const fresh = createFakeAudioBuffer(1, 22050, 44100);
    decode.mockImplementationOnce(async () => fresh);
    const reload = sampleManager.reloadByPathPrefix(`${PACKS}/uzu-drumkit/`);
    let loaded: string | null = null;
    const p = sampleManager.loadFromPath(FILE).then((r) => (loaded = r.id));
    await Promise.resolve();
    expect(loaded).toBeNull();
    finish(new ArrayBuffer(8));
    expect(await reload).toEqual([ref.id]);
    await p;
    expect(loaded).toBe(ref.id);
    expect(sampleManager.getBuffer(ref.id)).toBe(fresh);
  });
});
