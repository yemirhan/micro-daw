import type { KitState } from '@/types/sampler';
import { cloneKit, normalizeKit } from '@/audio/sampler/factoryKit';

const STORAGE_KEY = 'micro-daw:kits';

/**
 * User kits, persisted in localStorage. Kits are stored with file paths only
 * (the runtime `sampleId` of file sources is stripped).
 */
class KitStore {
  private listeners = new Set<() => void>();

  list(): KitState[] {
    return this.read();
  }

  get(id: string): KitState | null {
    return this.read().find((k) => k.id === id) ?? null;
  }

  /** Insert or replace (by id). */
  save(kit: KitState): void {
    const clean = forStorage(kit);
    const kits = this.read();
    const i = kits.findIndex((k) => k.id === clean.id);
    if (i >= 0) kits[i] = clean;
    else kits.push(clean);
    this.write(kits);
  }

  remove(id: string): void {
    const kits = this.read();
    const next = kits.filter((k) => k.id !== id);
    if (next.length !== kits.length) this.write(next);
  }

  subscribe(cb: () => void): () => void {
    this.listeners.add(cb);
    return () => {
      this.listeners.delete(cb);
    };
  }

  private read(): KitState[] {
    try {
      const raw = localStorage.getItem(STORAGE_KEY);
      if (!raw) return [];
      const parsed: unknown = JSON.parse(raw);
      return Array.isArray(parsed) ? parsed.filter((k) => k && typeof k === 'object').map(normalizeKit) : [];
    } catch (err) {
      console.warn('[kits] could not read saved kits', err);
      return [];
    }
  }

  private write(kits: KitState[]): void {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(kits));
    } catch (err) {
      console.warn('[kits] could not save kits', err);
    }
    for (const cb of [...this.listeners]) cb();
  }
}

function forStorage(kit: KitState): KitState {
  const copy = cloneKit(normalizeKit(kit));
  for (const pad of copy.pads) {
    const src = pad.sample?.source;
    if (src?.kind === 'file') delete src.sampleId;
  }
  return copy;
}

export const kitStore = new KitStore();
