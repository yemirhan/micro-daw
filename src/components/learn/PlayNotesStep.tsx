import { useRef } from 'react';
import { PianoRoll } from '@/components/PianoRoll';
import type { LessonStep } from '@/types/appMode';
import type { ActiveNote } from '@/hooks/useMidiNotes';
import { midiToNoteName } from '@/utils/noteHelpers';
import { pitchClass, playNotesMode } from '@/utils/lessonMatching';
import { NoteChip, type NoteChipState } from './kit';
import { InstrumentWell, PlayStatus, StepDescription, StepHint, noteLabel, useWrongNoteFlash } from './StepStage';

interface PlayNotesStepProps {
  step: LessonStep;
  activeNotes: Map<number, ActiveNote>;
  onNoteOn: (note: number, velocity: number) => void;
  onNoteOff: (note: number) => void;
  completed: boolean;
  pendingRelease?: boolean;
  /** Sequence mode: how many notes of the melody have been played correctly so far. */
  sequenceIndex?: number;
}

export function PlayNotesStep({
  step,
  activeNotes,
  onNoteOn,
  onNoteOff,
  completed,
  pendingRelease = false,
  sequenceIndex = 0,
}: PlayNotesStepProps) {
  const expected = step.expectedNotes ?? [];
  const sequence = playNotesMode(step) === 'sequence';
  const anyOctave = !!step.anyOctave;
  const progress = completed ? expected.length : sequenceIndex;
  const matches = (a: number, b: number) => (anyOctave ? pitchClass(a) === pitchClass(b) : a === b);
  const label = (n: { midi: number; label?: string }) =>
    anyOctave ? (n.label ?? midiToNoteName(n.midi)).replace(/-?\d+$/, '') : (n.label ?? midiToNoteName(n.midi));

  const isHeld = (midi: number) => [...activeNotes.keys()].some((n) => matches(n, midi));

  // Melody: mirror the player's cursor locally so a wrong note is judged
  // against the note that was due at that moment (not a stale render).
  const cursorRef = useRef(0);
  const flash = useWrongNoteFlash(
    (note) => {
      if (sequence) {
        const target = expected[cursorRef.current];
        if (target && matches(target.midi, note)) {
          cursorRef.current += 1;
          return false;
        }
        return target !== undefined;
      }
      return !expected.some((n) => matches(n.midi, note));
    },
    !completed && !pendingRelease && expected.length > 0,
    (note) => noteLabel(note, anyOctave),
  );

  // Together: every target key. Sequence: just the next note to play.
  const nextIndex = Math.min(progress, expected.length - 1);
  const targets = sequence ? expected.slice(nextIndex, nextIndex + 1) : expected;
  const highlightedNotes = new Set(targets.map((n) => (anyOctave ? pitchClass(n.midi) : n.midi)));

  const next = expected[progress];
  const prompt = sequence ? (
    next ? (
      <>
        Next note: <span className="font-mono font-semibold text-note-target">{label(next)}</span>
        <span className="ml-2 font-mono text-sm text-muted-foreground">
          {progress}/{expected.length}
        </span>
      </>
    ) : (
      'Melody played!'
    )
  ) : expected.length > 1 ? (
    <>
      Hold{' '}
      {expected.map((n, i) => (
        <span key={`${n.midi}-${i}`}>
          {i > 0 && ' + '}
          <span className="font-mono font-semibold text-note-target">{label(n)}</span>
        </span>
      ))}{' '}
      together
    </>
  ) : expected[0] ? (
    <>
      Play <span className="font-mono font-semibold text-note-target">{label(expected[0])}</span>
      {anyOctave && ' in any octave'}
    </>
  ) : (
    'Play the highlighted notes'
  );

  return (
    <div className="flex flex-col gap-5">
      <StepDescription>{step.description}</StepDescription>

      {expected.length > 0 && (
        <div className="flex flex-col gap-2">
          <span className="text-[11px] font-bold uppercase tracking-[0.08em] text-muted-foreground">
            {sequence ? 'Play in order' : expected.length > 1 ? 'Play together' : 'Target'}
            {anyOctave && ' · any octave'}
          </span>
          <div className="flex flex-wrap items-center gap-2" data-testid="expected-notes">
            {expected.map((n, i) => {
              let state: NoteChipState;
              if (sequence) state = i < progress ? 'done' : i === progress && !completed ? 'current' : 'todo';
              else state = completed ? 'done' : isHeld(n.midi) ? 'held' : 'current';
              return (
                <NoteChip key={`${n.midi}-${i}`} state={state} size={sequence ? 'md' : 'lg'}>
                  {label(n)}
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
          highlightedNotes={completed ? undefined : highlightedNotes}
          highlightMode={anyOctave ? 'pitch-class' : 'exact'}
          compact
        />
      </InstrumentWell>
    </div>
  );
}
