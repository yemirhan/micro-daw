import { useCallback } from 'react';
import { ArrowLeft } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { ScalePractice } from './ScalePractice';
import { ChordDetector } from './ChordDetector';
import { RhythmTrainer } from './RhythmTrainer';
import { DrumPatternPractice } from './DrumPatternPractice';
import { ScoreDisplay } from './ScoreDisplay';
import { usePracticeScoring } from '@/hooks/usePracticeScoring';
import type { PracticeExercise } from '@/types/appMode';
import type { ActiveNote } from '@/hooks/useMidiNotes';
import type { ChordInfo } from '@/types/music';
import type { ScaleName } from '@/types/music';
import type { DrumPadId } from '@/types/drums';
import type { RhythmFeedback } from '@/types/appMode';

interface ExerciseRunnerProps {
  exercise: PracticeExercise;
  onExit: () => void;
  onSaveScore: (id: string, stars: 0 | 1 | 2 | 3, accuracy: number) => void;
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

  const handleNoteHit = useCallback(
    (_midi: number, correct: boolean) => {
      scoring.recordEvent(correct);
    },
    [scoring],
  );

  const handleChordMatch = useCallback(
    (correct: boolean) => {
      scoring.recordEvent(correct);
    },
    [scoring],
  );

  const handleSequenceComplete = useCallback(() => {
    const result = scoring.finish();
    onSaveScore(exercise.id, result.stars, result.accuracy);
  }, [scoring, exercise.id, onSaveScore]);

  const handleRhythmComplete = useCallback(
    (accuracy: number, _feedbackHistory: RhythmFeedback[]) => {
      const result = scoring.finish(accuracy);
      onSaveScore(exercise.id, result.stars, result.accuracy);
    },
    [scoring, exercise.id, onSaveScore],
  );

  const handleRetry = useCallback(() => {
    scoring.reset();
  }, [scoring]);

  if (scoring.finished && scoring.result) {
    return (
      <div className="flex flex-1 flex-col overflow-hidden">
        <div className="flex items-center gap-3 border-b border-border/50 px-6 py-3">
          <Button variant="ghost" size="sm" className="h-7 w-7 p-0" onClick={onExit} title="Back to exercises">
            <ArrowLeft className="h-4 w-4" />
          </Button>
          <h2 className="text-sm font-semibold">{exercise.title}</h2>
        </div>
        <div className="flex flex-1 items-center justify-center">
          <ScoreDisplay
            accuracy={scoring.result.accuracy}
            stars={scoring.result.stars}
            onRetry={handleRetry}
            onBack={onExit}
          />
        </div>
      </div>
    );
  }

  return (
    <div className="flex flex-1 flex-col overflow-hidden">
      <div className="flex items-center gap-3 border-b border-border/50 px-6 py-3">
        <Button variant="ghost" size="sm" className="h-7 w-7 p-0" onClick={onExit} title="Back to exercises">
          <ArrowLeft className="h-4 w-4" />
        </Button>
        <h2 className="text-sm font-semibold">{exercise.title}</h2>
      </div>

      <div className="flex-1 overflow-y-auto">
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
          />
        )}
      </div>
    </div>
  );
}
