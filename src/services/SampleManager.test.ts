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

describe('SampleManager overlapping reloads (N3: latest request wins)', () => {
  /** Next sampleReadFile call resolves (or rejects) only when the returned controls say so. */
  const deferRead = () => {
    let resolve!: (b: ArrayBuffer) => void;
    let reject!: (e: unknown) => void;
    read.mockImplementationOnce(() => new Promise<ArrayBuffer>((res, rej) => ((resolve = res), (reject = rej))));
    return { resolve: () => resolve(new ArrayBuffer(8)), reject: () => reject(new Error('ENOENT')) };
  };
  const flush = () => new Promise((r) => setTimeout(r, 0));

  it('the older reload finishing first does not block the newer one: newest audio kept', async () => {
    const ref = await sampleManager.loadFromPath(FILE);
    const v1 = createFakeAudioBuffer(1, 100, 44100);
    const v2 = createFakeAudioBuffer(1, 200, 44100);
    const first = deferRead();
    const second = deferRead();
    decode.mockImplementationOnce(async () => v1).mockImplementationOnce(async () => v2);
    const r1 = sampleManager.reloadByPathPrefix(`${PACKS}/uzu-drumkit/`);
    const r2 = sampleManager.reloadByPathPrefix(`${PACKS}/uzu-drumkit/`);
    first.resolve();
    await flush();
    second.resolve();
    expect(await r1).toEqual([]); // superseded before it could apply
    expect(await r2).toEqual([ref.id]);
    expect(sampleManager.getBuffer(ref.id)).toBe(v2);
  });

  it('the older reload finishing LAST never overwrites the newer audio', async () => {
    const ref = await sampleManager.loadFromPath(FILE);
    const v1 = createFakeAudioBuffer(1, 100, 44100);
    const v2 = createFakeAudioBuffer(1, 200, 44100);
    const first = deferRead();
    const second = deferRead();
    const seen: string[][] = [];
    const off = sampleManager.onSamplesReloaded((ids) => seen.push(ids));
    const r1 = sampleManager.reloadByPathPrefix(`${PACKS}/uzu-drumkit/`);
    const r2 = sampleManager.reloadByPathPrefix(`${PACKS}/uzu-drumkit/`);
    decode.mockImplementationOnce(async () => v2);
    second.resolve();
    expect(await r2).toEqual([ref.id]);
    decode.mockImplementationOnce(async () => v1);
    first.resolve();
    expect(await r1).toEqual([]);
    off();
    expect(sampleManager.getBuffer(ref.id)).toBe(v2);
    expect(seen).toEqual([[ref.id]]);
  });

  it('a stale failure does not flag the sample unavailable; a stale success does not clear the flag', async () => {
    const ref = await sampleManager.loadFromPath(FILE);
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    // Older read fails late (pack being replaced), newer one succeeded: available.
    let first = deferRead();
    let second = deferRead();
    let r1 = sampleManager.reloadByPathPrefix(`${PACKS}/uzu-drumkit/`);
    let r2 = sampleManager.reloadByPathPrefix(`${PACKS}/uzu-drumkit/`);
    second.resolve();
    await r2;
    first.reject();
    await r1;
    expect(sampleManager.isUnavailable(ref.id)).toBe(false);
    // Older read succeeds late, newer one failed (pack removed): unavailable.
    first = deferRead();
    second = deferRead();
    r1 = sampleManager.reloadByPathPrefix(`${PACKS}/uzu-drumkit/`);
    r2 = sampleManager.reloadByPathPrefix(`${PACKS}/uzu-drumkit/`);
    second.reject();
    await r2;
    first.resolve();
    await r1;
    expect(sampleManager.isUnavailable(ref.id)).toBe(true);
  });

  it('a sample removed while reloading is not resurrected', async () => {
    const ref = await sampleManager.loadFromPath(FILE);
    const pending = deferRead();
    const r = sampleManager.reloadByPathPrefix(`${PACKS}/uzu-drumkit/`);
    sampleManager.removeSample(ref.id);
    pending.resolve();
    expect(await r).toEqual([]);
    expect(sampleManager.getSample(ref.id)).toBeUndefined();
    expect(sampleManager.getBuffer(ref.id)).toBeUndefined();
  });
});

describe('SampleManager change signal (N5)', () => {
  it('reports reloads and availability changes, with a revision that changes each time', async () => {
    const ref = await sampleManager.loadFromPath(FILE);
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const changed: string[][] = [];
    const off = sampleManager.onSamplesChanged((ids) => changed.push(ids));
    const r0 = sampleManager.getRevision(ref.id);
    await sampleManager.reloadByPathPrefix(`${PACKS}/uzu-drumkit/`);
    const r1 = sampleManager.getRevision(ref.id);
    expect(r1).not.toBe(r0);
    read.mockRejectedValueOnce(new Error('ENOENT'));
    await sampleManager.reloadByPathPrefix(`${PACKS}/uzu-drumkit/`);
    const r2 = sampleManager.getRevision(ref.id);
    expect(r2).not.toBe(r1);
    expect(sampleManager.getUnavailableIds()).toEqual([ref.id]);
    // Still gone: nothing changed, no signal.
    read.mockRejectedValueOnce(new Error('ENOENT'));
    await sampleManager.reloadByPathPrefix(`${PACKS}/uzu-drumkit/`);
    off();
    expect(sampleManager.getRevision(ref.id)).toBe(r2);
    expect(changed).toEqual([[ref.id], [ref.id]]);
  });
});
