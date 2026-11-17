/**
 * Small presentation primitives shared by the non-DAW views (Learn, Practice,
 * Samples, Settings, Dev). Purely visual: no audio, no app state.
 *
 * Built on the "studio instrument" tokens from index.css: surface-0..4,
 * signal-*, note-target / note-correct / note-wrong.
 */
import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { CheckCircle2, CircleAlert, Info, Sparkles, type LucideIcon } from 'lucide-react';
import { cn } from '@/lib/utils';
import type { LessonDifficulty } from '@/types/appMode';

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

/** Uppercase muted section label (the DAW's header style). */
export function SectionLabel({ children, className, right }: { children: ReactNode; className?: string; right?: ReactNode }) {
  return (
    <div className={cn('mb-3 flex items-baseline justify-between gap-3', className)}>
      <h2 className="text-[11px] font-bold uppercase tracking-[0.08em] text-muted-foreground">{children}</h2>
      {right && <div className="text-[11px] text-muted-foreground">{right}</div>}
    </div>
  );
}

/** Raised card surface. */
export function Panel({ className, children, ...rest }: React.HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      className={cn('rounded-xl border border-border bg-surface-1 shadow-[inset_0_1px_0_var(--hairline)]', className)}
      {...rest}
    >
      {children}
    </div>
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
// Progress
// ---------------------------------------------------------------------------

/** Thin horizontal progress bar (0..1). */
export function ProgressBar({
  value,
  tone = 'accent',
  className,
  label,
}: {
  value: number;
  tone?: 'accent' | 'correct' | 'muted';
  className?: string;
  label?: string;
}) {
  const pct = Math.max(0, Math.min(1, value)) * 100;
  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(pct)}
      className={cn('h-1.5 overflow-hidden rounded-full bg-surface-3', className)}
    >
      <div
        className={cn(
          'h-full rounded-full transition-[width] duration-200 ease-out',
          tone === 'accent' && 'bg-primary',
          tone === 'correct' && 'bg-note-correct',
          tone === 'muted' && 'bg-muted-foreground/60',
        )}
        style={{ width: `${pct}%` }}
      />
    </div>
  );
}

/** Segmented step track: one segment per step. */
export function StepTrack({
  total,
  current,
  isDone,
  className,
}: {
  total: number;
  current: number;
  isDone: (index: number) => boolean;
  className?: string;
}) {
  return (
    <div className={cn('flex items-center gap-1', className)} aria-hidden>
      {Array.from({ length: total }, (_, i) => {
        const done = isDone(i);
        const active = i === current;
        return (
          <span
            key={i}
            className={cn(
              'h-1.5 flex-1 rounded-full transition-colors duration-200',
              active ? 'bg-primary' : done ? 'bg-note-correct/70' : 'bg-surface-3',
            )}
          />
        );
      })}
    </div>
  );
}

const DIFFICULTY_LEVEL: Record<LessonDifficulty, number> = { beginner: 1, intermediate: 2, advanced: 3 };

/** Three-bar difficulty meter with label. */
export function DifficultyMeter({ difficulty, className }: { difficulty: LessonDifficulty; className?: string }) {
  const level = DIFFICULTY_LEVEL[difficulty];
  return (
    <span className={cn('inline-flex items-center gap-1.5 text-[11px] capitalize text-muted-foreground', className)}>
      <span className="flex items-end gap-[2px]" aria-hidden>
        {[1, 2, 3].map((i) => (
          <span
            key={i}
            className={cn('w-[3px] rounded-[1px]', i <= level ? 'bg-foreground/75' : 'bg-surface-4')}
            style={{ height: 4 + i * 3 }}
          />
        ))}
      </span>
      {difficulty}
    </span>
  );
}

// ---------------------------------------------------------------------------
// Note feedback
// ---------------------------------------------------------------------------

/** held = a target key being held (good); active = a key held with no right/wrong meaning. */
export type NoteChipState = 'todo' | 'current' | 'done' | 'held' | 'active' | 'wrong';

/** A note / chord / degree token with a learning state. */
export function NoteChip({
  children,
  state,
  size = 'md',
  className,
}: {
  children: ReactNode;
  state: NoteChipState;
  size?: 'sm' | 'md' | 'lg';
  className?: string;
}) {
  return (
    <span
      data-state={state}
      className={cn(
        'inline-flex items-center justify-center rounded-md border font-mono font-semibold tabular-nums transition-colors duration-150',
        size === 'sm' && 'h-7 min-w-9 px-2 text-xs',
        size === 'md' && 'h-9 min-w-11 px-2.5 text-sm',
        size === 'lg' && 'h-12 min-w-16 px-4 text-lg',
        state === 'todo' && 'border-border bg-surface-2 text-muted-foreground',
        state === 'current' && 'border-note-target bg-note-target/15 text-note-target ring-2 ring-note-target/25',
        (state === 'done' || state === 'held') && 'border-note-correct/50 bg-note-correct/15 text-note-correct',
        state === 'active' && 'border-hairline-strong bg-surface-3 text-foreground',
        state === 'wrong' && 'border-note-wrong/60 bg-note-wrong/15 text-note-wrong',
        className,
      )}
    >
      {children}
    </span>
  );
}

export type StatusTone = 'prompt' | 'success' | 'error' | 'info';

const STATUS_ICON: Record<StatusTone, LucideIcon> = {
  prompt: Sparkles,
  success: CheckCircle2,
  error: CircleAlert,
  info: Info,
};

/**
 * One line of live feedback under the instruction: what to do now, or what
 * just happened. Announced to screen readers.
 */
export function StatusLine({
  tone,
  children,
  className,
  testId,
  flashKey,
}: {
  tone: StatusTone;
  children: ReactNode;
  className?: string;
  testId?: string;
  /** Changing this replays the entry animation (e.g. a new wrong note). */
  flashKey?: string | number;
}) {
  const Icon = STATUS_ICON[tone];
  return (
    <div
      role="status"
      aria-live="polite"
      data-testid={testId}
      data-tone={tone}
      key={flashKey}
      className={cn(
        'flex min-h-11 items-center gap-2.5 rounded-lg border px-3.5 py-2 text-[15px] font-medium',
        'animate-in fade-in-0 slide-in-from-bottom-1 duration-150',
        tone === 'prompt' && 'border-note-target/30 bg-note-target/8 text-foreground',
        tone === 'success' && 'border-note-correct/35 bg-note-correct/10 text-note-correct',
        tone === 'error' && 'border-note-wrong/40 bg-note-wrong/10 text-note-wrong',
        tone === 'info' && 'border-border bg-surface-2 text-muted-foreground',
        className,
      )}
    >
      <Icon
        className={cn('size-4.5 shrink-0', tone === 'prompt' && 'text-note-target')}
        aria-hidden
      />
      <span className="min-w-0">{children}</span>
    </div>
  );
}

/**
 * Short-lived "that was wrong" message. `flash(label)` shows it for `ms`;
 * each flash gets a new key so repeated mistakes re-animate.
 */
export function useFlash(ms = 1400) {
  const [flash, setFlash] = useState<{ label: string; key: number } | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const counter = useRef(0);

  const trigger = useCallback(
    (label: string) => {
      counter.current += 1;
      setFlash({ label, key: counter.current });
      if (timer.current) clearTimeout(timer.current);
      timer.current = setTimeout(() => setFlash(null), ms);
    },
    [ms],
  );
  const clear = useCallback(() => {
    if (timer.current) clearTimeout(timer.current);
    setFlash(null);
  }, []);

  useEffect(() => () => {
    if (timer.current) clearTimeout(timer.current);
  }, []);

  return { flash, trigger, clear };
}

// ---------------------------------------------------------------------------
// Stars
// ---------------------------------------------------------------------------

/** 0-3 star rating; filled stars in the accent. */
export function StarRating({ stars, size = 'sm', className }: { stars: number; size?: 'sm' | 'lg'; className?: string }) {
  return (
    <span className={cn('inline-flex items-center gap-0.5', className)} aria-label={`${stars} of 3 stars`} role="img">
      {[1, 2, 3].map((i) => (
        <svg
          key={i}
          viewBox="0 0 24 24"
          aria-hidden
          className={cn(
            size === 'sm' ? 'size-3.5' : 'size-11',
            i <= stars ? 'fill-primary text-primary' : 'fill-surface-3 text-surface-4',
          )}
        >
          <path
            stroke="currentColor"
            strokeWidth={size === 'sm' ? 1.5 : 1}
            strokeLinejoin="round"
            d="M12 2.8l2.75 5.82 6.37.78-4.7 4.38 1.2 6.3L12 17.02l-5.62 3.06 1.2-6.3-4.7-4.38 6.37-.78z"
          />
        </svg>
      ))}
    </span>
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
