import { InfoStep } from './InfoStep';
import { PlayNotesStep } from './PlayNotesStep';
import { PlayChordStep } from './PlayChordStep';
import { PlayScaleStep } from './PlayScaleStep';
import { PlayDrumStep } from './PlayDrumStep';
import { QuizStep } from './QuizStep';
import { ListenAndIdentifyStep } from './ListenAndIdentifyStep';
import type { LessonStep } from '@/types/appMode';
import type { ActiveNote } from '@/hooks/useMidiNotes';
import type { ChordInfo } from '@/types/music';
import type { DrumPadId } from '@/types/drums';

interface LessonStepRendererProps {
  step: LessonStep;
  completed: boolean;
  /** Condition met; waiting for every key to be released. */
  pendingRelease?: boolean;
  sequenceIndex?: number;
  activeNotes: Map<number, ActiveNote>;
  onNoteOn: (note: number, velocity: number) => void;
  onNoteOff: (note: number) => void;
  activePads: Set<DrumPadId>;
  onDrumHit: (padId: DrumPadId, velocity: number) => void;
  detectedChord: ChordInfo | null;
  onQuizAnswer: (correct: boolean) => void;
}

export function LessonStepRenderer({
  step,
  completed,
  pendingRelease = false,
  sequenceIndex = 0,
  activeNotes,
  onNoteOn,
  onNoteOff,
  activePads,
  onDrumHit,
  detectedChord,
  onQuizAnswer,
}: LessonStepRendererProps) {
  switch (step.type) {
    case 'info':
      return <InfoStep step={step} />;
    case 'play-notes':
      return (
        <PlayNotesStep
          step={step}
          sequenceIndex={sequenceIndex}
          activeNotes={activeNotes}
          onNoteOn={onNoteOn}
          onNoteOff={onNoteOff}
          completed={completed}
          pendingRelease={pendingRelease}
        />
      );
    case 'play-chord':
      return (
        <PlayChordStep
          step={step}
          activeNotes={activeNotes}
          onNoteOn={onNoteOn}
          onNoteOff={onNoteOff}
          detectedChord={detectedChord}
          completed={completed}
          pendingRelease={pendingRelease}
        />
      );
    case 'play-scale':
      return (
        <PlayScaleStep
          step={step}
          sequenceIndex={sequenceIndex}
          activeNotes={activeNotes}
          onNoteOn={onNoteOn}
          onNoteOff={onNoteOff}
          completed={completed}
          pendingRelease={pendingRelease}
        />
      );
    case 'play-drums':
      return (
        <PlayDrumStep
          step={step}
          activePads={activePads}
          onDrumHit={onDrumHit}
          completed={completed}
        />
      );
    case 'quiz':
      return <QuizStep step={step} completed={completed} onAnswer={onQuizAnswer} />;
    case 'listen-and-identify':
      return <ListenAndIdentifyStep step={step} completed={completed} onAnswer={onQuizAnswer} />;
    default:
      return <p className="text-sm text-muted-foreground">Unknown step type</p>;
  }
}
