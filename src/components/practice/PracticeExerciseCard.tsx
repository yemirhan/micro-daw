import { ArrowRight } from 'lucide-react';
import { cn } from '@/lib/utils';
import { DifficultyMeter, StarRating } from '@/components/learn/kit';
import { ACTIVITY_LABEL } from './practiceMeta';
import type { PracticeExercise, PracticeScore } from '@/types/appMode';

interface PracticeExerciseCardProps {
  exercise: PracticeExercise;
  score?: PracticeScore;
  onClick: () => void;
}

export function PracticeExerciseCard({ exercise, score, onClick }: PracticeExerciseCardProps) {
  const played = !!score && score.attempts > 0;
  const bpm = exercise.config.bpm;

  return (
    <button
      type="button"
      onClick={onClick}
      data-exercise-id={exercise.id}
      className={cn(
        'group flex min-h-[124px] flex-col rounded-xl border border-border bg-surface-1 p-4 text-left',
        'shadow-[inset_0_1px_0_var(--hairline)] transition-[border-color,background-color,transform] duration-150',
        'hover:border-hairline-strong hover:bg-surface-2 active:translate-y-px',
        'outline-none focus-visible:ring-2 focus-visible:ring-ring/60',
      )}
    >
      <div className="flex items-start justify-between gap-3">
        <h3 className="text-[15px] font-semibold leading-snug text-foreground">{exercise.title}</h3>
        <StarRating stars={score?.bestStars ?? 0} className={cn('shrink-0 pt-0.5', !played && 'opacity-60')} />
      </div>
      <p className="mt-1 line-clamp-2 text-[12.5px] leading-relaxed text-muted-foreground">{exercise.description}</p>

      <div className="mt-auto flex items-center gap-3 pt-4 text-[11px] text-muted-foreground">
        <DifficultyMeter difficulty={exercise.difficulty} />
        <span className="rounded border border-border bg-surface-2 px-1.5 py-px text-[10px] font-semibold uppercase tracking-[0.06em]">
          {ACTIVITY_LABEL[exercise.activity]}
        </span>
        {bpm !== undefined && (
          <span>
            <span className="font-mono tabular-nums">{bpm}</span> BPM
          </span>
        )}
        {played ? (
          <span className="ml-auto">
            Best <span className="font-mono font-semibold tabular-nums text-foreground">{Math.round(score.bestAccuracy * 100)}%</span>
            <span className="text-muted-foreground/70"> · {score.attempts}×</span>
          </span>
        ) : (
          <span
            className="ml-auto inline-flex items-center gap-1 font-semibold text-transparent transition-colors duration-150 group-hover:text-foreground group-focus-visible:text-foreground"
            aria-hidden
          >
            Start
            <ArrowRight className="size-3.5" />
          </span>
        )}
      </div>
    </button>
  );
}
