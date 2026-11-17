import { useEffect, useRef } from 'react';
import { ArrowLeft, ArrowRight, Check, Clock, ListChecks, RotateCcw } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Kbd, Panel, ProgressBar } from './kit';
import { CATEGORY_META } from './lessonMeta';
import type { Lesson } from '@/types/appMode';

interface LessonCompleteProps {
  lesson: Lesson;
  nextLesson?: Lesson;
  categoryDone: number;
  categoryTotal: number;
  onNextLesson?: () => void;
  onReplay: () => void;
  onBack: () => void;
}

/** Full-view "lesson complete" screen with the obvious next action. */
export function LessonComplete({
  lesson,
  nextLesson,
  categoryDone,
  categoryTotal,
  onNextLesson,
  onReplay,
  onBack,
}: LessonCompleteProps) {
  const meta = CATEGORY_META[lesson.category];
  const primaryRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    primaryRef.current?.focus({ preventScroll: true });
  }, []);

  return (
    <div
      data-testid="lesson-complete-banner"
      className="flex flex-1 flex-col items-center justify-center overflow-y-auto bg-background px-8 py-10"
    >
      <div className="flex w-full max-w-lg flex-col items-center text-center">
        <div className="relative mb-6 animate-in zoom-in-75 fade-in-0 duration-200">
          <div className="absolute inset-0 rounded-full bg-note-correct/15 blur-xl" aria-hidden />
          <div className="relative flex size-20 items-center justify-center rounded-full border-2 border-note-correct/60 bg-note-correct/12">
            <Check className="size-10 text-note-correct" strokeWidth={2.5} aria-hidden />
          </div>
        </div>

        <p role="status" className="text-[11px] font-bold uppercase tracking-[0.14em] text-note-correct">
          Lesson complete
        </p>
        <h2 className="mt-2 text-3xl font-semibold tracking-tight text-foreground">{lesson.title}</h2>
        <p className="mt-2 text-sm text-muted-foreground">
          Nicely done. Every step is finished and your progress is saved.
        </p>

        <div className="mt-6 flex items-center gap-5 text-[13px] text-muted-foreground">
          <span className="inline-flex items-center gap-1.5">
            <ListChecks className="size-4" aria-hidden />
            <span className="font-mono font-semibold tabular-nums text-foreground">{lesson.steps.length}</span> steps
          </span>
          <span className="inline-flex items-center gap-1.5">
            <Clock className="size-4" aria-hidden />
            <span className="font-mono font-semibold tabular-nums text-foreground">~{lesson.estimatedMinutes}</span> min
          </span>
          <span className="inline-flex items-center gap-1.5">
            <meta.icon className="size-4" aria-hidden />
            {meta.label}
          </span>
        </div>

        <Panel className="mt-6 w-full px-5 py-4 text-left">
          <div className="mb-2 flex items-baseline justify-between text-xs">
            <span className="font-semibold text-foreground">{meta.label} path</span>
            <span className="text-muted-foreground">
              <span className="font-mono font-semibold tabular-nums text-foreground">{categoryDone}</span>
              <span className="font-mono tabular-nums"> / {categoryTotal}</span> lessons
            </span>
          </div>
          <ProgressBar value={categoryTotal ? categoryDone / categoryTotal : 0} tone="correct" label={`${meta.label} lessons completed`} />
        </Panel>

        {nextLesson && onNextLesson ? (
          <button
            ref={primaryRef}
            type="button"
            onClick={onNextLesson}
            className="group mt-4 flex w-full items-center gap-4 rounded-xl border border-primary/50 bg-primary/10 px-5 py-4 text-left outline-none transition-colors duration-150 hover:bg-primary/16 focus-visible:ring-2 focus-visible:ring-ring/70"
          >
            <div className="min-w-0 flex-1">
              <span className="text-[11px] font-bold uppercase tracking-[0.1em] text-primary">Up next</span>
              <span className="mt-0.5 block truncate text-base font-semibold text-foreground">{nextLesson.title}</span>
              <span className="block truncate text-xs text-muted-foreground">
                {CATEGORY_META[nextLesson.category].label} · {nextLesson.estimatedMinutes} min · {nextLesson.difficulty}
              </span>
            </div>
            <span className="inline-flex h-9 shrink-0 items-center gap-1.5 rounded-md bg-primary px-4 text-sm font-semibold text-primary-foreground">
              Next lesson
              <ArrowRight className="size-4 transition-transform duration-150 group-hover:translate-x-0.5" aria-hidden />
            </span>
          </button>
        ) : (
          <p className="mt-4 text-sm text-muted-foreground">You have finished every lesson. Time to make some music in the DAW.</p>
        )}

        <div className="mt-4 flex items-center gap-2">
          <Button variant="ghost" size="sm" onClick={onBack} ref={nextLesson ? undefined : primaryRef}>
            <ArrowLeft className="size-4" aria-hidden />
            Back to lessons
          </Button>
          <Button variant="ghost" size="sm" onClick={onReplay}>
            <RotateCcw className="size-4" aria-hidden />
            Replay lesson
          </Button>
        </div>
        {nextLesson && (
          <p className="mt-3 flex items-center gap-1.5 text-[11px] text-muted-foreground">
            <Kbd>Enter</Kbd> starts the next lesson
          </p>
        )}
      </div>
    </div>
  );
}
