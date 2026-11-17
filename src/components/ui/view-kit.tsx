/**
 * Small presentation primitives shared by the full-page views (Samples,
 * Settings). Purely visual: no audio, no app state.
 */
import type { ReactNode } from 'react';
import type { LucideIcon } from 'lucide-react';
import { cn } from '@/lib/utils';

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

interface ViewHeaderProps {
  icon?: LucideIcon;
  title: ReactNode;
  subtitle?: ReactNode;
  /** Right-aligned content (buttons, summary stats). */
  actions?: ReactNode;
  /** Row below the title (filters). */
  children?: ReactNode;
  className?: string;
}

/** Top header for a full view: title + subtitle, actions on the right. */
export function ViewHeader({ icon: Icon, title, subtitle, actions, children, className }: ViewHeaderProps) {
  return (
    <header className={cn('shrink-0 border-b border-border bg-surface-1 px-8 pb-4 pt-5', className)}>
      <div className="flex items-start justify-between gap-6">
        <div className="min-w-0">
          <h1 className="flex items-center gap-2.5 text-xl font-semibold tracking-tight text-foreground">
            {Icon && <Icon className="size-5 text-muted-foreground" aria-hidden />}
            {title}
          </h1>
          {subtitle && <p className="mt-1 text-[13px] text-muted-foreground">{subtitle}</p>}
        </div>
        {actions && <div className="flex shrink-0 items-center gap-3">{actions}</div>}
      </div>
      {children && <div className="mt-4">{children}</div>}
    </header>
  );
}


/** Centered empty state. */
export function EmptyState({ icon: Icon, title, children, className }: { icon?: LucideIcon; title: ReactNode; children?: ReactNode; className?: string }) {
  return (
    <div className={cn('flex flex-col items-center justify-center gap-2 px-6 py-10 text-center', className)}>
      {Icon && (
        <div className="mb-1 flex size-10 items-center justify-center rounded-lg border border-border bg-surface-2">
          <Icon className="size-5 text-muted-foreground" aria-hidden />
        </div>
      )}
      <p className="text-sm font-medium text-foreground">{title}</p>
      {children && <div className="max-w-xs text-xs leading-relaxed text-muted-foreground">{children}</div>}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Controls
// ---------------------------------------------------------------------------

export interface ChipOption<T> {
  value: T;
  label: string;
  count?: number;
  icon?: LucideIcon;
}

/** Single-select filter chips (segmented, wraps). */
export function FilterChips<T extends string | null>({
  options,
  value,
  onChange,
  ariaLabel,
}: {
  options: ChipOption<T>[];
  value: T;
  onChange: (value: T) => void;
  ariaLabel: string;
}) {
  return (
    <div role="radiogroup" aria-label={ariaLabel} className="flex flex-wrap gap-1.5">
      {options.map((opt) => {
        const active = opt.value === value;
        const Icon = opt.icon;
        return (
          <button
            key={opt.label}
            type="button"
            role="radio"
            aria-checked={active}
            onClick={() => onChange(opt.value)}
            className={cn(
              'inline-flex h-8 items-center gap-1.5 rounded-md border px-3 text-xs font-medium transition-colors duration-150',
              'outline-none focus-visible:ring-2 focus-visible:ring-ring/60',
              active
                ? 'border-primary/50 bg-primary/12 text-foreground'
                : 'border-border bg-surface-2 text-muted-foreground hover:border-hairline-strong hover:bg-surface-3 hover:text-foreground',
            )}
          >
            {Icon && <Icon className={cn('size-3.5', active ? 'text-primary' : '')} aria-hidden />}
            {opt.label}
            {opt.count !== undefined && (
              <span className={cn('font-mono text-[10px] tabular-nums', active ? 'text-primary' : 'text-muted-foreground/70')}>
                {opt.count}
              </span>
            )}
          </button>
        );
      })}
    </div>
  );
}

/** Keyboard shortcut hint. */
export function Kbd({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <kbd
      className={cn(
        'inline-flex h-5 min-w-5 items-center justify-center rounded border border-border bg-surface-2 px-1 font-mono text-[10px] font-medium text-muted-foreground',
        className,
      )}
    >
      {children}
    </kbd>
  );
}

// ---------------------------------------------------------------------------
// Readouts
// ---------------------------------------------------------------------------

/** Transport-style LCD readout: tiny label over a mono value. */
export function Readout({ label, children, className }: { label: string; children: ReactNode; className?: string }) {
  return (
    <div
      className={cn(
        'flex flex-col rounded-md border border-border bg-surface-0 px-3 py-1.5 shadow-[inset_0_1px_2px_oklch(0_0_0/0.35)]',
        className,
      )}
    >
      <span className="text-[9.5px] font-bold uppercase tracking-[0.1em] text-muted-foreground">{label}</span>
      <span className="font-mono text-base font-semibold tabular-nums leading-tight text-foreground">{children}</span>
    </div>
  );
}
