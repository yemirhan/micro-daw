/**
 * The Packs page's only link to the audio / pack services: built-in classic
 * machines (P1), the pack catalog + installs (P2's packService / usePacks),
 * previews, and "Load on pads". Everything below it is presentational.
 */
import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from 'react';
import { CLASSIC_MACHINES, createClassicKit } from '@/audio/sampler/classic';
import { usePacks } from '@/hooks/usePacks';
import { ipcErrorMessage, kitFromPack, packFilePath, packService } from '@/services/PackService';
import { arrangementEngine } from '@/services/ArrangementEngine';
import type { PackServiceSnapshot } from '@/services/PackService';
import type { InstalledPack, PackCatalog, PackInstallState } from '@/types/packs';
import type { ClassicMachineId, KitState } from '@/types/sampler';
import { kitPreview } from './kitPreview';
import { kitsUsePack, soundKey, type ClassicMachineInfo, type PackItem, type SoundRef } from './packsModel';
import { remoteKitPlan, remotePadLoader } from './remoteKit';

export type CatalogStatus = 'loading' | 'ready' | 'error';

export type { SoundRef } from './packsModel';

export interface PacksController {
  builtins: readonly ClassicMachineInfo[];
  catalog: PackCatalog | null;
  catalogStatus: CatalogStatus;
  catalogError: string | null;
  installed: readonly InstalledPack[];
  stateOf: (id: string) => PackInstallState;
  retryCatalog: () => void;
  install: (id: string) => void;
  cancel: (id: string) => void;
  uninstall: (id: string) => void;
  /** Why the last uninstall of `id` failed (null = it didn't). */
  uninstallError: (id: string) => string | null;
  dismissUninstallError: (id: string) => void;
  /** An installed pack whose catalog entry has a newer version. */
  hasUpdate: (id: string) => boolean;
  /** The ready-to-load kit (null for a downloadable pack that isn't installed). */
  kitFor: (item: PackItem, kitId: string) => KitState | null;
  /** Kit-preview key currently playing (`${item.id}/${kitId}`), or null. */
  previewing: string | null;
  /** Plays the demo groove on a kit; a not-installed pack streams its kit's files first. */
  previewKit: (item: PackItem, kitId: string) => void;
  /** Why the last preview of kit-preview key `key` failed (null = it didn't). */
  kitPreviewError: (key: string) => string | null;
  stopKitPreview: () => void;
  playSound: (item: PackItem, sound: SoundRef) => void;
  /** Why the last preview of soundKey() `key` failed (null = it didn't); playing it again retries. */
  soundError: (key: string) => string | null;
  /** soundKey() of the sound preview playing right now (cleared when it ends), or null. */
  soundPreviewing: string | null;
  /** Loads a kit on the active drum track's pads. */
  loadKit: (kit: KitState) => void;
  /** One step: download the pack, then load its kit `kitId` on the pads. */
  getAndLoad: (item: PackItem, kitId: string) => void;
  /** Kit-preview key (`${item.id}/${kitId}`) waiting for its download to load, or null. */
  pendingLoad: string | null;
  /** The open project has pads that use this installed pack (uninstall would leave them silent). */
  usedByProject: (id: string) => boolean;
}

const IDLE: PackInstallState = { state: 'idle' };
const INSTALLED: PackInstallState = { state: 'installed' };

function catalogStatus(s: PackServiceSnapshot): CatalogStatus {
  if (s.catalogStatus === 'error' || (!s.available && !s.catalog)) return 'error';
  if (s.catalog) return 'ready';
  return 'loading';
}

export function usePacksController(onLoaded?: () => void): PacksController {
  const packs = usePacks({ catalog: true });
  const previewing = useSyncExternalStore(
    (cb) => kitPreview.subscribe(cb),
    () => kitPreview.playing,
  );
  const [soundErrors, setSoundErrors] = useState<Readonly<Record<string, string>>>({});
  const [kitErrors, setKitErrors] = useState<Readonly<Record<string, string>>>({});
  const [pendingLoad, setPendingLoad] = useState<string | null>(null);
  const mounted = useRef(true);

  // Leaving the page stops the demo groove and any sound preview.
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      kitPreview.stop();
      packService.stopPreview();
    };
  }, []);

  const installedById = useMemo(() => new Map(packs.installed.map((p) => [p.id, p])), [packs.installed]);

  const kitFor = useCallback(
    (item: PackItem, kitId: string): KitState | null => {
      if (item.kind === 'builtin') return createClassicKit(item.machine.id as ClassicMachineId);
      const inst = installedById.get(item.id);
      if (!inst) return null;
      try {
        return kitFromPack(inst, kitId);
      } catch (err) {
        console.warn('[packs] could not build kit', err);
        return null;
      }
    },
    [installedById],
  );

  const setError = (set: typeof setSoundErrors, key: string, message: string | null) =>
    set((prev) => {
      if (message === null) {
        if (!(key in prev)) return prev;
        const next = { ...prev };
        delete next[key];
        return next;
      }
      return { ...prev, [key]: message };
    });

  const playSound = useCallback(
    (item: PackItem, sound: SoundRef) => {
      kitPreview.stop();
      const key = soundKey(item.id, sound);
      setError(setSoundErrors, key, null);
      // The service logs the error and clears `previewing`; the row shows it inline with a retry.
      const fail = (err: unknown) => {
        if (mounted.current) setError(setSoundErrors, key, ipcErrorMessage(err));
      };
      if (item.kind === 'builtin') {
        if (sound.kind === 'voice') void packService.previewSound({ kind: 'classic', machine: item.machine.id, voice: sound.voice }, key).catch(fail);
        return;
      }
      if (sound.kind !== 'file') return;
      const inst = installedById.get(item.id);
      if (inst) void packService.previewFile(packFilePath(inst.dir, sound.file.path), key).catch(fail);
      else void packService.previewRemote(sound.file.url, key).catch(fail);
    },
    [installedById],
  );

  const previewKit = useCallback(
    (item: PackItem, kitId: string) => {
      const key = `${item.id}/${kitId}`;
      packService.stopPreview();
      setError(setKitErrors, key, null);
      let run: Promise<string | null>;
      const kit = kitFor(item, kitId);
      if (kit) {
        run = kitPreview.play(key, kit);
      } else if (item.kind === 'remote') {
        // Not installed: stream the kit's files through the preview IPC.
        const plan = remoteKitPlan(item.manifest, kitId);
        if (!plan) return;
        run = kitPreview.play(key, plan.kit, { loadBuffer: remotePadLoader(plan) });
      } else {
        return;
      }
      void run.then((err) => {
        if (err && mounted.current) setError(setKitErrors, key, err);
      });
    },
    [kitFor],
  );

  const loadKit = useCallback(
    (kit: KitState) => {
      kitPreview.stop();
      arrangementEngine.setKit(arrangementEngine.ensureDrumTrack(), kit);
      onLoaded?.();
    },
    [onLoaded],
  );

  const getAndLoad = useCallback(
    (item: PackItem, kitId: string) => {
      if (item.kind !== 'remote') return;
      const key = `${item.id}/${kitId}`;
      setPendingLoad(key);
      void packService.install(item.id).then((inst) => {
        if (!mounted.current) return;
        setPendingLoad((cur) => (cur === key ? null : cur));
        // Failed / cancelled: the install state shows why (with Retry).
        if (!inst) return;
        try {
          const kit = kitFromPack(inst, kitId);
          if (kit) loadKit(kit);
        } catch (err) {
          console.warn('[packs] could not build kit', err);
        }
      });
    },
    [loadKit],
  );

  const usedByProject = useCallback(
    (id: string) => {
      // PackService resolves pack sources and legacy absolute paths against the
      // packs folder; the installed copy's own dir covers a folder override.
      const kits = arrangementEngine.getTracks().map((t) => t.instrument.kit);
      if (packService.isUsedByProject(id, kits)) return true;
      const inst = installedById.get(id);
      return !!inst && kitsUsePack(kits, id, inst.dir);
    },
    [installedById],
  );

  return {
    builtins: CLASSIC_MACHINES,
    catalog: packs.catalog,
    catalogStatus: catalogStatus(packs),
    catalogError: packs.catalogError ?? (packs.available ? null : 'Downloading packs needs the desktop app.'),
    installed: packs.installed,
    stateOf: (id) => packs.installStates[id] ?? (installedById.has(id) ? INSTALLED : IDLE),
    retryCatalog: () => void packService.loadCatalog({ force: true }),
    install: (id) => void packService.install(id),
    cancel: (id) => {
      setPendingLoad((cur) => (cur?.startsWith(`${id}/`) ? null : cur));
      void packService.cancel(id);
    },
    // A failure lands in packService's uninstallErrors (shown in the detail panel).
    uninstall: (id) => void packService.uninstall(id).catch((err: unknown) => console.warn('[packs] uninstall failed', err)),
    uninstallError: (id) => packs.uninstallErrors[id] ?? null,
    dismissUninstallError: (id) => packService.dismissError(id),
    hasUpdate: (id) => {
      const m = packs.catalog?.packs.find((p) => p.id === id);
      const inst = installedById.get(id);
      return !!m && !!inst && inst.version < m.version;
    },
    kitFor,
    previewing,
    previewKit,
    kitPreviewError: (key) => kitErrors[key] ?? null,
    stopKitPreview: () => kitPreview.stop(),
    playSound,
    soundError: (key) => soundErrors[key] ?? null,
    soundPreviewing: packs.previewing,
    loadKit,
    getAndLoad,
    pendingLoad,
    usedByProject,
  };
}
