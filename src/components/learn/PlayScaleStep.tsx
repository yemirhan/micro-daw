import { PianoRoll } from '@/components/PianoRoll';
import type { LessonStep } from '@/types/appMode';
import type { ActiveNote } from '@/hooks/useMidiNotes';
import { pitchClass } from '@/utils/lessonMatching';
import { NoteChip } from './kit';
import { InstrumentWell, PlayStatus, StepDescription, StepHint, noteLabel, useWrongNoteFlash } from './StepStage';

const NOTE_NAMES = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'];

interface PlayScaleStepProps {
  step: LessonStep;
  activeNotes: Map<number, ActiveNote>;
  onNoteOn: (note: number, velocity: number) => void;
  onNoteOff: (note: number) => void;
  completed: boolean;
  pendingRelease?: boolean;
  /** How many scale degrees have been played in order so far. */
  sequenceIndex?: number;
}

export function PlayScaleStep({
  step,
  activeNotes,
  onNoteOn,
  onNoteOff,
  completed,
  pendingRelease = false,
  sequenceIndex = 0,
}: PlayScaleStepProps) {
  const degrees = step.expectedScale?.notes ?? [];
  const progress = completed ? degrees.length : sequenceIndex;
  const highlightedNotes = new Set(degrees);
  const inScale = new Set(degrees);

  // Out-of-scale notes are wrong; order slips are handled by the player.
  const flash = useWrongNoteFlash(
    (note) => !inScale.has(pitchClass(note)),
    !completed && !pendingRelease && degrees.length > 0,
    (note) => noteLabel(note, true),
  );

  const next = degrees[progress];
  const prompt =
    next !== undefined ? (
      <>
        {progress === 0 ? 'Start on ' : 'Next: '}
        <span className="font-mono font-semibold text-note-target">{NOTE_NAMES[next]}</span>
        <span className="ml-2 font-mono text-sm text-muted-foreground">
          {progress}/{degrees.length}
        </span>
      </>
    ) : (
      'Scale played!'
    );

  return (
    <div className="flex flex-col gap-5">
      <StepDescription>{step.description}</StepDescription>

      {degrees.length > 0 && (
        <div className="flex flex-col gap-2">
          <span className="text-[11px] font-bold uppercase tracking-[0.08em] text-muted-foreground">
            {step.expectedScale?.name ? `${step.expectedScale.name} · ` : ''}Play in order, any octave
          </span>
          <div className="flex flex-wrap items-center gap-2" data-testid="scale-degrees">
            {degrees.map((pc, i) => {
              const done = i < progress;
              const current = i === progress && !completed;
              return (
                <NoteChip key={`${pc}-${i}`} state={done ? 'done' : current ? 'current' : 'todo'}>
                  {NOTE_NAMES[pc]}
                </NoteChip>
              );
            })}
          </div>
        </div>
      )}

      <PlayStatus completed={completed} pendingRelease={pendingRelease} flash={flash} prompt={prompt} />

      {step.hint && !completed && <StepHint>{step.hint}</StepHint>}

      <InstrumentWell>
        <PianoRoll
          activeNotes={activeNotes}
          onNoteOn={onNoteOn}
          onNoteOff={onNoteOff}
          highlightedNotes={highlightedNotes}
          compact
        />
      </InstrumentWell>
    </div>
  );
}
