/**
 * The Packs page's only link to the audio / pack services: built-in classic
 * machines (P1), the pack catalog + installs (P2's packService / usePacks),
 * previews, and "Load on pads". Everything below it is presentational.
 */
import { useCallback, useEffect, useMemo, useSyncExternalStore } from 'react';
import { CLASSIC_MACHINES, createClassicKit } from '@/audio/sampler/classic';
import { usePacks } from '@/hooks/usePacks';
import { kitFromPack, packFilePath, packService } from '@/services/PackService';
import { arrangementEngine } from '@/services/ArrangementEngine';
import type { PackServiceSnapshot } from '@/services/PackService';
import type { InstalledPack, PackCatalog, PackInstallState } from '@/types/packs';
import type { ClassicMachineId, KitState } from '@/types/sampler';
import { kitPreview } from './kitPreview';
import { soundKey, type ClassicMachineInfo, type PackItem, type SoundRef } from './packsModel';

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
  previewKit: (key: string, kit: KitState) => void;
  stopKitPreview: () => void;
  playSound: (item: PackItem, sound: SoundRef) => void;
  /** soundKey() of the sound preview playing right now (cleared when it ends), or null. */
  soundPreviewing: string | null;
  /** Loads a kit on the active drum track's pads. */
  loadKit: (kit: KitState) => void;
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

  // Leaving the page stops the demo groove and any sound preview.
  useEffect(
    () => () => {
      kitPreview.stop();
      packService.stopPreview();
    },
    [],
  );

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

  const playSound = useCallback(
    (item: PackItem, sound: SoundRef) => {
      kitPreview.stop();
      const key = soundKey(item.id, sound);
      // Errors are logged by the service (and clear `previewing`).
      const quiet = () => undefined;
      if (item.kind === 'builtin') {
        if (sound.kind === 'voice') void packService.previewSound({ kind: 'classic', machine: item.machine.id, voice: sound.voice }, key).catch(quiet);
        return;
      }
      if (sound.kind !== 'file') return;
      const inst = installedById.get(item.id);
      if (inst) void packService.previewFile(packFilePath(inst.dir, sound.file.path), key).catch(quiet);
      else void packService.previewRemote(sound.file.url, key).catch(quiet);
    },
    [installedById],
  );

  const loadKit = useCallback(
    (kit: KitState) => {
      kitPreview.stop();
      arrangementEngine.setKit(arrangementEngine.ensureDrumTrack(), kit);
      onLoaded?.();
    },
    [onLoaded],
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
    cancel: (id) => void packService.cancel(id),
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
    previewKit: (key, kit) => {
      packService.stopPreview();
      void kitPreview.play(key, kit);
    },
    stopKitPreview: () => kitPreview.stop(),
    playSound,
    soundPreviewing: packs.previewing,
    loadKit,
  };
}
