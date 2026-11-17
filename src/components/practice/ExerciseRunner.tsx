import { useCallback, useRef, useState } from 'react';
import { ArrowLeft } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { DifficultyMeter } from '@/components/learn/kit';
import { ScalePractice } from './ScalePractice';
import { ChordDetector } from './ChordDetector';
import { RhythmTrainer } from './RhythmTrainer';
import { DrumPatternPractice } from './DrumPatternPractice';
import { ScoreDisplay } from './ScoreDisplay';
import { AccuracyMeter } from './AccuracyMeter';
import { PRACTICE_CATEGORY_META } from './practiceMeta';
import { usePracticeScoring } from '@/hooks/usePracticeScoring';
import type { PracticeExercise } from '@/types/appMode';
import type { ActiveNote } from '@/hooks/useMidiNotes';
import type { ChordInfo } from '@/types/music';
import type { ScaleName } from '@/types/music';
import type { DrumPadId } from '@/types/drums';
import type { RhythmFeedback } from '@/types/appMode';

export interface ExerciseAttempt {
  accuracy: number;
  stars: 0 | 1 | 2 | 3;
}

interface ExerciseRunnerProps {
  exercise: PracticeExercise;
  /** Called with the best attempt finished in this session (null if none was finished). */
  onExit: (bestAttempt: ExerciseAttempt | null) => void;
  onSaveScore: (id: string, stars: 0 | 1 | 2 | 3, accuracy: number) => void;
  /** Best accuracy (0..1) saved before this session, for "new personal best". */
  initialBestAccuracy?: number;
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
}

export function ExerciseRunner({
  exercise,
  onExit,
  onSaveScore,
  initialBestAccuracy,
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
}: ExerciseRunnerProps) {
  const scoring = usePracticeScoring(exercise.scoring);
  const bestAttemptRef = useRef<ExerciseAttempt | null>(null);
  /** Best accuracy before the attempt that just finished (for the score screen). */
  const [bestBefore, setBestBefore] = useState<number | undefined>(initialBestAccuracy);
  /** Live tally for note / chord exercises (rhythm shows its own). */
  const [tally, setTally] = useState({ correct: 0, total: 0 });
  /** Bumped on retry so the activity remounts with fresh per-attempt state. */
  const [attempt, setAttempt] = useState(0);

  const recordAttempt = useCallback((result: ExerciseAttempt) => {
    const best = bestAttemptRef.current;
    const previous = Math.max(initialBestAccuracy ?? -1, best?.accuracy ?? -1);
    setBestBefore(previous >= 0 ? previous : undefined);
    if (!best || result.accuracy > best.accuracy) bestAttemptRef.current = result;
    onSaveScore(exercise.id, result.stars, result.accuracy);
  }, [exercise.id, onSaveScore, initialBestAccuracy]);

  const handleExit = useCallback(() => {
    onExit(bestAttemptRef.current);
  }, [onExit]);

  const countEvent = useCallback(
    (correct: boolean) => {
      scoring.recordEvent(correct);
      setTally((t) => ({ correct: t.correct + (correct ? 1 : 0), total: t.total + 1 }));
    },
    [scoring],
  );

  const handleNoteHit = useCallback((_midi: number, correct: boolean) => countEvent(correct), [countEvent]);
  const handleChordMatch = useCallback((correct: boolean) => countEvent(correct), [countEvent]);

  const handleSequenceComplete = useCallback(() => {
    recordAttempt(scoring.finish());
  }, [scoring, recordAttempt]);

  const handleRhythmComplete = useCallback(
    (accuracy: number, _feedbackHistory: RhythmFeedback[]) => {
      recordAttempt(scoring.finish(accuracy));
    },
    [scoring, recordAttempt],
  );

  const handleRetry = useCallback(() => {
    scoring.reset();
    setTally({ correct: 0, total: 0 });
    setAttempt((a) => a + 1);
  }, [scoring]);

  const category = PRACTICE_CATEGORY_META[exercise.category];
  const finished = scoring.finished && scoring.result;
  const showTally = !finished && (exercise.activity === 'scale-practice' || exercise.activity === 'chord-detection');

  const header = (
    <header className="flex h-14 shrink-0 items-center gap-4 border-b border-border bg-surface-1 px-4">
      <Button
        variant="ghost"
        size="icon-sm"
        className="text-muted-foreground hover:text-foreground"
        onClick={handleExit}
        title="Back to exercises"
        aria-label="Back to exercises"
      >
        <ArrowLeft className="size-4" />
      </Button>
      <div className="h-6 w-px bg-border" aria-hidden />
      <div className="flex min-w-0 items-center gap-2.5">
        <span className="flex size-8 shrink-0 items-center justify-center rounded-md border border-border bg-surface-2">
          <category.icon className="size-4 text-muted-foreground" aria-hidden />
        </span>
        <div className="min-w-0">
          <p className="text-[10px] font-bold uppercase leading-none tracking-[0.1em] text-muted-foreground">
            {category.label} exercise
          </p>
          <h2 className="mt-1 truncate text-sm font-semibold leading-none text-foreground">{exercise.title}</h2>
        </div>
        <DifficultyMeter difficulty={exercise.difficulty} className="ml-2" />
      </div>
      {showTally && (
        <div className="ml-auto flex items-center gap-5 text-xs text-muted-foreground">
          <span>
            Correct <span className="font-mono font-semibold tabular-nums text-note-correct">{tally.correct}</span>
          </span>
          <span>
            Misses <span className="font-mono font-semibold tabular-nums text-note-wrong">{tally.total - tally.correct}</span>
          </span>
          {tally.total > 0 && <AccuracyMeter accuracy={tally.correct / tally.total} compact />}
        </div>
      )}
    </header>
  );

  if (finished && scoring.result) {
    return (
      <div className="flex flex-1 flex-col overflow-hidden bg-background">
        {header}
        <div className="flex flex-1 items-center justify-center overflow-y-auto p-8">
          <ScoreDisplay
            accuracy={scoring.result.accuracy}
            stars={scoring.result.stars}
            previousBest={bestBefore}
            onRetry={handleRetry}
            onBack={handleExit}
          />
        </div>
      </div>
    );
  }

  return (
    <div className="flex flex-1 flex-col overflow-hidden bg-background">
      {header}

      <div className="flex-1 overflow-y-auto" key={attempt}>
        {exercise.activity === 'scale-practice' && (
          <ScalePractice
            activeNotes={activeNotes}
            onNoteOn={onNoteOn}
            onNoteOff={onNoteOff}
            highlightedNotes={highlightedNotes}
            selectedRoot={selectedRoot}
            setSelectedRoot={setSelectedRoot}
            selectedScale={selectedScale}
            setSelectedScale={setSelectedScale}
            exercise={exercise}
            onNoteHit={handleNoteHit}
            onSequenceComplete={handleSequenceComplete}
          />
        )}
        {exercise.activity === 'chord-detection' && (
          <ChordDetector
            activeNotes={activeNotes}
            onNoteOn={onNoteOn}
            onNoteOff={onNoteOff}
            detectedChord={detectedChord}
            exercise={exercise}
            onChordMatch={handleChordMatch}
            onSequenceComplete={handleSequenceComplete}
          />
        )}
        {exercise.activity === 'rhythm-training' && (
          <RhythmTrainer
            activeNotes={activeNotes}
            onNoteOn={onNoteOn}
            onNoteOff={onNoteOff}
            bpm={exercise.config.bpm}
            totalBeats={exercise.config.totalBeats}
            subdivision={exercise.config.subdivision}
            onComplete={handleRhythmComplete}
            instructions={exercise.config.instructions}
          />
        )}
        {exercise.activity === 'drum-patterns' && (
          <DrumPatternPractice
            activePads={activePads}
            onDrumHit={onDrumHit}
            pattern={exercise.config.pattern}
            bpm={exercise.config.bpm}
            totalBeats={exercise.config.totalBeats}
            patternName={exercise.config.patternName}
            onComplete={handleRhythmComplete}
            instructions={exercise.config.instructions}
          />
        )}
      </div>
    </div>
  );
}
