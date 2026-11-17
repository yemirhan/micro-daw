import { useCallback, useEffect, useMemo, useState } from 'react';
import { usePacks } from '@/hooks/usePacks';
import { packService } from '@/services/PackService';
import type { PackInstallState } from '@/types/packs';
import type { PadSettings } from '@/types/sampler';
import { packIdOfSource } from '@/components/packs/packsModel';

/** A pack some pads need that is not installed. */
export interface MissingPack {
  id: string;
  /** Catalog name when known, else the id. */
  name: string;
  /** Pads (indices) that play sounds of it. */
  pads: number[];
  /** Install progress / failure (idle when not started). */
  state: PackInstallState;
}

export interface PackAvailability {
  /** Installed packs are known (avoids flashing "not installed" while they load). */
  loaded: boolean;
  /** Version of an installed pack (undefined = not installed). */
  versionOf: (packId: string) => number | undefined;
  /** True once installed packs are known and `packId` is not one of them. */
  isMissing: (packId: string) => boolean;
  nameOf: (packId: string) => string;
  stateOf: (packId: string) => PackInstallState;
  /** Downloads + installs the pack (progress via `stateOf`). */
  get: (packId: string) => void;
}

const IDLE: PackInstallState = { state: 'idle' };

/**
 * Which packs are installed, for pads whose sample is a pack sound
 * (`{ kind: 'pack' }`). Loads the catalog only while something is missing
 * (for pack names).
 */
export function usePackAvailability(): PackAvailability {
  const snap = usePacks();
  const [loaded, setLoaded] = useState(false);
  useEffect(() => {
    let alive = true;
    void packService.init().then(() => alive && setLoaded(true));
    return () => {
      alive = false;
    };
  }, []);
  const versions = useMemo(() => new Map(snap.installed.map((p) => [p.id, p.version])), [snap.installed]);
  const names = useMemo(() => new Map((snap.catalog?.packs ?? []).map((m) => [m.id, m.name])), [snap.catalog]);
  const versionOf = useCallback((id: string) => versions.get(id), [versions]);
  const isMissing = useCallback((id: string) => loaded && !versions.has(id), [loaded, versions]);
  const nameOf = useCallback((id: string) => names.get(id) ?? id, [names]);
  const stateOf = useCallback((id: string) => snap.installStates[id] ?? IDLE, [snap.installStates]);
  const get = useCallback((id: string) => {
    void packService.loadCatalog().then(() => packService.install(id));
  }, []);
  return { loaded, versionOf, isMissing, nameOf, stateOf, get };
}

/** Pure: packs that `pads` need but are missing, in pad order. */
export function missingPacks(pads: readonly Pick<PadSettings, 'sample'>[], avail: Pick<PackAvailability, 'isMissing' | 'nameOf' | 'stateOf'>): MissingPack[] {
  const byId = new Map<string, MissingPack>();
  pads.forEach((p, i) => {
    const id = packIdOfSource(p.sample?.source);
    if (!id || !avail.isMissing(id)) return;
    const entry = byId.get(id);
    if (entry) entry.pads.push(i);
    else byId.set(id, { id, name: avail.nameOf(id), pads: [i], state: avail.stateOf(id) });
  });
  return [...byId.values()];
}

/** Loads the catalog (for pack names) while some pack is missing. */
export function useCatalogWhile(needed: boolean): void {
  useEffect(() => {
    if (needed) void packService.loadCatalog();
  }, [needed]);
}
