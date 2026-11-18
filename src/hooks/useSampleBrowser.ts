import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { drumEngine } from '@/services/DrumEngine';
import { usePacks } from '@/hooks/usePacks';
import type { FsDirEntry } from '@/types/electron';
import {
  basename,
  buildBreadcrumbs,
  filterEntries,
  findRootFor,
  moveIndex,
  normalizeRoots,
  parentPath,
  trimTrailingSep,
  type Crumb,
} from '@/components/browser/browserHelpers';

const ROOTS_KEY = 'micro-daw:browser-roots';
const LAST_PATH_KEY = 'micro-daw:browser-last-path';
const AUTO_PREVIEW_KEY = 'micro-daw:browser-auto-preview';

function readStorage(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function writeStorage(key: string, value: string | null): void {
  try {
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch {
    // storage unavailable (private mode / quota): browsing still works
  }
}

function loadRoots(): string[] {
  const raw = readStorage(ROOTS_KEY);
  if (!raw) return [];
  try {
    return normalizeRoots(JSON.parse(raw));
  } catch {
    return [];
  }
}

function loadLastPath(roots: string[]): string | null {
  const last = readStorage(LAST_PATH_KEY);
  return last && findRootFor(roots, last) ? last : null;
}

/** Display name of the pinned root holding installed packs (`<userData>/packs`). */
export const INSTALLED_PACKS_ROOT_NAME = 'Installed packs';

function rootEntry(root: string, pinnedRoot: string | null): FsDirEntry {
  return { name: root === pinnedRoot ? INSTALLED_PACKS_ROOT_NAME : basename(root), path: root, isDir: true, ext: '' };
}

/** A well-known folder offered in the empty state. */
export interface FolderSuggestion {
  name: string;
  path: string;
}

const SUGGESTED_HOME_FOLDERS = ['Music', 'Downloads', 'Splice'];

export interface SampleBrowserState {
  /** False outside Electron (plain browser dev / ui-snap). */
  available: boolean;
  /** User root folders (persisted, removable). */
  roots: string[];
  /** Pinned, non-removable "Installed packs" root (`<userData>/packs`) once packs are installed; listed first. */
  pinnedRoot: string | null;
  /** Folder being listed; null = the list of root folders. */
  currentPath: string | null;
  /** Root containing currentPath (null at the root list). */
  currentRoot: string | null;
  crumbs: Crumb[];
  /** Visible (filtered) rows. */
  entries: FsDirEntry[];
  /** Rows in the folder before filtering. */
  totalCount: number;
  loading: boolean;
  error: string | null;
  filter: string;
  selectedIndex: number;
  selected: FsDirEntry | null;
  /** Path of the last previewed file (cleared by stopPreview). */
  previewPath: string | null;
  autoPreview: boolean;
  suggestions: FolderSuggestion[];
}

export interface SampleBrowserActions {
  setFilter: (query: string) => void;
  /** Opens the native folder picker and adds the chosen folder as a root. */
  pickRoot: () => Promise<void>;
  addRoot: (path: string) => void;
  removeRoot: (path: string) => void;
  /** Lists `path` (null = root list). */
  navigate: (path: string | null) => void;
  /** Leaves the current folder (to its parent, or to the root list from a root). */
  goUp: () => void;
  refresh: () => void;
  select: (index: number, opts?: { preview?: boolean }) => void;
  /** Moves the selection; previews files when auto-preview is on. */
  move: (delta: number) => void;
  preview: (entry: FsDirEntry) => void;
  stopPreview: () => void;
  setAutoPreview: (on: boolean) => void;
}

export type UseSampleBrowser = SampleBrowserState & SampleBrowserActions;

export function useSampleBrowser(): UseSampleBrowser {
  const api = typeof window !== 'undefined' ? window.electronAPI : undefined;
  const available = !!api;

  const [roots, setRoots] = useState<string[]>(loadRoots);
  const packs = usePacks();
  const pinnedRoot = packs.packsDir && packs.installed.length > 0 ? trimTrailingSep(packs.packsDir) : null;
  const allRoots = useMemo(() => (pinnedRoot ? [pinnedRoot, ...roots.filter((r) => r !== pinnedRoot)] : roots), [pinnedRoot, roots]);
  const [currentPath, setCurrentPath] = useState<string | null>(() => loadLastPath(loadRoots()));
  const [listing, setListing] = useState<FsDirEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilterState] = useState('');
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [previewPath, setPreviewPath] = useState<string | null>(null);
  const [autoPreview, setAutoPreviewState] = useState(() => readStorage(AUTO_PREVIEW_KEY) !== '0');
  const [suggestions, setSuggestions] = useState<FolderSuggestion[]>([]);

  /** Bumped per listing request so a slow response for an old folder is dropped. */
  const requestRef = useRef(0);

  const load = useCallback(
    async (path: string | null, selectAfter: string | null) => {
      const req = ++requestRef.current;
      if (path === null || !api) {
        setListing([]);
        setLoading(false);
        setError(null);
        setSelectedPath(selectAfter);
        return;
      }
      setLoading(true);
      setError(null);
      try {
        const entries = await api.fsListDir(path);
        if (req !== requestRef.current) return;
        setListing(entries);
        setSelectedPath(selectAfter ?? entries[0]?.path ?? null);
      } catch (err) {
        if (req !== requestRef.current) return;
        setListing([]);
        setSelectedPath(null);
        const msg = err instanceof Error ? err.message.replace(/^Error invoking remote method '[^']+': (Error: )?/, '') : String(err);
        setError(msg || 'Could not read folder');
      } finally {
        if (req === requestRef.current) setLoading(false);
      }
    },
    [api],
  );

  const navigate = useCallback(
    (path: string | null, selectAfter: string | null = null) => {
      setCurrentPath(path);
      setFilterState('');
      writeStorage(LAST_PATH_KEY, path);
      void load(path, selectAfter);
    },
    [load],
  );

  // Initial listing (restored last folder).
  const initialPathRef = useRef(currentPath);
  useEffect(() => {
    void load(initialPathRef.current, null);
  }, [load]);

  // Suggest common folders for the empty state.
  useEffect(() => {
    if (!api || roots.length > 0) return;
    let cancelled = false;
    void (async () => {
      try {
        const home = await api.fsHomeDir();
        const homeEntries = await api.fsListDir(home);
        const found: FolderSuggestion[] = [];
        for (const want of SUGGESTED_HOME_FOLDERS) {
          const hit = homeEntries.find((e) => e.isDir && e.name.toLowerCase() === want.toLowerCase());
          if (hit) found.push({ name: `~/${hit.name}`, path: hit.path });
        }
        if (!cancelled) setSuggestions(found);
      } catch {
        // home not listable: no suggestions
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [api, roots.length]);

  const persistRoots = useCallback((next: string[]) => {
    setRoots(next);
    writeStorage(ROOTS_KEY, JSON.stringify(next));
  }, []);

  const addRoot = useCallback(
    (path: string) => {
      const p = trimTrailingSep(path);
      if (!p) return;
      if (!roots.includes(p)) persistRoots([...roots, p]);
      navigate(p);
    },
    [roots, persistRoots, navigate],
  );

  const pickRoot = useCallback(async () => {
    if (!api) return;
    const picked = await api.fsPickFolder();
    if (picked) addRoot(picked);
  }, [api, addRoot]);

  const removeRoot = useCallback(
    (path: string) => {
      if (path === pinnedRoot) return;
      const next = roots.filter((r) => r !== path);
      persistRoots(next);
      const remaining = pinnedRoot ? [pinnedRoot, ...next] : next;
      if (currentPath !== null && findRootFor(remaining, currentPath) === null) navigate(null);
      else if (currentPath === null && selectedPath === path) setSelectedPath(null);
    },
    [roots, pinnedRoot, persistRoots, currentPath, selectedPath, navigate],
  );

  const currentRoot = currentPath === null ? null : findRootFor(allRoots, currentPath);

  const goUp = useCallback(() => {
    if (currentPath === null) return;
    const atRoot = currentRoot === null || trimTrailingSep(currentRoot) === trimTrailingSep(currentPath);
    const parent = atRoot ? null : parentPath(currentPath);
    // Re-select the folder we just left.
    navigate(parent, atRoot ? (currentRoot ?? currentPath) : currentPath);
  }, [currentPath, currentRoot, navigate]);

  const refresh = useCallback(() => {
    void load(currentPath, selectedPath);
  }, [load, currentPath, selectedPath]);

  const allEntries = useMemo(
    () => (currentPath === null ? allRoots.map((r) => rootEntry(r, pinnedRoot)) : listing),
    [currentPath, allRoots, pinnedRoot, listing],
  );
  const entries = useMemo(() => filterEntries(allEntries, filter), [allEntries, filter]);
  const crumbs = useMemo(
    () => {
      if (currentPath === null || currentRoot === null) return [];
      const trail = buildBreadcrumbs(currentRoot, currentPath);
      if (currentRoot === pinnedRoot && trail[0]) trail[0] = { ...trail[0], name: INSTALLED_PACKS_ROOT_NAME };
      return trail;
    },
    [currentPath, currentRoot, pinnedRoot],
  );

  const foundIndex = selectedPath === null ? -1 : entries.findIndex((e) => e.path === selectedPath);
  // While filtering, fall back to the first match so Enter/Space act on it.
  const selectedIndex = foundIndex >= 0 ? foundIndex : filter.trim() && entries.length > 0 ? 0 : -1;
  const selected = selectedIndex >= 0 ? entries[selectedIndex] : null;

  const preview = useCallback((entry: FsDirEntry) => {
    if (entry.isDir) return;
    setPreviewPath(entry.path);
    drumEngine.preview({ kind: 'file', path: entry.path }).catch((err: unknown) => {
      console.warn('[browser] preview failed', err);
      setPreviewPath((p) => (p === entry.path ? null : p));
    });
  }, []);

  const stopPreview = useCallback(() => {
    drumEngine.stopPreview();
    setPreviewPath(null);
  }, []);

  const select = useCallback(
    (index: number, opts?: { preview?: boolean }) => {
      const entry = entries[index];
      if (!entry) return;
      setSelectedPath(entry.path);
      if (opts?.preview && !entry.isDir) preview(entry);
    },
    [entries, preview],
  );

  const move = useCallback(
    (delta: number) => {
      const next = moveIndex(selectedIndex, delta, entries.length);
      if (next < 0) return;
      select(next, { preview: autoPreview });
    },
    [selectedIndex, entries.length, select, autoPreview],
  );

  const setFilter = useCallback((query: string) => {
    setFilterState(query);
  }, []);

  const setAutoPreview = useCallback((on: boolean) => {
    setAutoPreviewState(on);
    writeStorage(AUTO_PREVIEW_KEY, on ? '1' : '0');
  }, []);

  // Silence any running preview when the browser unmounts.
  useEffect(() => () => drumEngine.stopPreview(), []);

  return {
    available,
    roots,
    pinnedRoot,
    currentPath,
    currentRoot,
    crumbs,
    entries,
    totalCount: allEntries.length,
    loading,
    error,
    filter,
    selectedIndex,
    selected,
    previewPath,
    autoPreview,
    suggestions,
    setFilter,
    pickRoot,
    addRoot,
    removeRoot,
    navigate,
    goUp,
    refresh,
    select,
    move,
    preview,
    stopPreview,
    setAutoPreview,
  };
}
