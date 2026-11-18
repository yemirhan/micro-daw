import { useEffect, useMemo, useState } from 'react';
import { CloudOff, Package, RotateCw, Search, X } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { EmptyState, FilterChips, ViewHeader, type ChipOption } from '@/components/ui/view-kit';
import { ProgressRing } from './InstallButton';
import { PackCard } from './PackCard';
import { PackDetail } from './PackDetail';
import { builtinItem, filterItems, installButtonState, remoteItems, topTags, type PackItem } from './packsModel';
import { usePacksController, type PacksController } from './usePacksController';

/** Chip value meaning "only installed packs". */
const INSTALLED = '__installed';

export interface PacksViewProps {
  /** After "Load on pads": show the Beat view. */
  onOpenBeat: () => void;
}

/** The Packs page (AppMode 'packs'). */
export function PacksView({ onOpenBeat }: PacksViewProps) {
  const controller = usePacksController(onOpenBeat);
  return <PacksBrowser controller={controller} />;
}

function SectionHeader({ title, note, count, children }: { title: string; note?: string; count?: number; children?: React.ReactNode }) {
  return (
    <div className="mb-2.5 flex items-baseline gap-2.5">
      <h2 className="text-[13px] font-semibold text-foreground">{title}</h2>
      {count !== undefined && <span className="text-[11px] text-muted-foreground tabular-nums">{count}</span>}
      {note && <span className="text-[12px] text-muted-foreground">{note}</span>}
      {children}
    </div>
  );
}

const GRID = 'grid grid-cols-[repeat(auto-fill,minmax(320px,1fr))] gap-2.5';

/** Presentational store: header, search + tag chips, built-in + downloadable sections, detail panel. */
export function PacksBrowser({ controller: c }: { controller: PacksController }) {
  const [query, setQuery] = useState('');
  const [tag, setTag] = useState<string | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);

  const installedIds = useMemo(() => new Set(c.installed.map((p) => p.id)), [c.installed]);
  const builtins = useMemo(() => c.builtins.map(builtinItem), [c.builtins]);
  const remotes = useMemo(() => remoteItems(c.catalog?.packs ?? null, c.installed), [c.catalog, c.installed]);
  const all = useMemo(() => [...builtins, ...remotes], [builtins, remotes]);

  const tagOptions = useMemo<ChipOption<string | null>[]>(() => {
    const opts: ChipOption<string | null>[] = [{ value: null, label: 'All', count: all.length }];
    if (installedIds.size > 0) opts.push({ value: INSTALLED, label: 'Installed', count: installedIds.size });
    for (const t of topTags(all, 9)) opts.push({ value: t.tag, label: t.tag, count: t.count });
    return opts;
  }, [all, installedIds]);

  const visible = (items: PackItem[]) =>
    tag === INSTALLED
      ? filterItems(items, query, null).filter((i) => i.kind === 'remote' && installedIds.has(i.id))
      : filterItems(items, query, tag);
  const shownBuiltins = visible(builtins);
  const shownRemotes = visible(remotes);
  const filtering = query.trim() !== '' || tag !== null;

  const selected = all.find((i) => i.id === selectedId) ?? null;
  const stopKitPreview = c.stopKitPreview;

  // Esc closes the detail panel (unless typing in the search box or a dialog owns the keyboard).
  useEffect(() => {
    if (!selectedId) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape' || e.defaultPrevented) return;
      if (document.querySelector('[role="dialog"][data-state="open"]')) return;
      if (e.target instanceof HTMLInputElement) return;
      stopKitPreview();
      setSelectedId(null);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [selectedId, stopKitPreview]);

  const card = (item: PackItem) => (
    <PackCard
      key={item.id}
      item={item}
      state={installButtonState(item, c.stateOf(item.id), installedIds.has(item.id))}
      selected={item.id === selectedId}
      onSelect={() => {
        if (c.previewing && !c.previewing.startsWith(`${item.id}/`)) c.stopKitPreview();
        setSelectedId((cur) => (cur === item.id ? null : item.id));
      }}
      onInstall={() => c.install(item.id)}
      onCancel={() => c.cancel(item.id)}
    />
  );

  const clearFilters = () => {
    setQuery('');
    setTag(null);
  };

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden bg-background">
      <ViewHeader
        icon={Package}
        title="Packs"
        subtitle="Drum kits for your pads. Built-in machines load instantly; free sample packs download once."
        className="px-6 pt-4 pb-3"
        actions={
          <div className="relative w-[260px]">
            <Search className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground" aria-hidden />
            <input
              type="search"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Escape' && query) {
                  e.preventDefault();
                  setQuery('');
                }
              }}
              placeholder="Search packs, sounds, styles"
              aria-label="Search packs"
              className="h-8 w-full rounded-md border border-border bg-surface-0 pr-7 pl-8 text-[13px] shadow-[inset_0_1px_2px_oklch(0_0_0/0.3)] outline-none placeholder:text-muted-foreground hover:border-input focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/30 [&::-webkit-search-cancel-button]:hidden"
            />
            {query && (
              <button
                type="button"
                aria-label="Clear search"
                onClick={() => setQuery('')}
                className="absolute top-1/2 right-1.5 flex size-5 -translate-y-1/2 items-center justify-center rounded-sm text-muted-foreground hover:text-foreground"
              >
                <X className="size-3.5" />
              </button>
            )}
          </div>
        }
      >
        <FilterChips ariaLabel="Filter by tag" options={tagOptions} value={tag} onChange={setTag} />
      </ViewHeader>

      <div className="flex min-h-0 flex-1">
        <div className="min-w-0 flex-1 overflow-y-auto">
          <div className="flex max-w-[1400px] flex-col gap-7 px-6 pt-5 pb-10">
            {(shownBuiltins.length > 0 || !filtering) && (
              <section aria-label="Built-in kits">
                <SectionHeader title="Built-in kits" note="Synthesized in the app, no download" />
                <div className={GRID}>{shownBuiltins.map(card)}</div>
              </section>
            )}

            <section aria-label="Downloadable packs">
              <SectionHeader
                title="Downloadable packs"
                count={c.catalogStatus === 'ready' ? shownRemotes.length : undefined}
                note="Free recordings with open licences"
              >
                {c.catalogStatus === 'loading' && remotes.length > 0 && (
                  <span className="ml-auto inline-flex items-center gap-1.5 self-center text-[11px] text-muted-foreground" role="status">
                    <ProgressRing progress={null} size={13} label="Loading catalog" />
                    Updating catalog
                  </span>
                )}
              </SectionHeader>
              {c.catalogStatus === 'loading' && remotes.length === 0 ? (
                <div className={GRID} aria-busy="true" aria-label="Loading packs">
                  {Array.from({ length: 4 }, (_, i) => (
                    <div key={i} className="flex gap-3 rounded-lg border border-border bg-surface-1 p-2.5">
                      <Skeleton className="size-[68px] rounded-md" />
                      <div className="flex flex-1 flex-col gap-2 pt-1">
                        <Skeleton className="h-3.5 w-2/3" />
                        <Skeleton className="h-3 w-full" />
                        <Skeleton className="mt-auto h-3 w-1/3" />
                      </div>
                    </div>
                  ))}
                </div>
              ) : c.catalogStatus === 'error' && remotes.length === 0 ? (
                <div className="rounded-lg border border-dashed border-border bg-surface-1/50">
                  <EmptyState icon={CloudOff} title="Can't reach the pack catalog">
                    <p>{c.catalogError || 'Check your internet connection.'} Built-in kits still work offline.</p>
                    <Button size="sm" variant="outline" className="mt-3" onClick={c.retryCatalog}>
                      <RotateCw aria-hidden />
                      Try again
                    </Button>
                  </EmptyState>
                </div>
              ) : shownRemotes.length > 0 ? (
                <>
                  {c.catalogStatus === 'error' && (
                    <div className="mb-2.5 flex items-center gap-2 rounded-md border border-border bg-surface-1 px-3 py-1.5 text-[12px] text-muted-foreground">
                      <CloudOff className="size-3.5" aria-hidden />
                      Offline: showing installed packs only.
                      <Button size="xs" variant="ghost" className="ml-auto" onClick={c.retryCatalog}>
                        <RotateCw aria-hidden />
                        Try again
                      </Button>
                    </div>
                  )}
                  <div className={GRID}>{shownRemotes.map(card)}</div>
                </>
              ) : filtering ? (
                <div className="rounded-lg border border-dashed border-border bg-surface-1/50">
                  <EmptyState icon={Search} title={query.trim() ? `No packs match "${query.trim()}"` : 'No packs with this tag'}>
                    <Button size="sm" variant="outline" className="mt-2" onClick={clearFilters}>
                      Show all packs
                    </Button>
                  </EmptyState>
                </div>
              ) : (
                <div className="rounded-lg border border-dashed border-border bg-surface-1/50">
                  <EmptyState icon={Package} title="No downloadable packs yet">
                    <p>The catalog is empty right now. Built-in kits are ready to play above.</p>
                  </EmptyState>
                </div>
              )}
            </section>
          </div>
        </div>

        {selected && (
          <PackDetail
            key={selected.id}
            item={selected}
            state={installButtonState(selected, c.stateOf(selected.id), installedIds.has(selected.id))}
            installed={installedIds.has(selected.id)}
            updateAvailable={c.hasUpdate(selected.id)}
            previewing={c.previewing}
            onClose={() => {
              c.stopKitPreview();
              setSelectedId(null);
            }}
            onInstall={() => c.install(selected.id)}
            onCancel={() => c.cancel(selected.id)}
            onUninstall={() => {
              c.stopKitPreview();
              c.uninstall(selected.id);
            }}
            uninstallError={c.uninstallError(selected.id)}
            onDismissUninstallError={() => c.dismissUninstallError(selected.id)}
            soundPreviewing={c.soundPreviewing}
            soundError={c.soundError}
            onPreviewKit={(kitId) => c.previewKit(selected, kitId)}
            onStopPreview={c.stopKitPreview}
            kitPreviewError={c.kitPreviewError}
            onLoadKit={(kitId) => {
              const kit = c.kitFor(selected, kitId);
              if (kit) c.loadKit(kit);
            }}
            onGetAndLoad={(kitId) => c.getAndLoad(selected, kitId)}
            pendingLoad={c.pendingLoad}
            usedByProject={installedIds.has(selected.id) && c.usedByProject(selected.id)}
            onPlaySound={(sound) => c.playSound(selected, sound)}
          />
        )}
      </div>
    </div>
  );
}
