import { useEffect, useRef } from 'react';
import { ArrowLeft, RotateCcw } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';
import { StarRating } from '@/components/learn/kit';

interface ScoreDisplayProps {
  accuracy: number;
  stars: 0 | 1 | 2 | 3;
  onRetry: () => void;
  onBack: () => void;
  /** Previous best accuracy (0..1) before this attempt, if any. */
  previousBest?: number;
}

const MESSAGES: Record<number, { title: string; body: string }> = {
  3: { title: 'Excellent!', body: 'Three stars. That was clean.' },
  2: { title: 'Great job!', body: 'Close to perfect. One more run for the third star?' },
  1: { title: 'Good start!', body: 'Keep at it: accuracy earns more stars.' },
  0: { title: 'Keep going!', body: 'Slow down and aim for the right notes first.' },
};

export function ScoreDisplay({ accuracy, stars, onRetry, onBack, previousBest }: ScoreDisplayProps) {
  const pct = Math.round(accuracy * 100);
  const msg = MESSAGES[stars];
  const isBest = previousBest !== undefined && accuracy > previousBest;
  const retryRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    retryRef.current?.focus({ preventScroll: true });
  }, []);

  return (
    <div className="flex w-full max-w-md flex-col items-center rounded-2xl border border-border bg-surface-1 px-10 py-10 text-center shadow-[inset_0_1px_0_var(--hairline)] animate-in fade-in-0 zoom-in-[0.98] duration-200">
      <p role="status" className="text-[11px] font-bold uppercase tracking-[0.14em] text-muted-foreground">
        Exercise complete
      </p>

      <StarRating stars={stars} size="lg" className="mt-5 gap-2" />

      <div
        className={cn(
          'mt-5 font-mono text-6xl font-semibold leading-none tabular-nums tracking-tight',
          pct >= 85 ? 'text-note-correct' : pct >= 60 ? 'text-signal-warn' : 'text-note-wrong',
        )}
      >
        {pct}%
      </div>
      <p className="mt-1 text-[11px] font-semibold uppercase tracking-[0.1em] text-muted-foreground">accuracy</p>

      <h2 className="mt-5 text-xl font-semibold text-foreground">{msg.title}</h2>
      <p className="mt-1 text-sm text-muted-foreground">{msg.body}</p>
      {isBest && (
        <p className="mt-3 rounded-full border border-primary/40 bg-primary/10 px-3 py-1 text-xs font-semibold text-primary">
          New personal best
        </p>
      )}

      <div className="mt-8 flex w-full gap-3">
        <Button ref={retryRef} variant="outline" className="h-10 flex-1" onClick={onRetry}>
          <RotateCcw className="size-4" />
          Try again
        </Button>
        <Button className="h-10 flex-1 font-semibold" onClick={onBack}>
          <ArrowLeft className="size-4" />
          Back to exercises
        </Button>
      </div>
    </div>
  );
}
