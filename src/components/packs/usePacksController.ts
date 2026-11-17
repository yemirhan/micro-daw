/**
 * The Packs page's only link to the audio / pack services: built-in classic
 * machines (P1), the pack catalog + installs (P2's packService / usePacks),
 * previews, and "Load on pads". Everything below it is presentational.
 */
import { useCallback, useEffect, useMemo, useSyncExternalStore } from 'react';
import { CLASSIC_MACHINES, createClassicKit } from '@/audio/sampler/classic';
import { usePacks } from '@/hooks/usePacks';
import { kitFromPack, packFilePath, packService } from '@/services/PackService';
import { drumEngine } from '@/services/DrumEngine';
import { arrangementEngine } from '@/services/ArrangementEngine';
import type { PackServiceSnapshot } from '@/services/PackService';
import type { InstalledPack, PackCatalog, PackFile, PackInstallState } from '@/types/packs';
import type { ClassicMachineId, KitState } from '@/types/sampler';
import { kitPreview } from './kitPreview';
import type { ClassicMachineInfo, PackItem } from './packsModel';

export type CatalogStatus = 'loading' | 'ready' | 'error';

/** A playable sound in the detail panel: a classic voice or a pack file. */
export type SoundRef = { kind: 'voice'; voice: string } | { kind: 'file'; file: PackFile };

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
  /** An installed pack whose catalog entry has a newer version. */
  hasUpdate: (id: string) => boolean;
  /** The ready-to-load kit (null for a downloadable pack that isn't installed). */
  kitFor: (item: PackItem, kitId: string) => KitState | null;
  /** Kit-preview key currently playing (`${item.id}/${kitId}`), or null. */
  previewing: string | null;
  previewKit: (key: string, kit: KitState) => void;
  stopKitPreview: () => void;
  playSound: (item: PackItem, sound: SoundRef) => void;
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
      if (item.kind === 'builtin') {
        if (sound.kind === 'voice') {
          packService.stopPreview();
          void drumEngine.preview({ kind: 'classic', machine: item.machine.id, voice: sound.voice });
        }
        return;
      }
      if (sound.kind !== 'file') return;
      const inst = installedById.get(item.id);
      if (inst) void packService.previewFile(packFilePath(inst.dir, sound.file.path));
      else void packService.previewRemote(sound.file.url).catch(() => undefined); // logged by the service
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
    uninstall: (id) => void packService.uninstall(id).catch((err: unknown) => console.warn('[packs] uninstall failed', err)),
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
    loadKit,
  };
}
