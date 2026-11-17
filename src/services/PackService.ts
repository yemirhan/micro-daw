import * as Tone from 'tone';
import type { InstalledPack, PackCatalog, PackInstallState, PackManifest } from '@/types/packs';
import type { ElectronAPI, PackProgressEvent } from '@/types/electron';
import { PAD_COUNT, type KitState, type PadSettings } from '@/types/sampler';
import { defaultPadSettings, normalizePad } from '@/audio/sampler/factoryKit';
import { LruCache } from '@/audio/sampler/bufferCache';
import { ensureMasterBus } from '@/audio/masterBus';
import { basename, displayName, joinPath, separatorOf } from '@/components/browser/browserHelpers';
import { isSafePackPath } from '@/main/packs/validate';
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
 * A 16-pad KitState for one kit of an installed pack: file sources with
 * absolute paths, pad settings from the manifest (clamped by normalizePad).
 * Unused pads are empty. Null when the pack has no kit `kitId`.
 */
export function kitFromPack(installed: InstalledPack, kitId: string): KitState | null {
  const kit = installed.manifest.kits.find((k) => k.id === kitId);
  if (!kit) return null;
  const files = new Set(installed.manifest.files.map((f) => f.path));
  const pads: PadSettings[] = Array.from({ length: PAD_COUNT }, defaultPadSettings);
  for (const p of kit.pads) {
    if (!Number.isInteger(p.pad) || p.pad < 0 || p.pad >= PAD_COUNT) continue;
    if (!files.has(p.file) || !isSafePackPath(p.file)) continue;
    const path = packFilePath(installed.dir, p.file);
    pads[p.pad] = normalizePad({
      ...defaultPadSettings(),
      ...(p.settings ?? {}),
      sample: { source: { kind: 'file', path }, name: p.name || displayName(basename(p.file)) },
    });
  }
  return { id: packKitId(installed.id, kit.id), name: kit.name, pads };
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
  /** URL (remote) or path (local) of the sound being previewed. */
  previewing: string | null;
}

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
  };
  private listeners = new Set<() => void>();
  private initPromise: Promise<void> | null = null;
  private catalogPromise: Promise<PackCatalog | null> | null = null;
  private unsubscribeProgress: (() => void) | null = null;
  private decoded = new LruCache<string, AudioBuffer>(8);
  private previewVolume: Tone.Volume | null = null;
  private previewSource: Tone.ToneBufferSource | null = null;
  private previewToken = 0;

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
      this.set({ installed });
      return installed;
    } catch (err) {
      console.warn('[packs] could not list installed packs', err);
      return this.snap.installed;
    }
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

  async uninstall(id: string): Promise<void> {
    const a = api();
    if (!a) return;
    await a.packsUninstall(id);
    this.setInstallState(id, null);
    await this.refreshInstalled();
  }

  /** Clears a failed install's error (back to idle/installed). */
  dismissError(id: string): void {
    if (this.snap.installStates[id]?.state === 'error') this.setInstallState(id, null);
  }

  // --- Previews -------------------------------------------------------------

  /** Downloads (via main, allowlisted + cached), decodes and plays a remote pack file. */
  async previewRemote(url: string): Promise<void> {
    const a = api();
    if (!a) return;
    this.stopPreview();
    const token = this.previewToken;
    this.set({ previewing: url });
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
      this.play(buffer, url);
    } catch (err) {
      console.warn('[packs] preview failed', err);
      if (token === this.previewToken) this.set({ previewing: null });
      throw err;
    }
  }

  /**
   * Plays an installed (local) pack file through the sample-browser preview
   * player (drumEngine). Not reflected in `previewing` (no end event there).
   */
  async previewFile(path: string): Promise<void> {
    this.stopPreview();
    await drumEngine.preview({ kind: 'file', path });
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
    this.snap = {
      available: false,
      catalog: null,
      catalogStatus: 'idle',
      catalogError: null,
      installed: [],
      installStates: {},
      packsDir: null,
      previewing: null,
    };
  }
}

export const packService = new PackService();
