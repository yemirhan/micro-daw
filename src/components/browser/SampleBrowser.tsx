import { useCallback, useEffect, useId, useRef, type KeyboardEvent, type ReactNode } from 'react';
import { FolderOpen, FolderPlus, Loader2, RefreshCw, Search, Volume2, VolumeX, X } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Kbd, Tip } from '@/components/ui/tip';
import { EmptyState } from '@/components/ui/view-kit';
import { cn } from '@/lib/utils';
import { useSampleBrowser } from '@/hooks/useSampleBrowser';
import type { FsDirEntry } from '@/types/electron';
import { BrowserBreadcrumbs } from './BrowserBreadcrumbs';
import { BrowserRow } from './BrowserRow';
import { displayName, isTypeToFilterKey } from './browserHelpers';

/**
 * The browser owns its single keys (type-to-filter, Space = preview…): the beat
 * view's performance keys skip `data-keys-scope="browser"` subtrees, and plain
 * keys are kept from bubbling to the app's window-level shortcuts. Modifier
 * chords (⌘Z, ⌘K…) and Escape still reach them; Tab moves focus as usual.
 */
function containKeys(e: KeyboardEvent<HTMLElement>) {
  if (e.metaKey || e.ctrlKey || e.key === 'Escape') return;
  e.stopPropagation();
}

export interface SampleBrowserProps {
  /** Assign a file to the selected pad (Enter / double-click). `name` has no extension. */
  onAssign: (path: string, name: string) => void;
  className?: string;
}

const PAGE = 10;

/**
 * File-system sample browser: user root folders, one level listed at a time.
 * Click = preview, Enter/double-click = assign, rows drag as
 * `application/x-micro-daw-sample`.
 */
export function SampleBrowser({ onAssign, className }: SampleBrowserProps) {
  const b = useSampleBrowser();
  const listRef = useRef<HTMLDivElement>(null);
  const filterRef = useRef<HTMLInputElement>(null);
  const baseId = useId();
  const rowId = (i: number) => `${baseId}-row-${i}`;
  const atRoots = b.currentPath === null;

  // Latest state for the memoised row handlers (rows are memo'd; folders can be large).
  const latest = useRef({ b, onAssign });
  useEffect(() => {
    latest.current = { b, onAssign };
  });

  // Keep the selected row in view.
  useEffect(() => {
    if (b.selectedIndex < 0) return;
    const el = listRef.current?.querySelector<HTMLElement>(`[data-index="${b.selectedIndex}"]`);
    el?.scrollIntoView?.({ block: 'nearest' });
  }, [b.selectedIndex, b.entries]);

  const activate = useCallback((entry: FsDirEntry) => {
    const { b: cur, onAssign: assign } = latest.current;
    if (entry.isDir) cur.navigate(entry.path);
    else assign(entry.path, displayName(entry.name));
  }, []);

  const handleRowClick = useCallback((index: number) => {
    const cur = latest.current.b;
    const entry = cur.entries[index];
    if (!entry) return;
    if (entry.isDir) cur.navigate(entry.path);
    else cur.select(index, { preview: true });
  }, []);

  const handleRowDoubleClick = useCallback(
    (index: number) => {
      const entry = latest.current.b.entries[index];
      if (entry && !entry.isDir) activate(entry);
    },
    [activate],
  );

  /** Keys shared by the list and the filter field. Returns true when handled. */
  const handleNavKey = (e: KeyboardEvent<HTMLElement>, fromInput: boolean): boolean => {
    const sel = b.selected;
    switch (e.key) {
      case 'ArrowDown':
        b.move(1);
        return true;
      case 'ArrowUp':
        b.move(-1);
        return true;
      case 'PageDown':
        b.move(PAGE);
        return true;
      case 'PageUp':
        b.move(-PAGE);
        return true;
      case 'Enter':
        if (sel) activate(sel);
        return true;
      case 'Escape':
        if (b.filter) b.setFilter('');
        else b.stopPreview();
        return true;
    }
    if (fromInput) return false;
    switch (e.key) {
      case 'Home':
        b.select(0, { preview: b.autoPreview });
        return true;
      case 'End':
        b.select(b.entries.length - 1, { preview: b.autoPreview });
        return true;
      case 'ArrowRight':
        if (sel?.isDir) b.navigate(sel.path);
        return true;
      case 'ArrowLeft':
      case 'Backspace':
        b.goUp();
        return true;
      case ' ':
        if (sel && !sel.isDir) b.preview(sel);
        return true;
    }
    if (isTypeToFilterKey(e)) {
      // Type-to-filter: jump into the search field and type the character there.
      b.setFilter(b.filter + e.key);
      filterRef.current?.focus();
      return true;
    }
    return false;
  };

  const onListKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (handleNavKey(e, false)) {
      e.preventDefault();
      e.stopPropagation();
    }
  };

  const onFilterKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (handleNavKey(e, true)) {
      e.preventDefault();
      e.stopPropagation();
    }
  };

  if (!b.available) {
    return (
      <section data-keys-scope="browser" onKeyDown={containKeys} className={cn('flex h-full min-h-0 flex-col bg-surface-1', className)} aria-label="Sample browser">
        <BrowserHeader />
        <EmptyState icon={FolderOpen} title="Browser unavailable">
          <p>Browsing sample folders needs the desktop app.</p>
        </EmptyState>
      </section>
    );
  }

  const noRoots = b.roots.length === 0;

  return (
    <section data-keys-scope="browser" onKeyDown={containKeys} className={cn('@container flex h-full min-h-0 flex-col bg-surface-1', className)} aria-label="Sample browser">
      <BrowserHeader>
        <Tip label={b.autoPreview ? 'Auto-preview on' : 'Auto-preview off'}>
          <Button
            variant="ghost"
            size="icon-xs"
            aria-pressed={b.autoPreview}
            aria-label="Auto-preview while navigating"
            onClick={() => b.setAutoPreview(!b.autoPreview)}
          >
            {b.autoPreview ? <Volume2 /> : <VolumeX />}
          </Button>
        </Tip>
        {!atRoots && (
          <Tip label="Refresh">
            <Button variant="ghost" size="icon-xs" aria-label="Refresh folder" onClick={b.refresh}>
              <RefreshCw />
            </Button>
          </Tip>
        )}
        <Tip label="Add folder">
          <Button variant="ghost" size="icon-xs" aria-label="Add folder" onClick={() => void b.pickRoot()}>
            <FolderPlus />
          </Button>
        </Tip>
      </BrowserHeader>

      {noRoots ? (
        <div className="flex-1 overflow-y-auto">
          <EmptyState icon={FolderOpen} title="Add a sample folder">
            <p>Point the browser at folders with your drums and one-shots. Click to preview, drag onto a pad or press Enter to assign.</p>
            <div className="mt-3 flex flex-col items-center gap-1.5">
              <Button size="sm" variant="outline" className="h-7 gap-1.5 text-xs" onClick={() => void b.pickRoot()}>
                <FolderPlus className="size-3.5" />
                Choose folder…
              </Button>
              {b.suggestions.length > 0 && (
                <div className="mt-1 flex flex-wrap justify-center gap-1">
                  {b.suggestions.map((s) => (
                    <Button
                      key={s.path}
                      size="xs"
                      variant="ghost"
                      className="font-mono text-[11px]"
                      onClick={() => b.addRoot(s.path)}
                      title={`Add ${s.path}`}
                    >
                      + {s.name}
                    </Button>
                  ))}
                </div>
              )}
            </div>
          </EmptyState>
        </div>
      ) : (
        <>
          <BrowserBreadcrumbs crumbs={b.crumbs} onNavigate={b.navigate} />

          <div className="relative shrink-0 border-b border-border p-1.5">
            <Search className="pointer-events-none absolute left-3.5 top-1/2 size-3 -translate-y-1/2 text-muted-foreground" aria-hidden />
            <Input
              ref={filterRef}
              value={b.filter}
              onChange={(e) => b.setFilter(e.target.value)}
              onKeyDown={onFilterKeyDown}
              placeholder={atRoots ? 'Filter folders' : 'Filter this folder'}
              aria-label="Filter"
              aria-controls={`${baseId}-list`}
              className="h-6 pl-6 pr-6 text-[12px]"
            />
            {b.filter && (
              <button
                type="button"
                aria-label="Clear filter"
                onClick={() => {
                  b.setFilter('');
                  listRef.current?.focus();
                }}
                className="absolute right-3 top-1/2 flex size-4 -translate-y-1/2 items-center justify-center rounded-sm text-muted-foreground hover:text-foreground"
              >
                <X className="size-3" />
              </button>
            )}
          </div>

          <div
            ref={listRef}
            id={`${baseId}-list`}
            role="listbox"
            aria-label={atRoots ? 'Sample folders' : `Contents of ${b.crumbs.at(-1)?.name ?? 'folder'}`}
            aria-activedescendant={b.selectedIndex >= 0 ? rowId(b.selectedIndex) : undefined}
            tabIndex={0}
            onKeyDown={onListKeyDown}
            className="min-h-0 flex-1 overflow-y-auto p-1 outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring/50"
          >
            {b.error ? (
              <p className="px-2 py-3 text-[11.5px] text-destructive">{b.error}</p>
            ) : b.loading && b.entries.length === 0 ? (
              <div className="flex items-center gap-1.5 px-2 py-3 text-[11.5px] text-muted-foreground">
                <Loader2 className="size-3 animate-spin" aria-hidden />
                Loading…
              </div>
            ) : b.entries.length === 0 ? (
              <p className="px-2 py-3 text-[11.5px] text-muted-foreground">
                {b.filter ? `Nothing matches "${b.filter}"` : 'No folders or audio files here'}
              </p>
            ) : (
              b.entries.map((entry, i) => (
                <BrowserRow
                  key={entry.path}
                  id={rowId(i)}
                  entry={entry}
                  index={i}
                  selected={i === b.selectedIndex}
                  previewing={entry.path === b.previewPath}
                  isRoot={atRoots}
                  onClick={handleRowClick}
                  onDoubleClick={handleRowDoubleClick}
                  onRemoveRoot={atRoots ? b.removeRoot : undefined}
                />
              ))
            )}
          </div>

          <footer className="flex h-6 shrink-0 items-center gap-2 border-t border-border px-2 font-mono text-[11px] tabular-nums text-muted-foreground">
            <span>
              {b.filter ? `${b.entries.length}/${b.totalCount}` : b.totalCount} {b.totalCount === 1 ? 'item' : 'items'}
            </span>
            <span className="ml-auto hidden items-center gap-1 @[16rem]:flex">
              <Kbd>↵</Kbd> assign <Kbd>␣</Kbd> play
            </span>
          </footer>
        </>
      )}
    </section>
  );
}

function BrowserHeader({ children }: { children?: ReactNode }) {
  return (
    <header className="flex h-8 shrink-0 items-center gap-0.5 border-b border-border pl-3 pr-1">
      <span className="text-[11px] font-bold uppercase tracking-[0.08em] text-muted-foreground">Browser</span>
      <div className="ml-auto flex items-center gap-0.5">{children}</div>
    </header>
  );
}
