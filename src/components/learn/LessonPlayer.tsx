import { useEffect, useLayoutEffect, useRef } from 'react';
import { ArrowLeft, ArrowRight, Check, Flag, X } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';
import { LessonProgress as ProgressBar } from './LessonProgress';
import { LessonStepRenderer } from './LessonStepRenderer';
import { Kbd } from './kit';
import { CATEGORY_META, STEP_KIND } from './lessonMeta';
import type { Lesson, LessonStep } from '@/types/appMode';
import type { ActiveNote } from '@/hooks/useMidiNotes';
import type { ChordInfo } from '@/types/music';
import type { DrumPadId } from '@/types/drums';

interface LessonPlayerProps {
  lesson: Lesson;
  currentStep: LessonStep;
  currentStepIndex: number;
  totalSteps: number;
  stepCompleted: boolean;
  /** Step condition met; waiting for the player to release the keys. */
  pendingRelease?: boolean;
  /** Notes/degrees matched so far (sequence + scale steps). */
  sequenceIndex?: number;
  /** Steps already finished in a previous (unfinished) run, for the step track. */
  completedSteps?: number[];
  activeNotes: Map<number, ActiveNote>;
  onNoteOn: (note: number, velocity: number) => void;
  onNoteOff: (note: number) => void;
  activePads: Set<DrumPadId>;
  onDrumHit: (padId: DrumPadId, velocity: number) => void;
  detectedChord: ChordInfo | null;
  onNext: () => void;
  onPrev: () => void;
  onExit: () => void;
  onQuizAnswer: (correct: boolean) => void;
}

function isTypingTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return (
    target.isContentEditable ||
    target instanceof HTMLInputElement ||
    target instanceof HTMLTextAreaElement ||
    target instanceof HTMLSelectElement ||
    target instanceof HTMLButtonElement ||
    target.getAttribute('role') === 'button'
  );
}

export function LessonPlayer({
  lesson,
  currentStep,
  currentStepIndex,
  totalSteps,
  stepCompleted,
  pendingRelease = false,
  sequenceIndex = 0,
  completedSteps,
  activeNotes,
  onNoteOn,
  onNoteOff,
  activePads,
  onDrumHit,
  detectedChord,
  onNext,
  onPrev,
  onExit,
  onQuizAnswer,
}: LessonPlayerProps) {
  const isLast = currentStepIndex === totalSteps - 1;
  const category = CATEGORY_META[lesson.category];
  const kind = STEP_KIND[currentStep.type];
  const KindIcon = kind.icon;
  const isInfo = currentStep.type === 'info';

  const scrollRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (scrollRef.current) scrollRef.current.scrollTop = 0;
  }, [currentStepIndex]);

  // Enter = Next / Finish once the step is done (unless a control has focus,
  // in which case Enter already activates that control).
  const nextRef = useRef({ onNext, stepCompleted });
  useLayoutEffect(() => {
    nextRef.current = { onNext, stepCompleted };
  });
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Enter' || e.defaultPrevented || e.repeat || e.metaKey || e.ctrlKey || e.altKey) return;
      if (isTypingTarget(e.target)) return;
      if (!nextRef.current.stepCompleted) return;
      e.preventDefault();
      nextRef.current.onNext();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  const doneSet = new Set(completedSteps ?? []);
  const isDone = (i: number) => i < currentStepIndex || doneSet.has(i) || (i === currentStepIndex && stepCompleted);

  return (
    <div className="flex flex-1 flex-col overflow-hidden bg-background">
      {/* Header */}
      <header className="flex h-14 shrink-0 items-center gap-4 border-b border-border bg-surface-1 px-4">
        <Button
          variant="ghost"
          size="icon-sm"
          className="text-muted-foreground hover:text-foreground"
          onClick={onExit}
          title="Exit lesson"
          aria-label="Exit lesson"
        >
          <X className="size-4" />
        </Button>
        <div className="h-6 w-px bg-border" aria-hidden />
        <div className="flex min-w-0 items-center gap-2.5">
          <span className="flex size-8 shrink-0 items-center justify-center rounded-md border border-border bg-surface-2">
            <category.icon className="size-4 text-muted-foreground" aria-hidden />
          </span>
          <div className="min-w-0">
            <p className="text-[10px] font-bold uppercase leading-none tracking-[0.1em] text-muted-foreground">
              {category.label} lesson
            </p>
            <h2 className="mt-1 truncate text-sm font-semibold leading-none text-foreground">{lesson.title}</h2>
          </div>
        </div>
        <div className="ml-auto w-[min(360px,36vw)]">
          <ProgressBar current={currentStepIndex} total={totalSteps} isDone={isDone} />
        </div>
      </header>

      {/* Step content */}
      <div ref={scrollRef} className="flex-1 overflow-y-auto">
        <div
          key={currentStepIndex}
          className="mx-auto flex w-full max-w-[1080px] flex-col px-10 pb-10 pt-10 animate-in fade-in-0 slide-in-from-bottom-1 duration-200"
        >
          <p className="inline-flex items-center gap-1.5 text-[11px] font-bold uppercase tracking-[0.12em] text-primary">
            <KindIcon className="size-3.5" aria-hidden />
            {kind.label}
            <span className="font-mono font-medium text-muted-foreground">
              · {currentStepIndex + 1}/{totalSteps}
            </span>
          </p>
          <h3 className="mt-2 text-[28px] font-semibold leading-tight tracking-tight text-foreground">
            {currentStep.title}
          </h3>

          <div className="mt-5">
            <LessonStepRenderer
              key={currentStepIndex}
              step={currentStep}
              completed={stepCompleted}
              pendingRelease={pendingRelease}
              sequenceIndex={sequenceIndex}
              activeNotes={activeNotes}
              onNoteOn={onNoteOn}
              onNoteOff={onNoteOff}
              activePads={activePads}
              onDrumHit={onDrumHit}
              detectedChord={detectedChord}
              onQuizAnswer={onQuizAnswer}
            />
          </div>
        </div>
      </div>

      {/* Navigation footer */}
      <footer className="flex h-16 shrink-0 items-center gap-4 border-t border-border bg-surface-1 px-6">
        <Button variant="ghost" onClick={onPrev} disabled={currentStepIndex === 0} className="text-muted-foreground">
          <ArrowLeft className="size-4" />
          Back
        </Button>

        <div className="flex flex-1 items-center justify-center text-xs text-muted-foreground">
          {stepCompleted ? (
            <span className="inline-flex items-center gap-2">
              {!isInfo && (
                <span className="inline-flex items-center gap-1 font-semibold text-note-correct">
                  <Check className="size-3.5" aria-hidden />
                  Step complete
                </span>
              )}
              <span className="inline-flex items-center gap-1.5">
                <Kbd>Enter</Kbd> {isLast ? 'to finish' : 'to continue'}
              </span>
            </span>
          ) : (
            <span>Complete the step to continue</span>
          )}
        </div>

        <Button
          onClick={onNext}
          disabled={!stepCompleted}
          data-testid="lesson-next"
          className={cn(
            'h-10 min-w-32 px-5 text-sm font-semibold',
            'disabled:border disabled:border-border disabled:bg-surface-2 disabled:text-muted-foreground disabled:opacity-100',
            stepCompleted && 'shadow-[0_0_0_3px_color-mix(in_oklch,var(--primary)_22%,transparent)]',
          )}
        >
          {isLast ? (
            <>
              <Flag className="size-4" />
              Finish
            </>
          ) : (
            <>
              Next
              <ArrowRight className="size-4" />
            </>
          )}
        </Button>
      </footer>
    </div>
  );
}
