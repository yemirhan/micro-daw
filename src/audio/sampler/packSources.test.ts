/**
 * Pack sample sources (P1) and pack cache invalidation (P2):
 * `{ kind: 'pack', packId, file }` resolves against the packs folder, old
 * absolute paths into the packs folder migrate on load, a missing pack fails
 * with MissingPackSampleError, and invalidatePackBuffers drops cached audio
 * and changes the cache key so live samplers reload.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ElectronAPI } from '@/types/electron';
import type { PadSampleSource } from '@/types/sampler';
import { normalizeKit, normalizePad, setPacksDirForTests } from './factoryKit';
import {
  clearPadBufferCache,
  getCachedPadBuffer,
  invalidatePackBuffers,
  loadPadBuffer,
  loadPreviewBuffer,
  MissingPackSampleError,
  onPadBuffersInvalidated,
  packIdOfSource,
  padSourceKey,
} from './bufferCache';
import { sampleManager } from '@/services/SampleManager';
import { SamplerKit } from './SamplerKit';
import { createFactoryKit } from './factoryKit';

const PACKS = '/data/packs';
const kick: PadSampleSource = { kind: 'pack', packId: 'tr808-fischer', file: 'bass-drum/bd0010.wav' };

let read: ReturnType<typeof vi.fn>;

beforeEach(() => {
  setPacksDirForTests(PACKS);
  clearPadBufferCache();
  read = vi.fn(async (_p: string) => new ArrayBuffer(16));
  window.electronAPI = { sampleReadFile: read } as unknown as ElectronAPI;
});

afterEach(() => {
  setPacksDirForTests(undefined);
  clearPadBufferCache();
  sampleManager.dispose();
  delete window.electronAPI;
});

describe('normalizePad migration (P1)', () => {
  it('keeps pack sources and validates them', () => {
    expect(normalizePad({ sample: { source: kick, name: 'Kick' } }).sample).toEqual({ source: kick, name: 'Kick' });
    expect(normalizePad({ sample: { source: { kind: 'pack', packId: '../x', file: 'a.wav' } } }).sample).toBeNull();
    expect(normalizePad({ sample: { source: { kind: 'pack', packId: 'p', file: '../../a.wav' } } }).sample).toBeNull();
    expect(normalizePad({ sample: { source: { kind: 'pack', packId: 'p', file: 'kicks/bd.wav' } } }).sample?.name).toBe('bd');
  });

  it('migrates absolute paths inside the packs folder to pack sources, leaves others alone', () => {
    const kit = normalizeKit({
      id: 'k',
      name: 'K',
      pads: [
        { sample: { source: { kind: 'file', path: `${PACKS}/tr808-fischer/bass-drum/bd0010.wav`, sampleId: 'stale' }, name: 'Kick' } },
        { sample: { source: { kind: 'file', path: '/Users/me/Samples/snare.wav' }, name: 'Snare' } },
      ],
    });
    expect(kit.pads[0].sample).toEqual({ source: kick, name: 'Kick' });
    expect(kit.pads[1].sample?.source).toEqual({ kind: 'file', path: '/Users/me/Samples/snare.wav' });
  });

  it('does not migrate when the packs folder is unknown (plain browser)', () => {
    setPacksDirForTests(null);
    const pad = normalizePad({ sample: { source: { kind: 'file', path: `${PACKS}/p/a.wav` }, name: 'a' } });
    expect(pad.sample?.source.kind).toBe('file');
  });
});

describe('loading pack sounds', () => {
  it('resolves against the packs folder and caches the buffer', async () => {
    const buf = await loadPadBuffer(kick);
    expect(read).toHaveBeenCalledWith(`${PACKS}/tr808-fischer/bass-drum/bd0010.wav`);
    expect(getCachedPadBuffer(kick)).toBe(buf);
    await loadPadBuffer(kick);
    expect(read).toHaveBeenCalledTimes(1);
    expect(packIdOfSource(kick)).toBe('tr808-fischer');
    expect(packIdOfSource({ kind: 'file', path: `${PACKS}/uzu-drumkit/kick/a.wav` })).toBe('uzu-drumkit');
  });

  it('a missing pack fails with MissingPackSampleError (pad shows "Pack not installed")', async () => {
    read.mockRejectedValueOnce(new Error('ENOENT: no such file or directory'));
    const err = await loadPadBuffer(kick).catch((e: unknown) => e);
    expect(err).toBeInstanceOf(MissingPackSampleError);
    expect(err).toMatchObject({ packId: 'tr808-fischer', file: 'bass-drum/bd0010.wav' });
    setPacksDirForTests(null);
    await expect(loadPadBuffer(kick)).rejects.toBeInstanceOf(MissingPackSampleError);
  });

  it('previews pack sounds without registering them with SampleManager', async () => {
    await loadPreviewBuffer(kick);
    expect(read).toHaveBeenCalledWith(`${PACKS}/tr808-fischer/bass-drum/bd0010.wav`);
    expect(sampleManager.getAllSamples()).toHaveLength(0);
  });
});

describe('invalidatePackBuffers (P2)', () => {
  it('drops cached audio of that pack only, changes its key and notifies listeners', async () => {
    const other: PadSampleSource = { kind: 'pack', packId: 'uzu-drumkit', file: 'kick/a.wav' };
    const legacy: PadSampleSource = { kind: 'file', path: `${PACKS}/tr808-fischer/snare/sd0000.wav` };
    await Promise.all([loadPadBuffer(kick), loadPadBuffer(other), loadPadBuffer(legacy)]);
    expect(sampleManager.getAllSamples().map((s) => s.path)).toEqual([`${PACKS}/tr808-fischer/snare/sd0000.wav`]);
    const keyBefore = padSourceKey(kick);
    const legacyKeyBefore = padSourceKey(legacy);
    const seen: string[] = [];
    const off = onPadBuffersInvalidated((id) => seen.push(id));

    invalidatePackBuffers('tr808-fischer');
    off();

    expect(seen).toEqual(['tr808-fischer']);
    expect(padSourceKey(kick)).not.toBe(keyBefore);
    expect(padSourceKey(legacy)).not.toBe(legacyKeyBefore);
    expect(getCachedPadBuffer(kick)).toBeNull();
    expect(getCachedPadBuffer(legacy)).toBeNull();
    expect(getCachedPadBuffer(other)).not.toBeNull();
    expect(sampleManager.getAllSamples()).toHaveLength(0);

    await loadPadBuffer(kick);
    expect(read.mock.calls.filter(([p]) => p === `${PACKS}/tr808-fischer/bass-drum/bd0010.wav`)).toHaveLength(2);
  });

  it('a load that finishes after the invalidation is not cached under the new key', async () => {
    let finish!: (b: ArrayBuffer) => void;
    read.mockImplementationOnce(() => new Promise<ArrayBuffer>((r) => (finish = r)));
    const p = loadPadBuffer(kick);
    await Promise.resolve();
    invalidatePackBuffers('tr808-fischer');
    finish(new ArrayBuffer(16));
    await p;
    expect(getCachedPadBuffer(kick)).toBeNull();
  });

  it('a live SamplerKit reloads the pads of an invalidated pack (install/update), and goes silent on uninstall', async () => {
    const base = createFactoryKit();
    const pads = base.pads.map((p, i) => (i === 0 ? { ...p, sample: { source: kick, name: 'Kick' } } : { ...p, sample: null }));
    const kit = new SamplerKit({ ...base, pads });
    await kit.ready;
    const kickPath = `${PACKS}/tr808-fischer/bass-drum/bd0010.wav`;
    const reads = () => read.mock.calls.filter(([p]) => p === kickPath).length;
    expect(reads()).toBe(1);
    expect(kit.isPadReady(0)).toBe(true);

    // Update: same kit, new pack generation -> the pad re-reads the file without a setKit.
    invalidatePackBuffers('tr808-fischer');
    await kit.ready;
    expect(reads()).toBe(2);
    expect(kit.isPadReady(0)).toBe(true);

    // Uninstall: the reload fails -> the pad is silent (no stale audio keeps playing).
    read.mockImplementation(async () => {
      throw new Error('ENOENT');
    });
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    invalidatePackBuffers('tr808-fischer');
    await kit.ready;
    expect(kit.isPadReady(0)).toBe(false);

    // Reinstall: back to life.
    read.mockImplementation(async () => new ArrayBuffer(16));
    invalidatePackBuffers('tr808-fischer');
    await kit.ready;
    expect(kit.isPadReady(0)).toBe(true);

    // A disposed kit stops listening.
    kit.dispose();
    const before = read.mock.calls.length;
    invalidatePackBuffers('tr808-fischer');
    expect(read.mock.calls.length).toBe(before);
    warn.mockRestore();
  });

  it('setKit with an unchanged kit still reloads a pad whose pack buffer is stale', async () => {
    const base = createFactoryKit();
    const pads = base.pads.map((p, i) => (i === 0 ? { ...p, sample: { source: kick, name: 'Kick' } } : { ...p, sample: null }));
    const state = { ...base, pads };
    const kit = new SamplerKit(state);
    await kit.ready;
    const off = onPadBuffersInvalidated(() => undefined);
    invalidatePackBuffers('tr808-fischer');
    off();
    kit.setKit(state);
    await kit.ready;
    expect(read.mock.calls.filter(([p]) => String(p).endsWith('bd0010.wav')).length).toBe(2);
    kit.dispose();
  });
});
