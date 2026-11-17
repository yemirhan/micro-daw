import { useMemo, useState } from 'react';
import { GraduationCap } from 'lucide-react';
import { CategoryFilter } from './CategoryFilter';
import { LessonCard } from './LessonCard';
import { LessonPlayer } from './LessonPlayer';
import { LessonComplete } from './LessonComplete';
import { ProgressBar, SectionLabel, ViewHeader } from './kit';
import { CATEGORY_META, CATEGORY_ORDER, lessonFraction, nextLessonAfter, readLessonProgress } from './lessonMeta';
import { useLessonPlayer } from '@/hooks/useLessonPlayer';
import { ALL_LESSONS, getLessonById, getLessonsByCategory } from '@/data/lessons';
import type { LessonCategory } from '@/types/appMode';
import type { ActiveNote } from '@/hooks/useMidiNotes';
import type { ChordInfo } from '@/types/music';
import type { DrumPadId } from '@/types/drums';

interface LearnViewProps {
  activeNotes: Map<number, ActiveNote>;
  onNoteOn: (note: number, velocity: number) => void;
  onNoteOff: (note: number) => void;
  activePads: Set<DrumPadId>;
  onDrumHit: (padId: DrumPadId, velocity: number) => void;
  detectedChord: ChordInfo | null;
  /** Panic: silence every live note (useMidiNotes().allNotesOff). */
  onAllNotesOff?: () => void;
}

const CATEGORY_COUNTS = CATEGORY_ORDER.reduce(
  (acc, c) => ({ ...acc, [c]: ALL_LESSONS.filter((l) => l.category === c).length }),
  {} as Record<LessonCategory, number>,
);

export function LearnView({
  activeNotes,
  onNoteOn,
  onNoteOff,
  activePads,
  onDrumHit,
  detectedChord,
  onAllNotesOff,
}: LearnViewProps) {
  const [categoryFilter, setCategoryFilter] = useState<LessonCategory | null>(null);
  // Every step change / exit / finish silences prompts and live notes.
  const lessonPlayer = useLessonPlayer({ onPanic: onAllNotesOff });
  const { isLessonCompleted } = lessonPlayer;
  const justCompleted = lessonPlayer.lastCompletedLessonId
    ? getLessonById(lessonPlayer.lastCompletedLessonId)
    : undefined;

  // isLessonCompleted changes identity whenever the stored progress changes,
  // so this re-reads the snapshot exactly when needed.
  const progress = useMemo(() => {
    void isLessonCompleted;
    return readLessonProgress();
  }, [isLessonCompleted]);

  const filteredLessons = getLessonsByCategory(categoryFilter ?? undefined);
  const completedCount = ALL_LESSONS.filter((l) => isLessonCompleted(l.id)).length;

  // If a lesson is active, show the player
  if (lessonPlayer.activeLesson && lessonPlayer.currentStep) {
    return (
      <LessonPlayer
        lesson={lessonPlayer.activeLesson}
        currentStep={lessonPlayer.currentStep}
        currentStepIndex={lessonPlayer.currentStepIndex}
        totalSteps={lessonPlayer.totalSteps}
        stepCompleted={lessonPlayer.stepCompleted}
        pendingRelease={lessonPlayer.pendingRelease}
        sequenceIndex={lessonPlayer.sequenceIndex}
        completedSteps={
          // A finished lesson restarts as a fresh run: don't paint it as done.
          progress.lessons[lessonPlayer.activeLesson.id]?.completed
            ? undefined
            : progress.lessons[lessonPlayer.activeLesson.id]?.completedSteps
        }
        activeNotes={activeNotes}
        onNoteOn={onNoteOn}
        onNoteOff={onNoteOff}
        activePads={activePads}
        onDrumHit={onDrumHit}
        detectedChord={detectedChord}
        onNext={lessonPlayer.nextStep}
        onPrev={lessonPlayer.prevStep}
        onExit={lessonPlayer.exitLesson}
        onQuizAnswer={lessonPlayer.completeQuiz}
      />
    );
  }

  // Finished a lesson: celebrate, then offer the next one.
  if (justCompleted) {
    const next = nextLessonAfter(justCompleted.id, isLessonCompleted);
    const category = justCompleted.category;
    const categoryTotal = CATEGORY_COUNTS[category];
    const categoryDone = ALL_LESSONS.filter((l) => l.category === category && isLessonCompleted(l.id)).length;
    return (
      <LessonComplete
        lesson={justCompleted}
        nextLesson={next}
        categoryDone={categoryDone}
        categoryTotal={categoryTotal}
        onNextLesson={next ? () => lessonPlayer.startLesson(next) : undefined}
        onReplay={() => lessonPlayer.startLesson(justCompleted)}
        onBack={lessonPlayer.dismissCompletion}
      />
    );
  }

  // Group the browser by category when showing everything.
  const groups: { category: LessonCategory; lessons: typeof filteredLessons }[] = categoryFilter
    ? [{ category: categoryFilter, lessons: filteredLessons }]
    : CATEGORY_ORDER.map((c) => ({ category: c, lessons: filteredLessons.filter((l) => l.category === c) })).filter(
        (g) => g.lessons.length > 0,
      );

  return (
    <div data-tour="learn-view" className="flex flex-1 flex-col overflow-hidden bg-background">
      <ViewHeader
        icon={GraduationCap}
        title="Learn"
        subtitle="Step-by-step lessons for piano, drums, theory and your ear. Play along with a MIDI keyboard or the on-screen keys."
        actions={
          <div className="w-56">
            <div className="mb-1.5 flex items-baseline justify-between text-[11px] text-muted-foreground">
              <span className="font-semibold uppercase tracking-[0.08em]">Your progress</span>
              <span>
                <span className="font-mono font-semibold tabular-nums text-foreground">{completedCount}</span>
                <span className="font-mono tabular-nums"> / {ALL_LESSONS.length}</span>
              </span>
            </div>
            <ProgressBar value={completedCount / ALL_LESSONS.length} tone="correct" label="Lessons completed" />
          </div>
        }
      >
        <CategoryFilter
          selected={categoryFilter}
          onChange={setCategoryFilter}
          counts={CATEGORY_COUNTS}
          total={ALL_LESSONS.length}
        />
      </ViewHeader>

      <div className="flex-1 overflow-y-auto px-8 py-6">
        <div className="mx-auto flex max-w-[1240px] flex-col gap-8">
          {groups.map((group) => {
            const meta = CATEGORY_META[group.category];
            const done = group.lessons.filter((l) => isLessonCompleted(l.id)).length;
            return (
              <section key={group.category} aria-label={meta.label}>
                <SectionLabel
                  right={
                    <span>
                      <span className="font-mono tabular-nums">{done}</span>/
                      <span className="font-mono tabular-nums">{group.lessons.length}</span> done
                    </span>
                  }
                >
                  {meta.label}
                  <span className="ml-2 font-medium normal-case tracking-normal text-muted-foreground/70">
                    {meta.blurb}
                  </span>
                </SectionLabel>
                <div className="grid grid-cols-2 gap-3 xl:grid-cols-3 2xl:grid-cols-4">
                  {group.lessons.map((lesson) => (
                    <LessonCard
                      key={lesson.id}
                      lesson={lesson}
                      isCompleted={isLessonCompleted(lesson.id)}
                      progress={lessonFraction(lesson, progress)}
                      stepCount={lesson.steps.length}
                      showCategory={false}
                      onClick={() => lessonPlayer.startLesson(lesson)}
                    />
                  ))}
                </div>
              </section>
            );
          })}

          {filteredLessons.length === 0 && (
            <p className="mt-8 text-center text-sm text-muted-foreground">No lessons in this category yet.</p>
          )}
        </div>
      </div>
    </div>
  );
}
