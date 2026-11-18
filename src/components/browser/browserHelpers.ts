/**
 * Pure helpers for the sample browser: listing sort/filter and path
 * manipulation. No DOM, no Node APIs. The listing rules (audio extensions,
 * hidden files, order) and the basic path helpers live in src/shared/ (the
 * main process's `fs:list-dir` uses them too) and are re-exported here.
 */
import type { FsDirEntry } from '@/types/electron';
import { basename, joinPath, separatorOf, trimTrailingSep } from '@/shared/paths';

export { AUDIO_EXTENSIONS, compareEntries, extOf, isAudioFileName, isHiddenName, sortEntries } from '@/shared/fsEntries';
export { basename, displayName, joinPath, separatorOf, trimTrailingSep } from '@/shared/paths';

/** Drag-and-drop MIME type carrying `{ path, name }` JSON (see docs/beat-mode-plan.md). */
export const SAMPLE_DRAG_MIME = 'application/x-micro-daw-sample';

export interface SampleDragPayload {
  path: string;
  name: string;
}

/**
 * Filters a listing by a search query. Every whitespace-separated term must
 * occur in the name (case-insensitive). Empty query returns the input.
 */
export function filterEntries<T extends Pick<FsDirEntry, 'name'>>(entries: readonly T[], query: string): T[] {
  const terms = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  if (terms.length === 0) return [...entries];
  return entries.filter((e) => {
    const name = e.name.toLowerCase();
    return terms.every((t) => name.includes(t));
  });
}

// ---------------------------------------------------------------------------
// Paths (POSIX and Windows separators; the renderer has no node:path)
// ---------------------------------------------------------------------------

/** Parent directory, or null for a filesystem root / single segment. */
export function parentPath(p: string): string | null {
  const trimmed = trimTrailingSep(p);
  const idx = Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\'));
  if (idx < 0) return null;
  if (idx === 0) return trimmed.length > 1 ? trimmed.slice(0, 1) : null;
  const parent = trimmed.slice(0, idx);
  // "C:" → "C:\"
  if (/^[A-Za-z]:$/.test(parent)) return trimmed.length > 3 ? `${parent}${trimmed[idx]}` : null;
  return parent;
}

/** True when `p` equals `root` or lies inside it. */
export function isWithin(root: string, p: string): boolean {
  const r = trimTrailingSep(root);
  const q = trimTrailingSep(p);
  if (q === r) return true;
  const prefix = /[\\/]$/.test(r) ? r : `${r}${separatorOf(r)}`;
  return q.startsWith(prefix);
}

/** The (longest) root folder that contains `p`, or null. */
export function findRootFor(roots: readonly string[], p: string): string | null {
  let best: string | null = null;
  for (const root of roots) {
    if (isWithin(root, p) && (best === null || trimTrailingSep(root).length > trimTrailingSep(best).length)) {
      best = root;
    }
  }
  return best;
}

export interface Crumb {
  name: string;
  path: string;
}

/**
 * Breadcrumb trail from a root folder down to `current`. The root crumb is
 * named after the root's last segment. Returns [] if `current` is outside `root`.
 */
export function buildBreadcrumbs(root: string, current: string): Crumb[] {
  if (!isWithin(root, current)) return [];
  const r = trimTrailingSep(root);
  const crumbs: Crumb[] = [{ name: basename(r), path: r }];
  const rest = trimTrailingSep(current).slice(r.length).split(/[\\/]/).filter(Boolean);
  let acc = r;
  for (const seg of rest) {
    acc = joinPath(acc, seg);
    crumbs.push({ name: seg, path: acc });
  }
  return crumbs;
}

/**
 * Parses persisted root folders defensively: strings only, trimmed of trailing
 * separators, de-duplicated, order kept.
 */
export function normalizeRoots(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  const out: string[] = [];
  for (const v of value) {
    if (typeof v !== 'string' || v.trim() === '') continue;
    const p = trimTrailingSep(v);
    if (!out.includes(p)) out.push(p);
  }
  return out;
}

/** Index to select after moving by `delta` in a list of `length`, clamped. */
export function moveIndex(current: number, delta: number, length: number): number {
  if (length <= 0) return -1;
  if (current < 0) return delta >= 0 ? 0 : length - 1;
  return Math.min(length - 1, Math.max(0, current + delta));
}

/** A printable key with no command modifier: the list's type-to-filter starts with it. */
export function isTypeToFilterKey(e: Pick<KeyboardEvent, 'key' | 'metaKey' | 'ctrlKey' | 'altKey'>): boolean {
  return e.key.length === 1 && !e.metaKey && !e.ctrlKey && !e.altKey && /\S/.test(e.key);
}
