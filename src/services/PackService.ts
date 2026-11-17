import * as Tone from 'tone';
import type { InstalledPack, PackCatalog, PackInstallState, PackManifest } from '@/types/packs';
import type { ElectronAPI, PackProgressEvent } from '@/types/electron';
import { PAD_COUNT, type KitState, type PadSampleSource, type PadSettings } from '@/types/sampler';
import { currentPacksDir, defaultPadSettings, normalizePad } from '@/audio/sampler/factoryKit';
import { LruCache, invalidatePackBuffers, loadPreviewBuffer } from '@/audio/sampler/bufferCache';
import { ensureMasterBus } from '@/audio/masterBus';
import { basename, displayName, joinPath, separatorOf } from '@/shared/paths';
import { isSafePackPath } from '@/shared/packValidate';
import { packRefFromPath } from '@/shared/packSource';
import { drumEngine } from '@/services/DrumEngine';

// ---------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------

/** Stable KitState id of a pack kit. */
export function packKitId(packId: string, kitId: string): string {
  return `pack:${packId}:${kitId}`;
}

/** Absolute path of a pack file (`rel` is POSIX; converted for Windows-style pack dirs). */
export function packFilePath(dir: string, rel: string): string {
  return joinPath(dir, separatorOf(dir) === '\\' ? rel.replace(/\//g, '\\') : rel);
}

/**
 * A 16-pad KitState for one kit of a pack: `{ kind: 'pack', packId, file }`
 * sources (resolved against the packs folder when loaded, so the project
 * stays portable), pad settings from the manifest (clamped by normalizePad).
 * Unused pads are empty. Null when the pack has no kit `kitId`. Works for a
 * catalog manifest too (pass `{ id, manifest }`), e.g. to set up pads before
 * the pack is installed.
 */
export function kitFromPack(installed: Pick<InstalledPack, 'id' | 'manifest'>, kitId: string): KitState | null {
  const kit = installed.manifest.kits.find((k) => k.id === kitId);
  if (!kit) return null;
  const files = new Map(installed.manifest.files.map((f) => [f.path, f]));
  const pads: PadSettings[] = Array.from({ length: PAD_COUNT }, defaultPadSettings);
  for (const p of kit.pads) {
    if (!Number.isInteger(p.pad) || p.pad < 0 || p.pad >= PAD_COUNT) continue;
    const file = files.get(p.file);
    if (!file || !isSafePackPath(p.file)) continue;
    pads[p.pad] = normalizePad({
      ...defaultPadSettings(),
      ...(p.settings ?? {}),
      sample: { source: { kind: 'pack', packId: installed.id, file: p.file }, name: p.name || packFileLabel(file) },
    });
  }
  return { id: packKitId(installed.id, kit.id), name: kit.name, pads };
}

/** Display name of a pack file: its `label`, else the file name without extension. */
export function packFileLabel(file: { path: string; label?: string }): string {
  return file.label || displayName(basename(file.path));
}

/** Pack id a pad source belongs to (pack sources, and absolute paths into `packsDir`), or null. */
export function packIdOfPadSource(source: PadSampleSource | null | undefined, packsDir: string | null = currentPacksDir()): string | null {
  if (!source) return null;
  if (source.kind === 'pack') return source.packId;
  if (source.kind === 'file') return packRefFromPath(packsDir, source.path)?.packId ?? null;
  return null;
}

type KitLike = { pads: readonly { sample: { source: PadSampleSource } | null }[] } | null | undefined;

/** Ids of every pack the kits use (pack sources and legacy absolute paths into the packs folder). */
export function packIdsUsedByKits(kits: Iterable<KitLike>, packsDir: string | null = currentPacksDir()): Set<string> {
  const out = new Set<string>();
  for (const kit of kits) {
    for (const p of kit?.pads ?? []) {
      const id = packIdOfPadSource(p.sample?.source, packsDir);
      if (id) out.add(id);
    }
  }
  return out;
}

/** Strips Electron's "Error invoking remote method 'x': Error: " prefix. */
export function ipcErrorMessage(err: unknown): string {
  const msg = err instanceof Error ? err.message : String(err);
  return msg.replace(/^Error invoking remote method '[^']+': (\w*Error: )?/, '') || 'Unknown error';
}

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

export type CatalogStatus = 'idle' | 'loading' | 'ready' | 'error';

export interface PackServiceSnapshot {
  /** False outside Electron (plain browser dev / ui-snap without a mocked API). */
  available: boolean;
  catalog: PackCatalog | null;
  catalogStatus: CatalogStatus;
  catalogError: string | null;
  installed: InstalledPack[];
  /** Install states of packs that are downloading or failed (others: see `installState`). */
  installStates: Readonly<Record<string, PackInstallState>>;
  /** `<userData>/packs`, once known. */
  packsDir: string | null;
  /**
   * Key of the sound preview playing right now (the caller's key; default the
   * URL / path). Cleared when the sound ends or is stopped.
   */
  previewing: string | null;
  /** Last uninstall failure per pack id (cleared on the next attempt / dismiss). */
  uninstallErrors: Readonly<Record<string, string>>;
}

/**
 * Status of a pack referenced by a pad: 'installed', 'installing' (download
 * running), or 'missing' (not installed: the pad is silent until it is; show
 * "Pack not installed — Get", see `packService.install`). 'unknown' before the
 * installed list was loaded (or outside Electron).
 */
export type PackRefStatus = 'installed' | 'installing' | 'missing' | 'unknown';

const PREVIEW_GAIN = 0.9;
const PREVIEW_FADE = 0.01;

/** The Electron API when it has the pack calls (partial mocks in tests / ui-snap count as unavailable). */
function api(): ElectronAPI | undefined {
  const a = typeof window !== 'undefined' ? window.electronAPI : undefined;
  return a && typeof a.packsListInstalled === 'function' && typeof a.onPacksProgress === 'function' ? a : undefined;
}

/**
 * Renderer side of the pack store: catalog, installed packs and per-pack
 * install state (driven by `packs:progress` from main), plus sound previews.
 * Subscribe with `subscribe` / read with `getSnapshot` (useSyncExternalStore).
 */
class PackService {
  private snap: PackServiceSnapshot = {
    available: false,
    catalog: null,
    catalogStatus: 'idle',
    catalogError: null,
    installed: [],
    installStates: {},
    packsDir: null,
    previewing: null,
    uninstallErrors: {},
  };
  private listeners = new Set<() => void>();
  private initPromise: Promise<void> | null = null;
  private catalogPromise: Promise<PackCatalog | null> | null = null;
  private unsubscribeProgress: (() => void) | null = null;
  private decoded = new LruCache<string, AudioBuffer>(8);
  private previewVolume: Tone.Volume | null = null;
  private previewSource: Tone.ToneBufferSource | null = null;
  private previewToken = 0;
  /** `${id}@${installedAt}` of each installed pack at the last refresh (null before the first). */
  private installedStamps: Map<string, string> | null = null;
  private projectKits: (() => Iterable<KitLike>) | null = null;

  getSnapshot = (): PackServiceSnapshot => this.snap;

  subscribe = (cb: () => void): (() => void) => {
    this.listeners.add(cb);
    return () => {
      this.listeners.delete(cb);
    };
  };

  private set(patch: Partial<PackServiceSnapshot>): void {
    this.snap = { ...this.snap, ...patch };
    for (const cb of [...this.listeners]) cb();
  }

  private setInstallState(id: string, state: PackInstallState | null): void {
    const next = { ...this.snap.installStates };
    if (state === null) delete next[id];
    else next[id] = state;
    this.set({ installStates: next });
  }

  /** Subscribes to progress events and loads installed packs + packs dir. Idempotent. */
  init(): Promise<void> {
    if (this.initPromise) return this.initPromise;
    const a = api();
    if (!a) return Promise.resolve();
    this.set({ available: true });
    this.unsubscribeProgress = a.onPacksProgress((e) => this.onProgress(e));
    this.initPromise = (async () => {
      try {
        const [dir, active] = await Promise.all([a.packsDir(), a.packsActiveStates().catch(() => ({}))]);
        this.set({ packsDir: dir, installStates: { ...active, ...this.snap.installStates } });
      } catch (err) {
        console.warn('[packs] init failed', err);
      }
      await this.refreshInstalled();
    })();
    return this.initPromise;
  }

  private onProgress(e: PackProgressEvent): void {
    if (!e || typeof e.id !== 'string' || !e.state) return;
    switch (e.state.state) {
      case 'downloading':
      case 'error':
        this.setInstallState(e.id, e.state);
        break;
      case 'installed':
      case 'idle':
        this.setInstallState(e.id, null);
        void this.refreshInstalled();
        break;
    }
  }

  /** Loads the catalog once (remote with bundled fallback, see main). `force` refetches. */
  loadCatalog(opts: { force?: boolean } = {}): Promise<PackCatalog | null> {
    const a = api();
    if (!a) return Promise.resolve(null);
    if (this.catalogPromise && !opts.force) return this.catalogPromise;
    if (!opts.force && this.snap.catalog) return Promise.resolve(this.snap.catalog);
    this.set({ catalogStatus: 'loading', catalogError: null });
    const p = a
      .packsFetchCatalog()
      .then((catalog) => {
        this.set({ catalog, catalogStatus: 'ready' });
        return catalog;
      })
      .catch((err: unknown) => {
        this.set({ catalogStatus: 'error', catalogError: ipcErrorMessage(err) });
        return null;
      })
      .finally(() => {
        if (this.catalogPromise === p) this.catalogPromise = null;
      });
    this.catalogPromise = p;
    return p;
  }

  async refreshInstalled(): Promise<InstalledPack[]> {
    const a = api();
    if (!a) return [];
    try {
      const installed = await a.packsListInstalled();
      this.invalidateChanged(installed);
      this.set({ installed });
      return installed;
    } catch (err) {
      console.warn('[packs] could not list installed packs', err);
      return this.snap.installed;
    }
  }

  /**
   * P2: a pack that was installed, updated (new installedAt) or removed since
   * the last refresh gets its cached buffers dropped, and live samplers are
   * told to reload (bufferCache.onPadBuffersInvalidated).
   */
  private invalidateChanged(installed: readonly InstalledPack[]): void {
    const next = new Map(installed.map((p) => [p.id, `${p.version}@${p.installedAt}`]));
    const prev = this.installedStamps;
    this.installedStamps = next;
    if (!prev) return;
    const changed = new Set<string>();
    for (const [id, stamp] of next) if (prev.get(id) !== stamp) changed.add(id);
    for (const id of prev.keys()) if (!next.has(id)) changed.add(id);
    for (const id of changed) invalidatePackBuffers(id);
  }

  // --- Project usage / missing packs -----------------------------------------

  /**
   * Registers how to read the open project's kits (every drum track's kit and
   * the live pad kit); used by `isUsedByProject` / `missingPacks`.
   */
  setProjectKitsProvider(provider: (() => Iterable<KitLike>) | null): void {
    this.projectKits = provider;
  }

  /** The open project has pads that play sounds of pack `id` (warn before uninstalling it). */
  isUsedByProject(id: string, kits?: Iterable<KitLike>): boolean {
    const source = kits ?? this.projectKits?.();
    return !!source && packIdsUsedByKits(source).has(id);
  }

  /** Status of a pack referenced by a pad (see PackRefStatus). */
  packStatus(id: string): PackRefStatus {
    if (this.snap.installStates[id]?.state === 'downloading') return 'installing';
    if (this.isInstalled(id)) return 'installed';
    return this.installedStamps ? 'missing' : 'unknown';
  }

  /** Pack id of a pad source whose pack is not installed (pad shows "Pack not installed — Get"), else null. */
  missingPackOf(source: PadSampleSource | null | undefined): string | null {
    const id = packIdOfPadSource(source);
    return id && this.packStatus(id) === 'missing' ? id : null;
  }

  /** Packs the kits (default: the open project) use that are not installed. */
  missingPacks(kits?: Iterable<KitLike>): string[] {
    const source = kits ?? this.projectKits?.();
    if (!source || !this.installedStamps) return [];
    return [...packIdsUsedByKits(source)].filter((id) => !this.isInstalled(id)).sort();
  }

  isInstalled(id: string): boolean {
    return this.snap.installed.some((p) => p.id === id);
  }

  getInstalled(id: string): InstalledPack | null {
    return this.snap.installed.find((p) => p.id === id) ?? null;
  }

  /** Effective state: running/failed install, else installed/idle. */
  installState(id: string): PackInstallState {
    return this.snap.installStates[id] ?? (this.isInstalled(id) ? { state: 'installed' } : { state: 'idle' });
  }

  /** Installed copy is older than the catalog's manifest. */
  hasUpdate(manifest: PackManifest): boolean {
    const inst = this.getInstalled(manifest.id);
    return !!inst && inst.version < manifest.version;
  }

  /** Downloads + installs a catalog pack. Resolves with the installed pack, or null on error/cancel (see installState). */
  async install(id: string): Promise<InstalledPack | null> {
    const a = api();
    if (!a) return null;
    this.setInstallState(id, { state: 'downloading', receivedBytes: 0, totalBytes: null, fileIndex: 0, fileCount: 0 });
    try {
      const installed = await a.packsInstall(id);
      this.setInstallState(id, null);
      await this.refreshInstalled();
      return installed;
    } catch (err) {
      const message = ipcErrorMessage(err);
      if (/cancelled/i.test(message)) {
        if (this.snap.installStates[id]?.state === 'downloading') this.setInstallState(id, null);
      } else if (this.snap.installStates[id]?.state !== 'error') {
        this.setInstallState(id, { state: 'error', message });
      }
      return null;
    }
  }

  async cancel(id: string): Promise<void> {
    await api()?.packsCancel(id);
  }

  /**
   * Removes an installed pack. A failure is kept in `uninstallErrors[id]` (for
   * the UI) and rethrown.
   */
  async uninstall(id: string): Promise<void> {
    const a = api();
    if (!a) return;
    this.setUninstallError(id, null);
    try {
      await a.packsUninstall(id);
    } catch (err) {
      this.setUninstallError(id, ipcErrorMessage(err));
      throw err;
    }
    this.setInstallState(id, null);
    await this.refreshInstalled();
  }

  /** Clears a failed install's (or uninstall's) error. */
  dismissError(id: string): void {
    if (this.snap.installStates[id]?.state === 'error') this.setInstallState(id, null);
    this.setUninstallError(id, null);
  }

  private setUninstallError(id: string, message: string | null): void {
    if (message === null && !(id in this.snap.uninstallErrors)) return;
    const next = { ...this.snap.uninstallErrors };
    if (message === null) delete next[id];
    else next[id] = message;
    this.set({ uninstallErrors: next });
  }

  // --- Previews -------------------------------------------------------------

  /**
   * Downloads (via main, allowlisted + cached), decodes and plays a remote pack
   * file. `previewing` is `key` until the sound ends.
   */
  async previewRemote(url: string, key: string = url): Promise<void> {
    const a = api();
    if (!a) return;
    this.stopPreview();
    const token = this.previewToken;
    this.set({ previewing: key });
    try {
      let buffer = this.decoded.get(url);
      if (!buffer) {
        const data = await a.packsFetchPreview(url);
        if (token !== this.previewToken) return;
        const ctx = Tone.getContext().rawContext as BaseAudioContext;
        buffer = await ctx.decodeAudioData(data);
        this.decoded.set(url, buffer);
      }
      if (token !== this.previewToken) return;
      this.play(buffer, key);
    } catch (err) {
      console.warn('[packs] preview failed', err);
      if (token === this.previewToken) this.set({ previewing: null });
      throw err;
    }
  }

  /** Plays an installed (local) pack file; `previewing` is `key` until it ends. */
  previewFile(path: string, key: string = path): Promise<void> {
    return this.previewSound({ kind: 'file', path }, key);
  }

  /** Plays a sound of an installed pack by pack id + pack path; `previewing` is `key` until it ends. */
  previewPackFile(packId: string, file: string, key = `${packId}/${file}`): Promise<void> {
    return this.previewSound({ kind: 'pack', packId, file }, key);
  }

  /**
   * Plays any pad source (local file, classic voice) through the pack preview
   * player, so `previewing` follows the real playback: set while loading and
   * playing, cleared by the source's end event (or stop / the next preview).
   */
  async previewSound(source: PadSampleSource, key: string): Promise<void> {
    this.stopPreview();
    const token = this.previewToken;
    this.set({ previewing: key });
    try {
      const buffer = await loadPreviewBuffer(source);
      if (token !== this.previewToken) return;
      this.play(buffer, key);
    } catch (err) {
      console.warn('[packs] preview failed', err);
      if (token === this.previewToken) this.set({ previewing: null });
      throw err;
    }
  }

  private play(buffer: AudioBuffer, key: string): void {
    ensureMasterBus();
    this.previewVolume ??= new Tone.Volume(-6).toDestination();
    void Tone.start().catch(() => undefined);
    const src = new Tone.ToneBufferSource({
      url: buffer,
      fadeOut: PREVIEW_FADE,
      curve: 'linear',
      onended: () => {
        if (this.previewSource === src) {
          this.previewSource = null;
          if (this.snap.previewing === key) this.set({ previewing: null });
        }
      },
    }).connect(this.previewVolume);
    src.start(Tone.immediate(), 0, undefined, PREVIEW_GAIN);
    this.previewSource = src;
  }

  stopPreview(): void {
    this.previewToken++;
    drumEngine.stopPreview();
    const src = this.previewSource;
    this.previewSource = null;
    if (src) {
      try {
        src.stop(Tone.immediate());
      } catch {
        /* already ended */
      }
    }
    if (this.snap.previewing !== null) this.set({ previewing: null });
  }

  /** Tests only: forget all state. */
  resetForTests(): void {
    this.unsubscribeProgress?.();
    this.unsubscribeProgress = null;
    this.initPromise = null;
    this.catalogPromise = null;
    this.decoded.clear();
    this.previewSource = null;
    this.previewVolume = null;
    this.installedStamps = null;
    this.projectKits = null;
    this.snap = {
      available: false,
      catalog: null,
      catalogStatus: 'idle',
      catalogError: null,
      installed: [],
      installStates: {},
      packsDir: null,
      previewing: null,
      uninstallErrors: {},
    };
  }
}

export const packService = new PackService();
