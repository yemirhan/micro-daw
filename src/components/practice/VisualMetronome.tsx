import { cn } from '@/lib/utils';

interface VisualMetronomeProps {
  currentBeat: number;
  isRunning: boolean;
  /** Ticks per bar (4 for quarter notes, 8 for eighths). */
  ticksPerBar?: number;
  size?: 'sm' | 'lg';
}

/** Beat lights: the downbeat in the accent, other beats in the foreground. */
export function VisualMetronome({ currentBeat, isRunning, ticksPerBar = 4, size = 'sm' }: VisualMetronomeProps) {
  const tick = currentBeat >= 0 ? currentBeat % ticksPerBar : -1;
  const ticksPerBeat = Math.max(1, ticksPerBar / 4);

  return (
    <div className={cn('flex items-center', size === 'lg' ? 'gap-2' : 'gap-1.5')} aria-hidden>
      {Array.from({ length: ticksPerBar }, (_, i) => {
        const lit = isRunning && tick === i;
        const downbeat = i === 0;
        const onBeat = i % ticksPerBeat === 0;
        return (
          <div
            key={i}
            className={cn(
              'flex items-center justify-center rounded-md border font-mono font-semibold transition-[background-color,border-color,transform] duration-75',
              size === 'lg' ? (onBeat ? 'h-14 w-14 text-lg' : 'h-10 w-10 text-xs') : onBeat ? 'h-5 w-5 text-[10px]' : 'h-3.5 w-3.5 text-[0px]',
              lit
                ? downbeat
                  ? 'scale-105 border-primary bg-primary text-primary-foreground'
                  : 'scale-105 border-foreground/80 bg-foreground/85 text-background'
                : 'border-border bg-surface-2 text-muted-foreground',
            )}
          >
            {size === 'lg' && onBeat ? i / ticksPerBeat + 1 : null}
          </div>
        );
      })}
    </div>
  );
}
