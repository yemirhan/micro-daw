import { PianoRoll } from '@/components/PianoRoll';
import type { LessonStep } from '@/types/appMode';
import type { ActiveNote } from '@/hooks/useMidiNotes';

interface PlayNotesStepProps {
  step: LessonStep;
  activeNotes: Map<number, ActiveNote>;
  onNoteOn: (note: number, velocity: number) => void;
  onNoteOff: (note: number) => void;
  completed: boolean;
}

export function PlayNotesStep({ step, activeNotes, onNoteOn, onNoteOff, completed }: PlayNotesStepProps) {
  const highlightedNotes = new Set(
    (step.expectedNotes ?? []).map((n) => n.midi % 12),
  );

  return (
    <div className="flex flex-col gap-4">
      <p className="text-sm text-foreground/90">{step.description}</p>

      {step.expectedNotes && (
        <div className="flex flex-wrap gap-2">
          {step.expectedNotes.map((n) => {
            const isPlaying = activeNotes.has(n.midi);
            return (
              <span
                key={n.midi}
                className="rounded-md px-2.5 py-1 text-xs font-medium transition-colors"
                style={{
                  backgroundColor: isPlaying
                    ? 'oklch(0.65 0.25 150 / 0.3)'
                    : 'oklch(0.20 0.02 270)',
                  color: isPlaying ? 'oklch(0.85 0.15 150)' : undefined,
                }}
              >
                {n.label ?? `Note ${n.midi}`}
              </span>
            );
          })}
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
          highlightedNotes={highlightedNotes}
          compact
        />
      </div>
    </div>
  );
}
