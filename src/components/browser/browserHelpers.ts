/**
 * Pure helpers for the sample browser: listing sort/filter and path
 * manipulation. No DOM, no Node APIs: also imported by the main process
 * (src/main.ts) so the listing order is defined in one place.
 */
import type { FsDirEntry } from '@/types/electron';

/** Audio file extensions the browser lists (lower-case, no dot). */
export const AUDIO_EXTENSIONS: readonly string[] = ['wav', 'aif', 'aiff', 'mp3', 'flac', 'ogg', 'm4a'];

/** Drag-and-drop MIME type carrying `{ path, name }` JSON (see docs/beat-mode-plan.md). */
export const SAMPLE_DRAG_MIME = 'application/x-micro-daw-sample';

export interface SampleDragPayload {
  path: string;
  name: string;
}

/** Lower-case extension of a file name without the dot; '' when there is none. */
export function extOf(name: string): string {
  const dot = name.lastIndexOf('.');
  if (dot <= 0 || dot === name.length - 1) return '';
  return name.slice(dot + 1).toLowerCase();
}

export function isAudioFileName(name: string): boolean {
  return AUDIO_EXTENSIONS.includes(extOf(name));
}

/** Hidden entries (dotfiles such as .DS_Store, ._foo.wav AppleDouble files). */
export function isHiddenName(name: string): boolean {
  return name.startsWith('.');
}

const collator = new Intl.Collator(undefined, { sensitivity: 'base', numeric: true });

/** Case-insensitive comparison: folders first, then files, natural number order. */
export function compareEntries(a: Pick<FsDirEntry, 'name' | 'isDir'>, b: Pick<FsDirEntry, 'name' | 'isDir'>): number {
  if (a.isDir !== b.isDir) return a.isDir ? -1 : 1;
  const byName = collator.compare(a.name, b.name);
  return byName !== 0 ? byName : a.name < b.name ? -1 : a.name > b.name ? 1 : 0;
}

/** Returns a new array sorted folders-first, then by name (case-insensitive). */
export function sortEntries<T extends Pick<FsDirEntry, 'name' | 'isDir'>>(entries: readonly T[]): T[] {
  return [...entries].sort(compareEntries);
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

/** File name without extension ("Kick 01.wav" → "Kick 01"). */
export function displayName(fileName: string): string {
  const dot = fileName.lastIndexOf('.');
  return dot > 0 ? fileName.slice(0, dot) : fileName;
}

// ---------------------------------------------------------------------------
// Paths (POSIX and Windows separators; the renderer has no node:path)
// ---------------------------------------------------------------------------

/** Separator used by a path: '\\' for Windows-style paths, otherwise '/'. */
export function separatorOf(p: string): '/' | '\\' {
  return p.includes('\\') && !p.includes('/') ? '\\' : '/';
}

/** Removes trailing separators, keeping filesystem roots ("/", "C:\\") intact. */
export function trimTrailingSep(p: string): string {
  let out = p;
  while (out.length > 1 && /[\\/]$/.test(out) && !/^[A-Za-z]:[\\/]$/.test(out)) {
    out = out.slice(0, -1);
  }
  return out;
}

/** Last path segment ("/a/b/c" → "c"); the path itself for a filesystem root. */
export function basename(p: string): string {
  const trimmed = trimTrailingSep(p);
  const idx = Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\'));
  const base = idx >= 0 ? trimmed.slice(idx + 1) : trimmed;
  return base || trimmed;
}

export function joinPath(dir: string, name: string): string {
  const sep = separatorOf(dir);
  const base = trimTrailingSep(dir);
  return /[\\/]$/.test(base) ? `${base}${name}` : `${base}${sep}${name}`;
}

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
