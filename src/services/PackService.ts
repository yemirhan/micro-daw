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
import { arrangementEngine } from '@/services/ArrangementEngine';
import withdrawnPacks from '../../packs/withdrawn.json';

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

/** N6: display names of withdrawn packs (packs/withdrawn.json), for projects that still use them. */
const WITHDRAWN_PACK_NAMES: Readonly<Record<string, string>> = (withdrawnPacks as { packs: Record<string, string> }).packs;

/** localStorage key of the pack names this machine has seen (catalog + installed), N6. */
export const PACK_NAMES_STORAGE_KEY = 'micro-daw:pack-names';
const MAX_REMEMBERED_NAMES = 200;

function readRememberedNames(): Record<string, string> {
  try {
    const raw = typeof localStorage !== 'undefined' ? localStorage.getItem(PACK_NAMES_STORAGE_KEY) : null;
    const parsed: unknown = raw ? JSON.parse(raw) : null;
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return {};
    const out: Record<string, string> = {};
    for (const [k, v] of Object.entries(parsed)) if (typeof v === 'string' && v) out[k] = v;
    return out;
  } catch {
    return {};
  }
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
  /**
   * M2: `${packId}/${kitId}` of the "Get & load" waiting for its download
   * (`getAndLoad`), or null. Only the latest request is pending.
   */
  pendingLoad: string | null;
  /** N2: details of `pendingLoad` for the Beat pads' "Getting <pack>… ✕" chip, or null. */
  pendingLoadInfo: PendingLoadInfo | null;
}

/** A "Get & load" waiting for its download (or for loop recording to stop). */
export interface PendingLoadInfo {
  /** `${packId}/${kitId}` (same as `pendingLoad`). */
  key: string;
  packId: string;
  kitId: string;
  /** Human pack name (catalog / installed / remembered), else the id. */
  packName: string;
  /** Downloaded, but loop recording is running: the kit loads once the take stops. */
  waitingForRecording: boolean;
}

/**
 * N2: what a "Get & load" was aimed at when it was requested. The kit only
 * loads while all three still hold (the user picked nothing else meanwhile).
 */
interface LoadTarget {
  /** Active drum track at request time (null = none yet: one is created on load). */
  trackId: string | null;
  /** That track's kit id at request time. */
  kitId: string | null;
  /** Project session (`arrangementEngine.getSessionId()`; a new / opened project drops the request). */
  session: number;
}

/** Emitted when a pending "Get & load" put its kit on the pads (see `onKitLoaded`). */
export interface KitLoadedEvent {
  packId: string;
  kitId: string;
  kit: KitState;
  /** Drum track the kit was put on (null only with a test applier and no drum track). */
  trackId: string | null;
}

/**
 * Status of a pack referenced by a pad: 'installed', 'installing' (download
 * running), or 'missing' (not installed: the pad is silent until it is; show
 * "Pack not installed — Get", see `packService.install`). 'unavailable' (L-c):
 * not installed AND the loaded catalog does not list it (withdrawn / unknown
 * pack): it cannot be downloaded, so offer to clear or replace the pads
 * instead of "Get". 'unknown' before the installed list was loaded (or outside
 * Electron).
 */
export type PackRefStatus = 'installed' | 'installing' | 'missing' | 'unavailable' | 'unknown';

function captureLoadTarget(): LoadTarget {
  const trackId = arrangementEngine.getActiveDrumTrackId();
  return { trackId, kitId: trackId ? arrangementEngine.getKit(trackId).id : null, session: arrangementEngine.getSessionId() };
}

/** The user changed nothing the request was aimed at (same project, drum track and kit). */
function loadTargetHolds(t: LoadTarget): boolean {
  if (arrangementEngine.getSessionId() !== t.session) return false;
  // Compare the active drum track first, also when there was none at request
  // time: a drum track created/selected since then is a new target the user
  // chose, so the request no longer applies. A target that stays null until
  // the download ends gets a drum track created when the kit is applied.
  if (arrangementEngine.getActiveDrumTrackId() !== t.trackId) return false;
  if (t.trackId === null) return true;
  return arrangementEngine.getKit(t.trackId).id === t.kitId;
}

/** A take is running (Beat loop take or Song recording): kit swaps wait for it to end. */
function isRecordingNow(): boolean {
  return arrangementEngine.isLoopRecording() || arrangementEngine.getState() === 'recording';
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
    uninstallErrors: {},
    pendingLoad: null,
    pendingLoadInfo: null,
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
  /** Bumped by every "Get & load" request (and its cancel): only the latest one loads. */
  private loadToken = 0;
  private kitApplier: ((kit: KitState, trackId: string | null) => void | Promise<void>) | null = null;
  private kitLoadedListeners = new Set<(e: KitLoadedEvent) => void>();
  /** N2: target of the pending "Get & load" and the engine watch that drops it when the target changes. */
  private pendingTarget: LoadTarget | null = null;
  private stopTargetWatch: (() => void) | null = null;
  /** Wakes a "Get & load" deferred by loop recording (recording stopped, or the request was dropped). */
  private recordingWaiters = new Set<() => void>();
  /** True while a "Get & load" puts its own kit on the pads (that kit change must not cancel it). */
  private applyingKit = false;
  /** N6: names of packs seen on this machine (catalog + installed), persisted. */
  private rememberedNames: Record<string, string> = readRememberedNames();

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
        this.rememberNames(catalog.packs);
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
      this.rememberNames(installed.map((p) => p.manifest));
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

  // --- Pack names (N6) ---------------------------------------------------------

  /**
   * Human name of pack `id`: the loaded catalog, the installed copy, the
   * withdrawn-packs list (packs/withdrawn.json), a name remembered from an
   * earlier catalog / install on this machine, else the id itself.
   */
  packName(id: string): string {
    return (
      this.snap.catalog?.packs.find((p) => p.id === id)?.name ||
      this.getInstalled(id)?.manifest.name ||
      (Object.prototype.hasOwnProperty.call(WITHDRAWN_PACK_NAMES, id) ? WITHDRAWN_PACK_NAMES[id] : '') ||
      (Object.prototype.hasOwnProperty.call(this.rememberedNames, id) ? this.rememberedNames[id] : '') ||
      id
    );
  }

  private rememberNames(manifests: readonly { id: string; name: string }[]): void {
    let changed = false;
    const next = { ...this.rememberedNames };
    for (const m of manifests) {
      if (!m || typeof m.id !== 'string' || typeof m.name !== 'string' || !m.name) continue;
      if (next[m.id] === m.name) continue;
      delete next[m.id]; // re-insert: most recent last (oldest trimmed first)
      next[m.id] = m.name;
      changed = true;
    }
    if (!changed) return;
    const keys = Object.keys(next);
    for (const k of keys.slice(0, Math.max(0, keys.length - MAX_REMEMBERED_NAMES))) delete next[k];
    this.rememberedNames = next;
    try {
      localStorage.setItem(PACK_NAMES_STORAGE_KEY, JSON.stringify(next));
    } catch {
      /* private window / storage blocked: names just aren't remembered */
    }
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
    if (!this.installedStamps) return 'unknown';
    return this.isUnavailable(id) ? 'unavailable' : 'missing';
  }

  /**
   * L-c: the loaded catalog does not list pack `id` (withdrawn or unknown), so
   * it cannot be downloaded. False while the catalog is not loaded (call
   * `loadCatalog()` when a project references missing packs).
   */
  isUnavailable(id: string): boolean {
    const cat = this.snap.catalog;
    return !!cat && !this.isInstalled(id) && !cat.packs.some((p) => p.id === id);
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
    if (this.snap.pendingLoad?.startsWith(`${id}/`)) this.cancelPendingLoad();
    await api()?.packsCancel(id);
  }

  // --- Get & load (M2, N2) ---------------------------------------------------

  /**
   * One step "Get & load": installs pack `packId` when needed, then loads its
   * kit `kitId` on the drum track that was active when it was requested. Lives
   * here (app level), so it completes even when the Packs page was left
   * meanwhile. The request is an intent bound to what the user was looking at
   * (N2): it is dropped when a newer request or `cancelPendingLoad()` (any
   * explicit kit choice) comes in, or when the project session, the active
   * drum track or that track's kit changes before the download finishes.
   * While loop recording runs the kit waits (`pendingLoadInfo.
   * waitingForRecording`) and loads after the take stops, so a take never
   * changes kit midway. `pendingLoad` is `${packId}/${kitId}` while waiting.
   * Resolves with the loaded kit, or null when it failed / was cancelled /
   * was superseded / was dropped. `onKitLoaded` reports a load.
   */
  async getAndLoad(packId: string, kitId: string): Promise<KitState | null> {
    this.dropPendingLoad();
    const token = ++this.loadToken;
    const key = `${packId}/${kitId}`;
    const target = captureLoadTarget();
    this.pendingTarget = target;
    this.watchTarget();
    this.set({ pendingLoad: key, pendingLoadInfo: { key, packId, kitId, packName: this.packName(packId), waitingForRecording: false } });
    const current = () => token === this.loadToken;
    try {
      let inst = this.getInstalled(packId);
      const manifest = this.snap.catalog?.packs.find((p) => p.id === packId);
      if (!inst || (manifest && inst.version < manifest.version)) inst = await this.install(packId);
      if (!inst || !current()) return null; // failed (installState says why), cancelled or superseded
      const kit = kitFromPack(inst, kitId);
      if (!kit) {
        console.warn(`[packs] ${packId} has no kit ${kitId}`);
        return null;
      }
      if (!loadTargetHolds(target)) return null;
      // Never swap the kit mid-take (loop take or Song recording). If a new take
      // started before the waiter woke (Stop → Record in one step), wait again
      // instead of dropping the request (review R3/R4).
      while (isRecordingNow()) {
        this.set({ pendingLoadInfo: { ...this.snap.pendingLoadInfo!, waitingForRecording: true } });
        await new Promise<void>((resolve) => this.recordingWaiters.add(resolve));
        if (!current() || !loadTargetHolds(target)) return null;
      }
      let trackId: string | null = target.trackId;
      this.applyingKit = true;
      try {
        trackId = await this.applyKit(kit, trackId);
      } finally {
        this.applyingKit = false;
      }
      if (!current()) return null;
      const event: KitLoadedEvent = { packId, kitId, kit, trackId };
      for (const cb of [...this.kitLoadedListeners]) {
        try {
          cb(event);
        } catch (err) {
          console.warn('[packs] kit-loaded listener failed', err);
        }
      }
      return kit;
    } catch (err) {
      console.warn('[packs] get & load failed', err);
      return null;
    } finally {
      if (current()) this.dropPendingLoad();
    }
  }

  /**
   * Drops the pending "Get & load" (its download keeps running; it just will
   * not load). Call on every explicit kit choice, in any view (N2).
   */
  cancelPendingLoad(): void {
    this.loadToken++;
    this.dropPendingLoad();
  }

  private dropPendingLoad(): void {
    this.pendingTarget = null;
    this.stopTargetWatch?.();
    this.stopTargetWatch = null;
    const waiters = [...this.recordingWaiters];
    this.recordingWaiters.clear();
    for (const w of waiters) w();
    if (this.snap.pendingLoad !== null || this.snap.pendingLoadInfo !== null) this.set({ pendingLoad: null, pendingLoadInfo: null });
  }

  /**
   * While a request is pending: drop it as soon as its target changes (so the
   * chip disappears at once, not when the download ends), and wake a load
   * deferred by loop recording once the take stops.
   */
  private watchTarget(): void {
    if (this.stopTargetWatch) return;
    const check = () => {
      const target = this.pendingTarget;
      if (!target || this.applyingKit) return;
      if (!loadTargetHolds(target)) {
        this.cancelPendingLoad();
        return;
      }
      if (this.recordingWaiters.size > 0 && !isRecordingNow()) {
        const waiters = [...this.recordingWaiters];
        this.recordingWaiters.clear();
        for (const w of waiters) w();
      }
    };
    const offState = arrangementEngine.onStateChange(check);
    const offKit = arrangementEngine.onKitChange(check);
    // Any explicit kit choice, in any view, even of the kit already loaded, wins (latest intent).
    const offSelected = arrangementEngine.onKitSelected((e) => {
      if (e.source === 'user' && this.pendingTarget && !this.applyingKit) this.cancelPendingLoad();
    });
    this.stopTargetWatch = () => {
      offState();
      offKit();
      offSelected();
    };
  }

  /**
   * Called after a pending "Get & load" put its kit on the pads (the Packs page
   * navigates to the pads when it is still open). Returns unsubscribe.
   */
  onKitLoaded(cb: (e: KitLoadedEvent) => void): () => void {
    this.kitLoadedListeners.add(cb);
    return () => {
      this.kitLoadedListeners.delete(cb);
    };
  }

  /** Overrides how a "Get & load" kit is applied (default: `arrangementEngine.setKit(trackId, kit, 'pack-load')`). Tests. */
  setKitApplier(fn: ((kit: KitState, trackId: string | null) => void | Promise<void>) | null): void {
    this.kitApplier = fn;
  }

  /** Puts the kit on `trackId` (null: the active drum track, created when there is none). Returns the track. */
  private async applyKit(kit: KitState, trackId: string | null): Promise<string | null> {
    if (this.kitApplier) {
      await this.kitApplier(kit, trackId);
      return trackId;
    }
    const id = trackId ?? arrangementEngine.ensureDrumTrack();
    arrangementEngine.setKit(id, kit, 'pack-load');
    return id;
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
    this.loadToken = 0;
    this.stopTargetWatch?.();
    this.stopTargetWatch = null;
    this.pendingTarget = null;
    this.recordingWaiters.clear();
    this.applyingKit = false;
    this.rememberedNames = readRememberedNames();
    this.kitApplier = null;
    this.kitLoadedListeners.clear();
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
      pendingLoad: null,
      pendingLoadInfo: null,
    };
  }
}

export const packService = new PackService();
