import { cn } from '@/lib/utils';

interface AccuracyMeterProps {
  accuracy: number; // 0-1
  /** Hide the "Accuracy" caption (when the context already says it). */
  compact?: boolean;
  className?: string;
}

/** Live accuracy: a bar plus a tabular percentage, colored by quality band. */
export function AccuracyMeter({ accuracy, compact, className }: AccuracyMeterProps) {
  const pct = Math.round(Math.max(0, Math.min(1, accuracy)) * 100);
  const band = pct >= 85 ? 'good' : pct >= 60 ? 'ok' : 'low';

  return (
    <div className={cn('flex items-center gap-2.5', className)} aria-label={`Accuracy ${pct}%`} role="meter" aria-valuenow={pct} aria-valuemin={0} aria-valuemax={100}>
      {!compact && (
        <span className="text-[11px] font-bold uppercase tracking-[0.08em] text-muted-foreground">Accuracy</span>
      )}
      <div className="h-1.5 w-28 overflow-hidden rounded-full bg-surface-3">
        <div
          className={cn(
            'h-full rounded-full transition-[width,background-color] duration-200',
            band === 'good' && 'bg-note-correct',
            band === 'ok' && 'bg-signal-warn',
            band === 'low' && 'bg-note-wrong',
          )}
          style={{ width: `${pct}%` }}
        />
      </div>
      <span
        className={cn(
          'w-10 text-right font-mono text-sm font-semibold tabular-nums',
          band === 'good' && 'text-note-correct',
          band === 'ok' && 'text-signal-warn',
          band === 'low' && 'text-note-wrong',
        )}
      >
        {pct}%
      </span>
    </div>
  );
}
