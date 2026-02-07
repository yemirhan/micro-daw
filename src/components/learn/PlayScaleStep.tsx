import { PianoRoll } from '@/components/PianoRoll';
import type { LessonStep } from '@/types/appMode';
import type { ActiveNote } from '@/hooks/useMidiNotes';

interface PlayScaleStepProps {
  step: LessonStep;
  activeNotes: Map<number, ActiveNote>;
  onNoteOn: (note: number, velocity: number) => void;
  onNoteOff: (note: number) => void;
  completed: boolean;
}

export function PlayScaleStep({ step, activeNotes, onNoteOn, onNoteOff, completed }: PlayScaleStepProps) {
  const highlightedNotes = new Set(step.expectedScale?.notes ?? []);

  return (
    <div className="flex flex-col gap-4">
      <p className="text-sm text-foreground/90">{step.description}</p>

      {step.expectedScale && (
        <p className="text-xs text-muted-foreground">
          Play the notes in order: {step.expectedScale.notes.map((pc) => {
            const names = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'];
            return names[pc];
          }).join(' → ')}
        </p>
      )}

      {step.hint && !completed && (
        <p className="text-xs text-muted-foreground italic">{step.hint}</p>
      )}

      <div className="overflow-x-auto">
        <PianoRoll
          activeNotes={activeNotes}
          onNoteOn={onNoteOn}
          onNoteOff={onNoteOff}
          highlightedNotes={highlightedNotes}
          compact
        />
      </div>
    </div>
  );
}
