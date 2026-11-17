import { Check, Download, RotateCw, X, Zap } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Tip } from '@/components/ui/tip';
import { cn } from '@/lib/utils';
import type { InstallButtonState } from './packsModel';

/** Circular progress (indeterminate spin when progress is null). */
export function ProgressRing({ progress, size = 18, label = 'Download progress', className }: { progress: number | null; size?: number; label?: string; className?: string }) {
  const r = (size - 3) / 2;
  const c = 2 * Math.PI * r;
  return (
    <svg
      width={size}
      height={size}
      viewBox={`0 0 ${size} ${size}`}
      className={cn('shrink-0 -rotate-90', progress == null && 'animate-spin motion-reduce:animate-none', className)}
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={progress == null ? undefined : Math.round(progress * 100)}
      aria-label={label}
    >
      <circle cx={size / 2} cy={size / 2} r={r} fill="none" strokeWidth={2.5} className="stroke-surface-4" />
      <circle
        cx={size / 2}
        cy={size / 2}
        r={r}
        fill="none"
        strokeWidth={2.5}
        strokeLinecap="round"
        className="stroke-primary transition-[stroke-dashoffset] duration-200"
        strokeDasharray={c}
        strokeDashoffset={progress == null ? c * 0.7 : c * (1 - progress)}
      />
    </svg>
  );
}

interface InstallButtonProps {
  state: InstallButtonState;
  name: string;
  onInstall: () => void;
  onCancel: () => void;
  size?: 'sm' | 'md';
}

/** Get / progress + Cancel / Installed / Retry, for one pack. Clicks never bubble to the card. */
export function InstallButton({ state, name, onInstall, onCancel, size = 'sm' }: InstallButtonProps) {
  const h = size === 'sm' ? 'h-7' : 'h-8';
  const stop = (fn: () => void) => (e: React.MouseEvent) => {
    e.stopPropagation();
    fn();
  };
  switch (state.kind) {
    case 'builtin':
      return (
        <span className={cn('inline-flex items-center gap-1 rounded-md px-1.5 text-[11px] font-medium text-muted-foreground', h)}>
          <Zap className="size-3.5 text-primary" aria-hidden />
          Ready
        </span>
      );
    case 'get':
      return (
        <Button size="sm" className={h} onClick={stop(onInstall)} aria-label={`Get ${name}`}>
          <Download aria-hidden />
          Get
        </Button>
      );
    case 'downloading':
      return (
        <span className={cn('inline-flex items-center gap-1.5 rounded-md border border-border bg-surface-0 pl-1.5', h)}>
          <ProgressRing progress={state.progress} />
          <span className="min-w-[30px] font-mono text-[11px] tabular-nums text-foreground" aria-live="polite">
            {state.label}
          </span>
          <Tip label="Cancel download">
            <button
              type="button"
              onClick={stop(onCancel)}
              aria-label={`Cancel downloading ${name}`}
              className="flex h-full items-center rounded-r-md border-l border-border px-1.5 text-muted-foreground outline-none hover:bg-surface-3 hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/60"
            >
              <X className="size-3.5" />
            </button>
          </Tip>
        </span>
      );
    case 'installed':
      return (
        <span className={cn('inline-flex items-center gap-1 rounded-md px-1.5 text-[11px] font-medium text-signal-play', h)}>
          <Check className="size-3.5" aria-hidden />
          Installed
        </span>
      );
    case 'error':
      return (
        <Tip label={state.message || 'Download failed'}>
          <Button
            size="sm"
            variant="outline"
            className={cn('border-signal-rec/40 text-signal-rec hover:text-signal-rec', h)}
            onClick={stop(onInstall)}
            aria-label={`Retry downloading ${name}`}
          >
            <RotateCw aria-hidden />
            Retry
          </Button>
        </Tip>
      );
  }
}
