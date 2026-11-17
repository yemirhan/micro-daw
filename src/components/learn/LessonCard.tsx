import { ArrowRight, Check, Clock, ListChecks } from 'lucide-react';
import { cn } from '@/lib/utils';
import { DifficultyMeter, ProgressBar } from './kit';
import { CATEGORY_META } from './lessonMeta';
import type { LessonMeta } from '@/types/appMode';

interface LessonCardProps {
  lesson: LessonMeta;
  isCompleted: boolean;
  onClick: () => void;
  /** 0..1 steps done (in-progress lessons show a bar). */
  progress?: number;
  stepCount?: number;
  /** Show the category label (off when the browser already groups by category). */
  showCategory?: boolean;
}

export function LessonCard({ lesson, isCompleted, onClick, progress = 0, stepCount, showCategory = true }: LessonCardProps) {
  const meta = CATEGORY_META[lesson.category];
  const Icon = meta.icon;
  const inProgress = !isCompleted && progress > 0;
  const action = isCompleted ? 'Review' : inProgress ? 'Continue' : 'Start';

  return (
    <button
      type="button"
      onClick={onClick}
      data-lesson-id={lesson.id}
      className={cn(
        'group relative flex min-h-[136px] flex-col overflow-hidden rounded-xl border border-border bg-surface-1 p-4 text-left',
        'shadow-[inset_0_1px_0_var(--hairline)] transition-[border-color,background-color,transform] duration-150',
        'hover:border-hairline-strong hover:bg-surface-2 active:translate-y-px',
        'outline-none focus-visible:ring-2 focus-visible:ring-ring/60',
      )}
    >
      {showCategory && (
        <span className="mb-2 inline-flex items-center gap-1.5 text-[11px] font-semibold uppercase tracking-[0.08em] text-muted-foreground">
          <Icon className="size-3.5" aria-hidden />
          {meta.label}
        </span>
      )}
      <div className="flex items-start justify-between gap-3">
        <h3 className="text-[15px] font-semibold leading-snug text-foreground">{lesson.title}</h3>
        {isCompleted ? (
          <span className="inline-flex shrink-0 items-center gap-1 rounded-full border border-note-correct/40 bg-note-correct/12 px-2 py-0.5 text-[11px] font-semibold text-note-correct">
            <Check className="size-3" aria-hidden />
            Done
          </span>
        ) : inProgress ? (
          <span className="shrink-0 rounded-full border border-primary/40 bg-primary/10 px-2 py-0.5 font-mono text-[10px] font-semibold text-primary">
            {Math.round(progress * 100)}%
          </span>
        ) : null}
      </div>
      <p className="mt-1 line-clamp-2 text-[12.5px] leading-relaxed text-muted-foreground">{lesson.description}</p>

      <div className="mt-auto flex items-center gap-3 pt-4 text-[11px] text-muted-foreground">
        <DifficultyMeter difficulty={lesson.difficulty} />
        <span className="inline-flex items-center gap-1">
          <Clock className="size-3" aria-hidden />
          <span className="font-mono tabular-nums">{lesson.estimatedMinutes}</span> min
        </span>
        {stepCount !== undefined && (
          <span className="inline-flex items-center gap-1">
            <ListChecks className="size-3" aria-hidden />
            <span className="font-mono tabular-nums">{stepCount}</span> steps
          </span>
        )}
        <span
          className={cn(
            'ml-auto inline-flex items-center gap-1 font-semibold transition-colors duration-150',
            inProgress ? 'text-primary' : 'text-muted-foreground/0 group-hover:text-foreground group-focus-visible:text-foreground',
          )}
          aria-hidden
        >
          {action}
          <ArrowRight className="size-3.5" />
        </span>
      </div>

      {inProgress && (
        <ProgressBar value={progress} className="absolute inset-x-0 bottom-0 h-[3px] rounded-none bg-transparent" />
      )}
    </button>
  );
}
