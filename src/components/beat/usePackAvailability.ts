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
  /**
   * False when the loaded catalog no longer lists the pack (withdrawn, or
   * never published here): Get can't work, so the UI offers clear / replace
   * instead (review L-c). True while the catalog is unknown.
   */
  available: boolean;
}

export interface PackAvailability {
  /** Installed packs are known (avoids flashing "not installed" while they load). */
  loaded: boolean;
  /** Version of an installed pack (undefined = not installed). */
  versionOf: (packId: string) => number | undefined;
  /** True once installed packs are known and `packId` is not one of them. */
  isMissing: (packId: string) => boolean;
  nameOf: (packId: string) => string;
  /** False once the catalog is loaded and does not list `packId` (see MissingPack.available). */
  isAvailable: (packId: string) => boolean;
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
  // packService.isUnavailable reads the snapshot's catalog / installed list.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const isAvailable = useCallback((id: string) => !packService.isUnavailable(id), [snap.catalog, snap.installed]);
  const stateOf = useCallback((id: string) => snap.installStates[id] ?? IDLE, [snap.installStates]);
  const get = useCallback((id: string) => {
    void packService.loadCatalog().then(() => packService.install(id));
  }, []);
  // Stable identity until something changes, so memoised consumers skip renders.
  return useMemo(
    () => ({ loaded, versionOf, isMissing, nameOf, isAvailable, stateOf, get }),
    [loaded, versionOf, isMissing, nameOf, isAvailable, stateOf, get],
  );
}

/** Pure: packs that `pads` need but are missing, in pad order. */
export function missingPacks(
  pads: readonly Pick<PadSettings, 'sample'>[],
  avail: Pick<PackAvailability, 'isMissing' | 'nameOf' | 'stateOf'> & Partial<Pick<PackAvailability, 'isAvailable'>>,
): MissingPack[] {
  const byId = new Map<string, MissingPack>();
  pads.forEach((p, i) => {
    const id = packIdOfSource(p.sample?.source);
    if (!id || !avail.isMissing(id)) return;
    const entry = byId.get(id);
    if (entry) entry.pads.push(i);
    else byId.set(id, { id, name: avail.nameOf(id), pads: [i], state: avail.stateOf(id), available: avail.isAvailable?.(id) ?? true });
  });
  return [...byId.values()];
}

/**
 * Pure: per pad, the tooltip for its "no pack" badge (null = the pad's pack is
 * fine). Strings, so memoised pads only re-render when their own text changes
 * (review L-b: thumbnails no longer subscribe to pack state themselves).
 */
export function missingPackTitles(missing: readonly MissingPack[], padCount: number): (string | null)[] {
  const out: (string | null)[] = new Array(padCount).fill(null);
  for (const m of missing) {
    const title = m.available ? `Pack not installed: ${m.name}` : `Pack no longer available: ${m.name}`;
    for (const p of m.pads) if (p >= 0 && p < padCount) out[p] = title;
  }
  return out;
}

/** Loads the catalog (for pack names) while some pack is missing. */
export function useCatalogWhile(needed: boolean): void {
  useEffect(() => {
    if (needed) void packService.loadCatalog();
  }, [needed]);
}
