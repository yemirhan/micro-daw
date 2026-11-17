import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ElectronAPI, PackProgressEvent } from '@/types/electron';
import type { InstalledPack, PackManifest } from '@/types/packs';
import { manifest } from '@/main/packs/testUtils';
import { createFakeAudioBuffer, instancesOf } from '@/test/toneMock';

const preview = vi.fn((_src: unknown) => Promise.resolve());
const stopPreview = vi.fn();
vi.mock('@/services/DrumEngine', () => ({ drumEngine: { preview: (s: unknown) => preview(s), stopPreview: () => stopPreview() } }));
const loadPreviewBuffer = vi.fn(async (_src: unknown) => createFakeAudioBuffer(1, 4410, 44100) as unknown as AudioBuffer);

const invalidatePackBuffers = vi.fn((_id: string) => undefined);
vi.mock('@/audio/sampler/bufferCache', async (orig) => ({
  ...(await orig<typeof import('@/audio/sampler/bufferCache')>()),
  loadPreviewBuffer: (s: unknown) => loadPreviewBuffer(s),
  invalidatePackBuffers: (id: string) => invalidatePackBuffers(id),
}));

import { ipcErrorMessage, kitFromPack, packFileLabel, packFilePath, packIdsUsedByKits, packKitId, packService } from './PackService';
import { setPacksDirForTests } from '@/audio/sampler/factoryKit';
import type { KitState } from '@/types/sampler';

function installed(m: PackManifest = manifest(), dir = '/data/packs/test-pack'): InstalledPack {
  return { id: m.id, version: m.version, dir, installedAt: '2026-11-17T00:00:00Z', manifest: m };
}

describe('kitFromPack', () => {
  it('builds a 16-pad kit with portable pack sources (P1) and clamped settings', () => {
    const m = manifest();
    m.kits[0].pads[0].settings = { chokeGroup: 1, gainDb: 99, mode: 'gate' };
    const kit = kitFromPack(installed(m), 'main')!;
    expect(kit.id).toBe(packKitId('test-pack', 'main'));
    expect(kit.name).toBe('Main');
    expect(kit.pads).toHaveLength(16);
    expect(kit.pads[0].sample).toEqual({ source: { kind: 'pack', packId: 'test-pack', file: 'kicks/bd.wav' }, name: 'Kick' });
    expect(JSON.stringify(kit)).not.toContain('/data/packs');
    expect(kit.pads[0]).toMatchObject({ chokeGroup: 1, gainDb: 12, mode: 'gate' });
    expect(kit.pads[1].sample?.name).toBe('sd');
    expect(kit.pads[2].sample).toBeNull();
  });

  it('uses Windows separators for Windows pack dirs and returns null for unknown kits', () => {
    expect(packFilePath('C:\\Users\\me\\packs\\p', 'kicks/bd.wav')).toBe('C:\\Users\\me\\packs\\p\\kicks\\bd.wav');
    expect(kitFromPack(installed(), 'nope')).toBeNull();
  });

  it('skips pads pointing at unsafe or unknown files (tampered pack.json)', () => {
    const m = manifest();
    m.kits[0].pads.push({ pad: 5, file: '../../etc/passwd.wav' }, { pad: 6, file: 'missing.wav' });
    const kit = kitFromPack(installed(m), 'main')!;
    expect(kit.pads[5].sample).toBeNull();
    expect(kit.pads[6].sample).toBeNull();
  });

  it('names unnamed pads from the file label (P4), works from a catalog manifest', () => {
    const m = manifest();
    m.files[1] = { ...m.files[1], label: 'Snare · tone 0 · snappy 10' };
    const kit = kitFromPack({ id: m.id, manifest: m }, 'main')!;
    expect(kit.pads[1].sample?.name).toBe('Snare · tone 0 · snappy 10');
    expect(packFileLabel({ path: 'bass-drum/bd0010.wav' })).toBe('bd0010');
  });

  it('finds the packs a project uses, incl. legacy absolute paths', () => {
    const kit = kitFromPack(installed(), 'main')!;
    const legacy: KitState = { ...kit, pads: kit.pads.map((p, i) => (i === 0 ? { ...p, sample: { source: { kind: 'file', path: '/data/packs/old-pack/a.wav' }, name: 'a' } } : p)) };
    expect([...packIdsUsedByKits([kit, null, legacy], '/data/packs')].sort()).toEqual(['old-pack', 'test-pack']);
    expect(packIdsUsedByKits([legacy], null).has('old-pack')).toBe(false);
  });

  it('cleans IPC error messages', () => {
    expect(ipcErrorMessage(new Error("Error invoking remote method 'packs:install': Error: HTTP 404"))).toBe('HTTP 404');
  });
});

describe('packService', () => {
  let progress: ((e: PackProgressEvent) => void) | null;
  let api: Partial<ElectronAPI>;
  let installedList: InstalledPack[];

  beforeEach(() => {
    packService.resetForTests();
    progress = null;
    installedList = [];
    api = {
      packsDir: vi.fn(async () => '/data/packs'),
      packsActiveStates: vi.fn(async () => ({})),
      packsListInstalled: vi.fn(async () => installedList),
      packsFetchCatalog: vi.fn(async () => ({ version: 1 as const, updatedAt: 'x', packs: [manifest()] })),
      packsInstall: vi.fn(),
      packsCancel: vi.fn(async () => true),
      packsUninstall: vi.fn(async () => undefined),
      packsFetchPreview: vi.fn(async () => new ArrayBuffer(8)),
      onPacksProgress: vi.fn((cb: (e: PackProgressEvent) => void) => {
        progress = cb;
        return () => undefined;
      }),
    };
    window.electronAPI = api as ElectronAPI;
  });

  afterEach(() => {
    packService.resetForTests();
    setPacksDirForTests(undefined);
    invalidatePackBuffers.mockClear();
    delete window.electronAPI;
  });

  it('is unavailable without (or with a partial) Electron API', async () => {
    delete window.electronAPI;
    await packService.init();
    expect(packService.getSnapshot().available).toBe(false);
    window.electronAPI = { fsListDir: vi.fn() } as unknown as ElectronAPI;
    await packService.init();
    expect(packService.getSnapshot().available).toBe(false);
  });

  it('loads dir, installed packs and the catalog', async () => {
    installedList = [installed()];
    await packService.init();
    await packService.loadCatalog();
    const s = packService.getSnapshot();
    expect(s).toMatchObject({ available: true, packsDir: '/data/packs', catalogStatus: 'ready' });
    expect(s.catalog?.packs).toHaveLength(1);
    expect(packService.isInstalled('test-pack')).toBe(true);
    expect(packService.installState('test-pack')).toEqual({ state: 'installed' });
  });

  it('tracks install progress and completion', async () => {
    await packService.init();
    let resolveInstall!: (p: InstalledPack) => void;
    vi.mocked(api.packsInstall!).mockImplementation(() => new Promise((r) => (resolveInstall = r)));
    const listener = vi.fn();
    const unsub = packService.subscribe(listener);
    const p = packService.install('test-pack');
    expect(packService.installState('test-pack').state).toBe('downloading');
    progress?.({ id: 'test-pack', state: { state: 'downloading', receivedBytes: 5, totalBytes: 10, fileIndex: 1, fileCount: 2 } });
    expect(packService.installState('test-pack')).toMatchObject({ receivedBytes: 5, fileIndex: 1 });
    installedList = [installed()];
    resolveInstall(installed());
    await p;
    expect(packService.installState('test-pack')).toEqual({ state: 'installed' });
    expect(listener).toHaveBeenCalled();
    unsub();
  });

  it('reports install errors and treats cancel as idle', async () => {
    await packService.init();
    vi.mocked(api.packsInstall!).mockRejectedValueOnce(new Error("Error invoking remote method 'packs:install': Error: Checksum mismatch"));
    expect(await packService.install('test-pack')).toBeNull();
    expect(packService.installState('test-pack')).toEqual({ state: 'error', message: 'Checksum mismatch' });
    packService.dismissError('test-pack');
    expect(packService.installState('test-pack')).toEqual({ state: 'idle' });

    vi.mocked(api.packsInstall!).mockRejectedValueOnce(new Error('Install cancelled'));
    await packService.install('test-pack');
    expect(packService.installState('test-pack')).toEqual({ state: 'idle' });
  });

  it('previews remote files through main and local files through the drum engine', async () => {
    await packService.init();
    await packService.previewRemote('https://raw.githubusercontent.com/o/r/sha/bd.wav');
    expect(api.packsFetchPreview).toHaveBeenCalledWith('https://raw.githubusercontent.com/o/r/sha/bd.wav');
    expect(packService.getSnapshot().previewing).toBe('https://raw.githubusercontent.com/o/r/sha/bd.wav');
    // Cached decode: no second fetch.
    await packService.previewRemote('https://raw.githubusercontent.com/o/r/sha/bd.wav');
    expect(api.packsFetchPreview).toHaveBeenCalledTimes(1);
    packService.stopPreview();
    expect(packService.getSnapshot().previewing).toBeNull();
    await packService.previewFile('/data/packs/test-pack/sd.wav');
    expect(loadPreviewBuffer).toHaveBeenCalledWith({ kind: 'file', path: '/data/packs/test-pack/sd.wav' });
    expect(packService.getSnapshot().previewing).toBe('/data/packs/test-pack/sd.wav');
  });

  it('"now playing" follows the real playback: cleared when the sound ends', async () => {
    await packService.init();
    await packService.previewSound({ kind: 'classic', machine: 'tr808', voice: 'snare' }, 'builtin:tr808::snare');
    expect(packService.getSnapshot().previewing).toBe('builtin:tr808::snare');
    const src = instancesOf('ToneBufferSource').at(-1)!;
    const opts = src.__args[0] as { onended: () => void };
    opts.onended();
    expect(packService.getSnapshot().previewing).toBeNull();
    // A stale end event (an older sound) never clears a newer preview.
    await packService.previewFile('/data/packs/test-pack/sd.wav', 'k2');
    opts.onended();
    expect(packService.getSnapshot().previewing).toBe('k2');
  });

  it('P2: drops cached audio of a pack when it is installed, updated or removed', async () => {
    installedList = [installed()];
    await packService.init();
    expect(invalidatePackBuffers).not.toHaveBeenCalled(); // first listing: nothing cached yet
    installedList = [{ ...installed(manifest({ version: 2 })), version: 2, installedAt: '2026-11-18T00:00:00Z' }];
    progress?.({ id: 'test-pack', state: { state: 'installed' } });
    await vi.waitFor(() => expect(invalidatePackBuffers).toHaveBeenCalledWith('test-pack'));
    invalidatePackBuffers.mockClear();
    await packService.refreshInstalled(); // unchanged: no invalidation
    expect(invalidatePackBuffers).not.toHaveBeenCalled();
    installedList = [];
    await packService.uninstall('test-pack');
    expect(invalidatePackBuffers).toHaveBeenCalledWith('test-pack');
  });

  it('P1: usage by the open project and missing-pack status', async () => {
    setPacksDirForTests('/data/packs');
    const kit = kitFromPack(installed(), 'main')!;
    expect(packService.packStatus('test-pack')).toBe('unknown'); // installed list not loaded yet
    expect(packService.missingPacks([kit])).toEqual([]);
    await packService.init();
    packService.setProjectKitsProvider(() => [kit]);
    expect(packService.isUsedByProject('test-pack')).toBe(true);
    expect(packService.isUsedByProject('other')).toBe(false);
    expect(packService.packStatus('test-pack')).toBe('missing');
    expect(packService.missingPackOf(kit.pads[0].sample?.source)).toBe('test-pack');
    expect(packService.missingPackOf({ kind: 'factory', voice: 'kick' })).toBeNull();
    expect(packService.missingPacks()).toEqual(['test-pack']);
    installedList = [installed()];
    await packService.refreshInstalled();
    expect(packService.packStatus('test-pack')).toBe('installed');
    expect(packService.missingPacks()).toEqual([]);
    expect(packService.missingPackOf(kit.pads[0].sample?.source)).toBeNull();
  });

  it('previews an installed pack sound by pack id + path', async () => {
    await packService.init();
    await packService.previewPackFile('test-pack', 'sd.wav');
    expect(loadPreviewBuffer).toHaveBeenCalledWith({ kind: 'pack', packId: 'test-pack', file: 'sd.wav' });
    expect(packService.getSnapshot().previewing).toBe('test-pack/sd.wav');
  });

  it('keeps an uninstall failure for the UI and clears it on the next attempt', async () => {
    await packService.init();
    vi.mocked(api.packsUninstall!).mockRejectedValueOnce(new Error("Error invoking remote method 'packs:uninstall': Error: EBUSY: resource busy"));
    await expect(packService.uninstall('test-pack')).rejects.toThrow();
    expect(packService.getSnapshot().uninstallErrors).toEqual({ 'test-pack': 'EBUSY: resource busy' });
    await packService.uninstall('test-pack');
    expect(packService.getSnapshot().uninstallErrors).toEqual({});
    vi.mocked(api.packsUninstall!).mockRejectedValueOnce(new Error('nope'));
    await expect(packService.uninstall('test-pack')).rejects.toThrow();
    packService.dismissError('test-pack');
    expect(packService.getSnapshot().uninstallErrors).toEqual({});
  });
});
