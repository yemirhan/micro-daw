import { ChevronRight, HardDrive } from 'lucide-react';
import { cn } from '@/lib/utils';
import type { Crumb } from './browserHelpers';

interface BrowserBreadcrumbsProps {
  crumbs: Crumb[];
  onNavigate: (path: string | null) => void;
}

/** "Folders › Root › Sub › Current" trail; the first item returns to the root list. */
export function BrowserBreadcrumbs({ crumbs, onNavigate }: BrowserBreadcrumbsProps) {
  const atRoots = crumbs.length === 0;
  return (
    <nav
      aria-label="Folder path"
      className="flex h-7 shrink-0 items-center gap-0.5 overflow-hidden border-b border-border px-1.5 text-[11px]"
    >
      <button
        type="button"
        onClick={() => onNavigate(null)}
        aria-current={atRoots ? 'page' : undefined}
        title="All folders"
        className={cn(
          'flex h-5 shrink-0 items-center gap-1 rounded-sm px-1 outline-none hover:bg-surface-3 focus-visible:ring-2 focus-visible:ring-ring/60',
          atRoots ? 'text-foreground' : 'text-muted-foreground',
        )}
      >
        <HardDrive className="size-3" aria-hidden />
        {atRoots && <span>Folders</span>}
      </button>
      {crumbs.map((c, i) => {
        const last = i === crumbs.length - 1;
        return (
          <span key={c.path} className={cn('flex min-w-0 items-center gap-0.5', last ? 'shrink' : 'shrink-[2]')}>
            <ChevronRight className="size-3 shrink-0 text-muted-foreground/50" aria-hidden />
            <button
              type="button"
              onClick={() => onNavigate(c.path)}
              aria-current={last ? 'page' : undefined}
              title={c.path}
              className={cn(
                'h-5 min-w-0 truncate rounded-sm px-1 outline-none hover:bg-surface-3 focus-visible:ring-2 focus-visible:ring-ring/60',
                last ? 'font-medium text-foreground' : 'text-muted-foreground',
              )}
            >
              {c.name}
            </button>
          </span>
        );
      })}
    </nav>
  );
}
