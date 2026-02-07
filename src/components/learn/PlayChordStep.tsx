import { PianoRoll } from '@/components/PianoRoll';
import type { LessonStep } from '@/types/appMode';
import type { ActiveNote } from '@/hooks/useMidiNotes';
import type { ChordInfo } from '@/types/music';

interface PlayChordStepProps {
  step: LessonStep;
  activeNotes: Map<number, ActiveNote>;
  onNoteOn: (note: number, velocity: number) => void;
  onNoteOff: (note: number) => void;
  detectedChord: ChordInfo | null;
  completed: boolean;
}

export function PlayChordStep({ step, activeNotes, onNoteOn, onNoteOff, detectedChord, completed }: PlayChordStepProps) {
  return (
    <div className="flex flex-col gap-4">
      <p className="text-sm text-foreground/90">{step.description}</p>

      {step.expectedChord && (
        <div className="flex items-center gap-3">
          <span className="text-xs text-muted-foreground">Target:</span>
          <span
            className="rounded-lg px-3 py-1.5 text-lg font-bold"
            style={{ backgroundColor: 'oklch(0.20 0.02 270)' }}
          >
            {step.expectedChord.display}
          </span>
          {detectedChord && (
            <>
              <span className="text-xs text-muted-foreground">Playing:</span>
              <span
                className="rounded-lg px-3 py-1.5 text-lg font-bold"
                style={{
                  backgroundColor: completed
                    ? 'oklch(0.65 0.25 150 / 0.2)'
                    : 'oklch(0.20 0.02 270)',
                  color: completed ? 'oklch(0.85 0.15 150)' : undefined,
                }}
              >
                {detectedChord.display}
              </span>
            </>
          )}
        </div>
      )}

      {step.hint && !completed && (
        <p className="text-xs text-muted-foreground italic">{step.hint}</p>
      )}

      <div className="overflow-x-auto">
        <PianoRoll
          activeNotes={activeNotes}
          onNoteOn={onNoteOn}
          onNoteOff={onNoteOff}
          compact
        />
      </div>
    </div>
  );
}
