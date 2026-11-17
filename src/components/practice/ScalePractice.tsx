import { useEffect, useMemo, useRef, useState } from 'react';
import { PianoRoll } from '@/components/PianoRoll';
import { SCALE_DEFINITIONS } from '@/utils/scales';
import { ScaleSelector } from '@/components/ScaleSelector';
import { NoteChip, useFlash } from '@/components/learn/kit';
import { InstrumentWell, PlayStatus, StepDescription } from '@/components/learn/StepStage';
import { midiToNoteName } from '@/utils/noteHelpers';
import { useNoteEvents } from '@/hooks/useNoteEvents';
import { pitchClass } from '@/utils/lessonMatching';
import type { ActiveNote } from '@/hooks/useMidiNotes';
import type { ScaleName } from '@/types/music';
import type { PracticeExercise } from '@/types/appMode';

interface ScalePracticeProps {
  activeNotes: Map<number, ActiveNote>;
  onNoteOn: (note: number, velocity: number) => void;
  onNoteOff: (note: number) => void;
  highlightedNotes: Set<number>;
  selectedRoot: number;
  setSelectedRoot: (root: number) => void;
  selectedScale: ScaleName | null;
  setSelectedScale: (scale: ScaleName | null) => void;
  exercise?: PracticeExercise;
  onNoteHit?: (midi: number, correct: boolean) => void;
  onSequenceComplete?: () => void;
}

export function ScalePractice({
  activeNotes,
  onNoteOn,
  onNoteOff,
  highlightedNotes,
  selectedRoot,
  setSelectedRoot,
  selectedScale,
  setSelectedScale,
  exercise,
  onNoteHit,
  onSequenceComplete,
}: ScalePracticeProps) {
  // Apply exercise config
  const isExercise = !!exercise;

  useEffect(() => {
    if (!exercise) return;
    if (exercise.config.scaleRoot !== undefined) setSelectedRoot(exercise.config.scaleRoot);
    if (exercise.config.scaleName) setSelectedScale(exercise.config.scaleName as ScaleName);
  }, [exercise, setSelectedRoot, setSelectedScale]);

  // What the exercise asks for, in order: exact notes, or the scale ascending
  // one octave (root ... root) in any octave.
  const sequence = useMemo(() => buildExerciseSequence(exercise), [exercise]);

  const [sequenceIndex, setSequenceIndex] = useState(0);
  const { flash, trigger, clear } = useFlash();
  const sequenceIndexRef = useRef(0);
  const doneRef = useRef(false);

  // (Mounted fresh per exercise attempt: ExerciseRunner is keyed by exercise
  // and swaps to the score screen when finished.)

  // Score every note-on (MIDI keyboard or on-screen keys) against the next target.
  useNoteEvents((event) => {
    if (event.type !== 'noteon' || !sequence || doneRef.current) return;
    const idx = sequenceIndexRef.current;
    const target = sequence.items[idx];
    if (!target) return;
    const correct = sequence.exact ? event.note === target.value : pitchClass(event.note) === target.value;
    onNoteHit?.(event.note, correct);
    if (!correct) {
      trigger(sequence.exact ? midiToNoteName(event.note) : PC_NAMES[pitchClass(event.note)]);
      return;
    }
    clear();
    const next = idx + 1;
    sequenceIndexRef.current = next;
    setSequenceIndex(next);
    if (next >= sequence.items.length) {
      doneRef.current = true;
      onSequenceComplete?.();
    }
  }, !!sequence);

  const currentTarget = sequence?.items[sequenceIndex];
  // Highlight: the next note to play (exercises) or the selected scale (free).
  const displayHighlight = useMemo(() => {
    if (!sequence) return highlightedNotes;
    return currentTarget ? new Set([currentTarget.value]) : new Set<number>();
  }, [sequence, currentTarget, highlightedNotes]);
  const highlightMode = sequence?.exact ? 'exact' : 'pitch-class';
  const effectiveRoot = exercise?.config.scaleRoot ?? selectedRoot;
  const effectiveScale = (exercise?.config.scaleName as ScaleName) ?? selectedScale;
  const scaleTones = effectiveScale
    ? SCALE_DEFINITIONS.find((s) => s.name === effectiveScale)?.intervals.map((i) => (effectiveRoot + i) % 12) ?? []
    : [];
  const heldPcs = new Set([...activeNotes.keys()].map(pitchClass));

  return (
    <div className="mx-auto flex w-full max-w-[1080px] flex-col gap-5 px-10 py-8">
      <StepDescription>
        {exercise?.config.instructions ??
          'Pick a root and a scale. Every key in the scale lights up, in every octave, so you can explore it freely.'}
      </StepDescription>

      {!isExercise && (
        <div className="flex flex-wrap items-center gap-6">
          <div className="flex items-center gap-3">
            <span className="text-[11px] font-bold uppercase tracking-[0.08em] text-muted-foreground">Scale</span>
            <ScaleSelector
              selectedRoot={selectedRoot}
              selectedScale={selectedScale}
              onRootChange={setSelectedRoot}
              onScaleChange={setSelectedScale}
            />
          </div>
          {scaleTones.length > 0 && (
            <div className="flex flex-wrap items-center gap-1.5" aria-label="Scale tones">
              {scaleTones.map((pc, i) => (
                <NoteChip key={`${pc}-${i}`} size="sm" state={heldPcs.has(pc) ? 'held' : i === 0 ? 'current' : 'todo'}>
                  {PC_NAMES[pc]}
                </NoteChip>
              ))}
            </div>
          )}
        </div>
      )}

      {/* Sequence progress */}
      {sequence && (
        <div className="flex flex-col gap-2">
          <span className="text-[11px] font-bold uppercase tracking-[0.08em] text-muted-foreground">
            Play in order{sequence.exact ? '' : ', any octave'}
          </span>
          <div className="flex flex-wrap items-center gap-2" data-testid="exercise-sequence">
            {sequence.items.map((item, i) => (
              <NoteChip
                key={i}
                state={i < sequenceIndex ? 'done' : i === sequenceIndex ? 'current' : 'todo'}
              >
                {item.label}
              </NoteChip>
            ))}
          </div>
        </div>
      )}

      {sequence && (
        <PlayStatus
          completed={!currentTarget}
          pendingRelease={false}
          flash={flash}
          prompt={
            currentTarget ? (
              <>
                {sequenceIndex === 0 ? 'Start on ' : 'Next: '}
                <span className="font-mono font-semibold text-note-target">{currentTarget.label}</span>
                <span className="ml-2 font-mono text-sm text-muted-foreground">
                  {sequenceIndex}/{sequence.items.length}
                </span>
              </>
            ) : null
          }
          successText="All done!"
        />
      )}

      <InstrumentWell>
        <PianoRoll
          activeNotes={activeNotes}
          onNoteOn={onNoteOn}
          onNoteOff={onNoteOff}
          highlightedNotes={displayHighlight}
          highlightMode={highlightMode}
          compact
        />
      </InstrumentWell>
    </div>
  );
}

const PC_NAMES = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'];

export interface ExerciseSequence {
  /** true: MIDI notes must match exactly; false: pitch classes, any octave. */
  exact: boolean;
  items: { value: number; label: string }[];
}

/**
 * The ordered notes a scale-practice exercise expects, or null for free
 * practice: explicit target notes (exact), else the scale ascending one
 * octave from the root back to the root (any octave).
 */
export function buildExerciseSequence(exercise?: PracticeExercise): ExerciseSequence | null {
  if (!exercise) return null;
  const { targetNotes, scaleName, scaleRoot } = exercise.config;
  if (targetNotes && targetNotes.length > 0) {
    return { exact: true, items: targetNotes.map((n) => ({ value: n.midi, label: n.label })) };
  }
  const scale = SCALE_DEFINITIONS.find((d) => d.name === scaleName);
  if (!scale) return null;
  const root = scaleRoot ?? 0;
  const pcs = [...scale.intervals.map((i) => (root + i) % 12), root];
  return { exact: false, items: pcs.map((pc) => ({ value: pc, label: PC_NAMES[pc] })) };
}
