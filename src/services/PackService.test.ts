import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ElectronAPI, PackProgressEvent } from '@/types/electron';
import type { InstalledPack, PackManifest } from '@/types/packs';
import { manifest } from '@/main/packs/testUtils';
import { createFakeAudioBuffer, instancesOf } from '@/test/toneMock';

const preview = vi.fn((_src: unknown) => Promise.resolve());
const stopPreview = vi.fn();
vi.mock('@/services/DrumEngine', () => ({
  drumEngine: { preview: (s: unknown) => preview(s), stopPreview: () => stopPreview(), setSink: () => undefined },
}));
const loadPreviewBuffer = vi.fn(async (_src: unknown) => createFakeAudioBuffer(1, 4410, 44100) as unknown as AudioBuffer);

const invalidatePackBuffers = vi.fn((_id: string) => undefined);
vi.mock('@/audio/sampler/bufferCache', async (orig) => ({
  ...(await orig<typeof import('@/audio/sampler/bufferCache')>()),
  loadPreviewBuffer: (s: unknown) => loadPreviewBuffer(s),
  invalidatePackBuffers: (id: string) => invalidatePackBuffers(id),
}));

import { ipcErrorMessage, kitFromPack, PACK_NAMES_STORAGE_KEY, packFileLabel, packFilePath, packIdsUsedByKits, packKitId, packService } from './PackService';
import { arrangementEngine } from './ArrangementEngine';
import { createFactoryKit } from '@/audio/sampler/factoryKit';
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
  describe('Get & load (M2)', () => {
    const other = () => manifest({ id: 'other-pack', name: 'Other' });
    function deferredInstalls() {
      const pending: Record<string, (p: InstalledPack) => void> = {};
      vi.mocked(api.packsInstall!).mockImplementation((id: string) => new Promise((r) => (pending[id] = r)));
      return pending;
    }

    it('the latest request wins, even when an older download finishes later or first', async () => {
      await packService.init();
      const applied: string[] = [];
      packService.setKitApplier((kit) => {
        applied.push(kit.id);
      });
      const loaded = vi.fn();
      packService.onKitLoaded(loaded);
      const pending = deferredInstalls();

      const a = packService.getAndLoad('test-pack', 'main');
      expect(packService.getSnapshot().pendingLoad).toBe('test-pack/main');
      const b = packService.getAndLoad('other-pack', 'main');
      expect(packService.getSnapshot().pendingLoad).toBe('other-pack/main');

      installedList = [installed()];
      pending['test-pack'](installed()); // the superseded one finishes first
      expect(await a).toBeNull();
      expect(applied).toEqual([]);
      expect(packService.getSnapshot().pendingLoad).toBe('other-pack/main');

      installedList = [installed(), installed(other(), '/data/packs/other-pack')];
      pending['other-pack'](installed(other(), '/data/packs/other-pack'));
      const kit = await b;
      expect(kit?.id).toBe('pack:other-pack:main');
      expect(applied).toEqual(['pack:other-pack:main']);
      expect(loaded).toHaveBeenCalledWith(expect.objectContaining({ packId: 'other-pack', kitId: 'main' }));
      expect(packService.getSnapshot().pendingLoad).toBeNull();
    });

    it('an older request finishing after the latest one never overrides it', async () => {
      await packService.init();
      const applied: string[] = [];
      packService.setKitApplier((kit) => {
        applied.push(kit.id);
      });
      const pending = deferredInstalls();
      const a = packService.getAndLoad('test-pack', 'main');
      const b = packService.getAndLoad('other-pack', 'main');
      pending['other-pack'](installed(other(), '/data/packs/other-pack'));
      await b;
      pending['test-pack'](installed());
      await a;
      expect(applied).toEqual(['pack:other-pack:main']);
    });

    it('applies at app level without any page mounted, and loads an installed pack at once', async () => {
      installedList = [installed()];
      await packService.init();
      const applied: string[] = [];
      packService.setKitApplier((kit) => {
        applied.push(kit.id);
      });
      expect((await packService.getAndLoad('test-pack', 'main'))?.id).toBe('pack:test-pack:main');
      expect(api.packsInstall).not.toHaveBeenCalled();
      expect(applied).toEqual(['pack:test-pack:main']);
    });

    it('cancel / failure clear the pending load and load nothing', async () => {
      await packService.init();
      const applier = vi.fn();
      packService.setKitApplier(applier);
      const pending = deferredInstalls();
      const a = packService.getAndLoad('test-pack', 'main');
      await packService.cancel('test-pack');
      expect(packService.getSnapshot().pendingLoad).toBeNull();
      pending['test-pack'](installed());
      expect(await a).toBeNull();

      vi.mocked(api.packsInstall!).mockRejectedValueOnce(new Error('HTTP 404'));
      expect(await packService.getAndLoad('test-pack', 'main')).toBeNull();
      expect(packService.getSnapshot().pendingLoad).toBeNull();
      expect(applier).not.toHaveBeenCalled();
    });
  });

  describe('Get & load intent (N2)', () => {
    const engine = arrangementEngine as unknown as {
      getSessionId?: () => unknown;
      isLoopRecording: () => boolean;
      emitStateChange: () => void;
    };
    const ownSession = Object.prototype.hasOwnProperty.call(engine, 'getSessionId');
    const savedSession = engine.getSessionId;
    let resolveInstall: ((p: InstalledPack) => void) | null = null;
    let drums: string;

    beforeEach(async () => {
      arrangementEngine.resetToStarter();
      drums = arrangementEngine.ensureDrumTrack();
      vi.mocked(api.packsInstall!).mockImplementation(() => new Promise((r) => (resolveInstall = r)));
      await packService.init();
      await packService.loadCatalog();
    });

    afterEach(() => {
      vi.restoreAllMocks();
      if (ownSession) engine.getSessionId = savedSession;
      else delete engine.getSessionId;
    });

    async function finishDownload() {
      installedList = [installed()];
      resolveInstall!(installed());
      await new Promise((r) => setTimeout(r, 0));
    }

    const kitId = () => arrangementEngine.getKit(arrangementEngine.getActiveDrumTrackId()!).id;

    it('loads on the drum track that was active at request time and shows the pack name', async () => {
      const loaded = vi.fn();
      packService.onKitLoaded(loaded);
      const run = packService.getAndLoad('test-pack', 'main');
      expect(packService.getSnapshot().pendingLoadInfo).toMatchObject({ packId: 'test-pack', kitId: 'main', packName: manifest().name, waitingForRecording: false });
      await finishDownload();
      expect((await run)?.id).toBe('pack:test-pack:main');
      expect(kitId()).toBe('pack:test-pack:main');
      expect(loaded).toHaveBeenCalledWith(expect.objectContaining({ trackId: drums }));
      expect(packService.getSnapshot().pendingLoadInfo).toBeNull();
    });

    it('a kit chosen on the target track meanwhile (any view) drops the request at once', async () => {
      const run = packService.getAndLoad('test-pack', 'main');
      arrangementEngine.setKit(drums, { ...createFactoryKit(), id: 'user-kit', name: 'Mine' });
      expect(packService.getSnapshot().pendingLoad).toBeNull(); // chip gone before the download ends
      await finishDownload();
      expect(await run).toBeNull();
      expect(kitId()).toBe('user-kit');
    });

    it('re-choosing the kit already loaded (any view, source "user") also drops it', async () => {
      const run = packService.getAndLoad('test-pack', 'main');
      arrangementEngine.setKit(drums, arrangementEngine.getKit(drums));
      expect(packService.getSnapshot().pendingLoad).toBeNull();
      await finishDownload();
      expect(await run).toBeNull();
    });

    it('a real project switch (new session) drops it', async () => {
      const run = packService.getAndLoad('test-pack', 'main');
      arrangementEngine.resetToStarter();
      expect(packService.getSnapshot().pendingLoad).toBeNull();
      await finishDownload();
      expect(await run).toBeNull();
      expect(arrangementEngine.getTracks().some((t) => t.instrument.kit?.id === 'pack:test-pack:main')).toBe(false);
    });

    it('pad edits on the target kit keep the request', async () => {
      const run = packService.getAndLoad('test-pack', 'main');
      arrangementEngine.updatePad(drums, 0, { gainDb: -3 });
      expect(packService.getSnapshot().pendingLoad).toBe('test-pack/main');
      await finishDownload();
      expect((await run)?.id).toBe('pack:test-pack:main');
    });

    it('switching the active drum track drops the request', async () => {
      const run = packService.getAndLoad('test-pack', 'main');
      const other = arrangementEngine.addTrack('drums').id;
      arrangementEngine.setActiveDrumTrack(other);
      expect(packService.getSnapshot().pendingLoad).toBeNull();
      await finishDownload();
      expect(await run).toBeNull();
      expect(arrangementEngine.getKit(drums).id).not.toBe('pack:test-pack:main');
      expect(arrangementEngine.getKit(other).id).not.toBe('pack:test-pack:main');
    });

    it('a new project session drops the request (even with the same track + kit ids)', async () => {
      let session = 1;
      engine.getSessionId = () => session;
      const before = kitId();
      const run = packService.getAndLoad('test-pack', 'main');
      session = 2;
      engine.emitStateChange();
      expect(packService.getSnapshot().pendingLoad).toBeNull();
      await finishDownload();
      expect(await run).toBeNull();
      expect(kitId()).toBe(before);
    });

    it('waits while loop recording and loads after the take stops', async () => {
      let recording = true;
      vi.spyOn(engine, 'isLoopRecording').mockImplementation(() => recording);
      const before = kitId();
      const run = packService.getAndLoad('test-pack', 'main');
      await finishDownload();
      expect(kitId()).toBe(before); // never swaps kit mid-take
      expect(packService.getSnapshot().pendingLoadInfo?.waitingForRecording).toBe(true);
      recording = false;
      engine.emitStateChange();
      expect((await run)?.id).toBe('pack:test-pack:main');
      expect(kitId()).toBe('pack:test-pack:main');
      expect(packService.getSnapshot().pendingLoad).toBeNull();
    });

    it('cancelling while it waits for the take to stop loads nothing', async () => {
      vi.spyOn(engine, 'isLoopRecording').mockReturnValue(true);
      const before = kitId();
      const run = packService.getAndLoad('test-pack', 'main');
      await finishDownload();
      packService.cancelPendingLoad();
      expect(await run).toBeNull();
      expect(kitId()).toBe(before);
    });
  });

  describe('pack names (N6)', () => {
    afterEach(() => localStorage.removeItem(PACK_NAMES_STORAGE_KEY));

    it('names withdrawn packs from packs/withdrawn.json', () => {
      expect(packService.packName('tr8-mckaudio')).toMatch(/TR-8 Recordings/);
      expect(packService.packName('never-heard-of')).toBe('never-heard-of');
    });

    it('remembers names seen in the catalog after the pack is withdrawn', async () => {
      api.packsFetchCatalog = vi.fn(async () => ({ version: 1 as const, updatedAt: 'x', packs: [manifest({ id: 'soon-gone', name: 'Soon Gone Kit' })] }));
      await packService.init();
      await packService.loadCatalog();
      expect(packService.packName('soon-gone')).toBe('Soon Gone Kit');
      packService.resetForTests(); // next app start: the newer catalog no longer lists it
      window.electronAPI = api as ElectronAPI;
      api.packsFetchCatalog = vi.fn(async () => ({ version: 1 as const, updatedAt: 'y', packs: [manifest()] }));
      await packService.init();
      await packService.loadCatalog();
      expect(packService.isUnavailable('soon-gone')).toBe(true);
      expect(packService.packName('soon-gone')).toBe('Soon Gone Kit');
    });
  });

  it('L-c: a pack the loaded catalog does not list is "unavailable", not "missing"', async () => {
    await packService.init();
    expect(packService.packStatus('gone-pack')).toBe('missing'); // catalog not loaded yet
    await packService.loadCatalog();
    expect(packService.packStatus('gone-pack')).toBe('unavailable');
    expect(packService.isUnavailable('gone-pack')).toBe(true);
    expect(packService.packStatus('test-pack')).toBe('missing'); // listed: can be downloaded
    installedList = [installed(manifest({ id: 'gone-pack' }), '/data/packs/gone-pack')];
    await packService.refreshInstalled();
    expect(packService.packStatus('gone-pack')).toBe('installed'); // installed copy still works
  });
});
