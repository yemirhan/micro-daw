import { useEffect, useSyncExternalStore } from 'react';
import { packService, type PackServiceSnapshot } from '@/services/PackService';

/**
 * Pack store state for React. Initialises the service (progress events,
 * installed packs) on first use; pass `{ catalog: true }` to also load the
 * catalog (remote with bundled fallback). Actions live on `packService`.
 */
export function usePacks(opts: { catalog?: boolean } = {}): PackServiceSnapshot {
  const snap = useSyncExternalStore(packService.subscribe, packService.getSnapshot, packService.getSnapshot);
  const wantCatalog = !!opts.catalog;
  useEffect(() => {
    void packService.init();
    if (wantCatalog) void packService.loadCatalog();
  }, [wantCatalog]);
  return snap;
}
