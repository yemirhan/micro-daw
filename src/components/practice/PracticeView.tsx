import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { ArrowLeft, BarChart3, Flame, Target } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { SectionLabel, ViewHeader } from '@/components/learn/kit';
import { PracticeActivityCard } from './PracticeActivityCard';
import { PracticeCategoryFilter } from './PracticeCategoryFilter';
import { PracticeExerciseCard } from './PracticeExerciseCard';
import { ExerciseRunner, type ExerciseAttempt } from './ExerciseRunner';
import { FreePlay } from './FreePlay';
import { ChordDetector } from './ChordDetector';
import { ScalePractice } from './ScalePractice';
import { RhythmTrainer } from './RhythmTrainer';
import { DrumPatternPractice } from './DrumPatternPractice';
import { StatsDashboard } from './stats/StatsDashboard';
import { usePracticeSession } from '@/hooks/usePracticeSession';
import { usePracticeProgress } from '@/hooks/usePracticeProgress';
import { usePracticeStats } from '@/hooks/usePracticeStats';
import { ALL_PRACTICES, getPracticesByCategory } from '@/data/practices';
import { ACTIVITIES, PRACTICE_CATEGORY_META, PRACTICE_CATEGORY_ORDER } from './practiceMeta';
import type { ActiveNote } from '@/hooks/useMidiNotes';
import type { ChordInfo } from '@/types/music';
import type { ScaleName } from '@/types/music';
import type { DrumPadId } from '@/types/drums';
import type { PracticeCategory } from '@/types/appMode';

interface PracticeViewProps {
  activeNotes: Map<number, ActiveNote>;
  onNoteOn: (note: number, velocity: number) => void;
  onNoteOff: (note: number) => void;
  activePads: Set<DrumPadId>;
  onDrumHit: (padId: DrumPadId, velocity: number) => void;
  detectedChord: ChordInfo | null;
  highlightedNotes: Set<number>;
  selectedRoot: number;
  setSelectedRoot: (root: number) => void;
  selectedScale: ScaleName | null;
  setSelectedScale: (scale: ScaleName | null) => void;
  /** Panic: silence every live note (useMidiNotes().allNotesOff). */
  onAllNotesOff?: () => void;
}

const CATEGORY_COUNTS = PRACTICE_CATEGORY_ORDER.reduce(
  (acc, c) => ({ ...acc, [c]: ALL_PRACTICES.filter((e) => e.category === c).length }),
  {} as Record<PracticeCategory, number>,
);

/** Back-arrow page header used by the free activities and the stats page. */
function SubHeader({ title, eyebrow, backTitle, onBack }: { title: string; eyebrow: string; backTitle: string; onBack: () => void }) {
  return (
    <header className="flex h-14 shrink-0 items-center gap-4 border-b border-border bg-surface-1 px-4">
      <Button
        variant="ghost"
        size="icon-sm"
        className="text-muted-foreground hover:text-foreground"
        onClick={onBack}
        title={backTitle}
        aria-label={backTitle}
      >
        <ArrowLeft className="size-4" />
      </Button>
      <div className="h-6 w-px bg-border" aria-hidden />
      <div className="min-w-0">
        <p className="text-[10px] font-bold uppercase leading-none tracking-[0.1em] text-muted-foreground">{eyebrow}</p>
        <h2 className="mt-1 truncate text-sm font-semibold leading-none text-foreground">{title}</h2>
      </div>
    </header>
  );
}

/** Small labelled number for the Practice header. */
function HeaderStat({ label, value, icon: Icon }: { label: string; value: React.ReactNode; icon?: typeof Flame }) {
  return (
    <div className="flex flex-col items-end">
      <span className="text-[10px] font-bold uppercase tracking-[0.1em] text-muted-foreground">{label}</span>
      <span className="flex items-center gap-1 font-mono text-lg font-semibold leading-tight tabular-nums text-foreground">
        {Icon && <Icon className="size-4 text-primary" aria-hidden />}
        {value}
      </span>
    </div>
  );
}

export function PracticeView({
  activeNotes,
  onNoteOn,
  onNoteOff,
  activePads,
  onDrumHit,
  detectedChord,
  highlightedNotes,
  selectedRoot,
  setSelectedRoot,
  selectedScale,
  setSelectedScale,
  onAllNotesOff,
}: PracticeViewProps) {
  const { config, activeExercise, startActivity, startExercise, exitActivity } = usePracticeSession();
  const { getScore, saveScore } = usePracticeProgress();
  const practiceStats = usePracticeStats();
  const [selectedCategory, setSelectedCategory] = useState<PracticeCategory | null>(null);
  const [showStats, setShowStats] = useState(false);

  const exercises = getPracticesByCategory(selectedCategory);

  const handleExit = useCallback(() => {
    onAllNotesOff?.();
    const result = exitActivity();
    if (result) {
      practiceStats.recordSession({
        exerciseId: result.exerciseId,
        durationSeconds: result.durationSeconds,
        accuracy: null,
        stars: 0,
        category: result.category,
      });
    }
  }, [exitActivity, practiceStats, onAllNotesOff]);

  const handleExerciseExit = useCallback((bestAttempt: ExerciseAttempt | null) => {
    onAllNotesOff?.();
    const result = exitActivity();
    // Per-attempt scores are saved via saveScore; the session records this
    // session's best attempt (not the all-time best), or no accuracy if the
    // exercise was abandoned.
    if (result) {
      practiceStats.recordSession({
        exerciseId: result.exerciseId,
        durationSeconds: result.durationSeconds,
        accuracy: bestAttempt ? Math.round(bestAttempt.accuracy * 100) : null, // percent
        stars: bestAttempt?.stars ?? 0,
        category: result.category,
      });
    }
  }, [exitActivity, practiceStats, onAllNotesOff]);

  // Leaving Practice mode mid-activity (sidebar / shortcut) still records the session.
  const exitOnUnmountRef = useRef<() => void>(() => undefined);
  const inActivity = !!(config || activeExercise);
  useLayoutEffect(() => {
    exitOnUnmountRef.current = () => {
      if (inActivity) handleExit();
    };
  }, [inActivity, handleExit]);
  useEffect(() => () => exitOnUnmountRef.current(), []);

  // Active exercise mode
  if (activeExercise) {
    return (
      <ExerciseRunner
        key={activeExercise.id}
        exercise={activeExercise}
        onExit={handleExerciseExit}
        onSaveScore={saveScore}
        initialBestAccuracy={getScore(activeExercise.id)?.bestAccuracy}
        activeNotes={activeNotes}
        onNoteOn={onNoteOn}
        onNoteOff={onNoteOff}
        activePads={activePads}
        onDrumHit={onDrumHit}
        detectedChord={detectedChord}
        highlightedNotes={highlightedNotes}
        selectedRoot={selectedRoot}
        setSelectedRoot={setSelectedRoot}
        selectedScale={selectedScale}
        setSelectedScale={setSelectedScale}
      />
    );
  }

  // Active free activity mode
  if (config) {
    return (
      <div className="flex flex-1 flex-col overflow-hidden bg-background">
        <SubHeader
          eyebrow="Free practice"
          title={ACTIVITIES.find((a) => a.id === config.activity)?.title ?? 'Practice'}
          backTitle="Back to activities"
          onBack={handleExit}
        />

        <div className="flex-1 overflow-y-auto">
          {config.activity === 'free-play' && (
            <FreePlay
              activeNotes={activeNotes}
              onNoteOn={onNoteOn}
              onNoteOff={onNoteOff}
              activePads={activePads}
              onDrumHit={onDrumHit}
              detectedChord={detectedChord}
            />
          )}
          {config.activity === 'chord-detection' && (
            <ChordDetector
              activeNotes={activeNotes}
              onNoteOn={onNoteOn}
              onNoteOff={onNoteOff}
              detectedChord={detectedChord}
            />
          )}
          {config.activity === 'scale-practice' && (
            <ScalePractice
              activeNotes={activeNotes}
              onNoteOn={onNoteOn}
              onNoteOff={onNoteOff}
              highlightedNotes={highlightedNotes}
              selectedRoot={selectedRoot}
              setSelectedRoot={setSelectedRoot}
              selectedScale={selectedScale}
              setSelectedScale={setSelectedScale}
            />
          )}
          {config.activity === 'rhythm-training' && (
            <RhythmTrainer
              activeNotes={activeNotes}
              onNoteOn={onNoteOn}
              onNoteOff={onNoteOff}
            />
          )}
          {config.activity === 'drum-patterns' && (
            <DrumPatternPractice
              activePads={activePads}
              onDrumHit={onDrumHit}
            />
          )}
        </div>
      </div>
    );
  }

  // Stats view
  if (showStats) {
    return (
      <div className="flex flex-1 flex-col overflow-hidden bg-background">
        <SubHeader eyebrow="Practice" title="Statistics" backTitle="Back to practice" onBack={() => setShowStats(false)} />
        <div className="flex-1 overflow-y-auto px-8 py-6">
          <div className="mx-auto max-w-[1240px]">
            <StatsDashboard stats={practiceStats} />
          </div>
        </div>
      </div>
    );
  }

  // Browse mode — activity hub + exercise grid
  const streak = practiceStats.getStreak();
  const weekMinutes = practiceStats.getDailySummaries(7).reduce((sum, d) => sum + d.totalMinutes, 0);
  const groups: { category: PracticeCategory; items: typeof exercises }[] = selectedCategory
    ? [{ category: selectedCategory, items: exercises }]
    : PRACTICE_CATEGORY_ORDER.map((c) => ({ category: c, items: exercises.filter((e) => e.category === c) })).filter(
        (g) => g.items.length > 0,
      );

  return (
    <div data-tour="practice-view" className="flex flex-1 flex-col overflow-hidden bg-background">
      <ViewHeader
        icon={Target}
        title="Practice"
        subtitle="Warm up freely, or take a scored exercise and earn up to three stars."
        actions={
          <>
            <HeaderStat label="Streak" value={`${streak.currentStreak}d`} icon={streak.currentStreak > 0 ? Flame : undefined} />
            <HeaderStat label="This week" value={`${Math.round(weekMinutes)}m`} />
            <HeaderStat label="Sessions" value={practiceStats.totalSessions} />
            <div className="mx-1 h-9 w-px bg-border" aria-hidden />
            <Button variant="outline" onClick={() => setShowStats(true)}>
              <BarChart3 className="size-4" />
              Stats
            </Button>
          </>
        }
      />

      <div className="flex-1 overflow-y-auto px-8 py-6">
        <div className="mx-auto flex max-w-[1240px] flex-col gap-8">
          <section aria-label="Free activities">
            <SectionLabel right="No score, no clock">Warm up</SectionLabel>
            <div className="grid grid-cols-5 gap-3">
              {ACTIVITIES.map((activity) => (
                <PracticeActivityCard
                  key={activity.id}
                  icon={activity.icon}
                  title={activity.title}
                  description={activity.description}
                  onClick={() => startActivity(activity.id)}
                />
              ))}
            </div>
          </section>

          <section aria-label="Exercises" className="flex flex-col gap-6">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <SectionLabel className="mb-0">Exercises</SectionLabel>
              <PracticeCategoryFilter
                selected={selectedCategory}
                onChange={setSelectedCategory}
                counts={CATEGORY_COUNTS}
                total={ALL_PRACTICES.length}
              />
            </div>
            {groups.map((group) => {
              const meta = PRACTICE_CATEGORY_META[group.category];
              const starred = group.items.reduce((sum, e) => sum + (getScore(e.id)?.bestStars ?? 0), 0);
              return (
                <div key={group.category}>
                  <SectionLabel
                    right={
                      <span>
                        <span className="font-mono tabular-nums">{starred}</span>/
                        <span className="font-mono tabular-nums">{group.items.length * 3}</span> stars
                      </span>
                    }
                  >
                    {meta.label}
                    <span className="ml-2 font-medium normal-case tracking-normal text-muted-foreground/70">{meta.blurb}</span>
                  </SectionLabel>
                  <div className="grid grid-cols-2 gap-3 xl:grid-cols-3 2xl:grid-cols-4">
                    {group.items.map((exercise) => (
                      <PracticeExerciseCard
                        key={exercise.id}
                        exercise={exercise}
                        score={getScore(exercise.id)}
                        onClick={() => startExercise(exercise)}
                      />
                    ))}
                  </div>
                </div>
              );
            })}
          </section>
        </div>
      </div>
    </div>
  );
}
