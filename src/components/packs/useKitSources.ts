import { useMemo } from 'react';
import { CLASSIC_MACHINES, createClassicKit } from '@/audio/sampler/classic';
import { usePacks } from '@/hooks/usePacks';
import { kitFromPack, packKitId } from '@/services/PackService';
import type { InstalledPack } from '@/types/packs';
import type { KitState } from '@/types/sampler';

/** One loadable kit in the Beat view's kit menu. */
export interface KitChoice {
  id: string;
  name: string;
  /** Secondary text (the pack's name). */
  detail?: string;
  color?: string;
  /** Builds a fresh kit (null if the pack's files are gone). */
  create: () => KitState | null;
}

export interface KitSourceGroup {
  builtIn: KitChoice[];
  packs: KitChoice[];
}

/** Pure: kit choices of installed packs, in install order. */
export function packKitChoices(installed: readonly InstalledPack[]): KitChoice[] {
  return installed.flatMap((inst) =>
    inst.manifest.kits.map((k) => ({
      id: packKitId(inst.id, k.id),
      name: k.name,
      detail: inst.manifest.name,
      color: inst.manifest.color,
      create: () => kitFromPack(inst, k.id),
    })),
  );
}

/** Built-in classic kits + kits of installed packs, for the kit menu. */
export function useKitSources(): KitSourceGroup {
  const { installed } = usePacks();
  const builtIn = useMemo<KitChoice[]>(
    () =>
      CLASSIC_MACHINES.map((m) => ({
        id: `classic:${m.id}`,
        name: m.name,
        color: m.color,
        create: () => createClassicKit(m.id),
      })),
    [],
  );
  const packs = useMemo(() => packKitChoices(installed), [installed]);
  return { builtIn, packs };
}
