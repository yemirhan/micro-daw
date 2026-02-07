import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { PianoRoll } from '@/components/PianoRoll';
import { SCALE_DEFINITIONS, getRootNoteOptions, getScaleOptions } from '@/utils/scales';
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
  const rootOptions = getRootNoteOptions();
  const scaleOptions = getScaleOptions();

  // Apply exercise config
  const isExercise = !!exercise;
  const targetNotes = exercise?.config.targetNotes;

  useEffect(() => {
    if (!exercise) return;
    if (exercise.config.scaleRoot !== undefined) setSelectedRoot(exercise.config.scaleRoot);
    if (exercise.config.scaleName) setSelectedScale(exercise.config.scaleName as ScaleName);
  }, [exercise, setSelectedRoot, setSelectedScale]);

  // Note sequence tracking
  const [sequenceIndex, setSequenceIndex] = useState(0);
  const prevNotesRef = useRef<Set<number>>(new Set());

  // Compute highlighted notes for target-note exercises
  const exerciseHighlightedNotes = useMemo(() => {
    if (!targetNotes) return highlightedNotes;
    return new Set(targetNotes.map((n) => n.midi));
  }, [targetNotes, highlightedNotes]);

  // Current target for sequence
  const currentTarget = targetNotes?.[sequenceIndex];

  // Track note-on events for sequence and accuracy
  const handleNoteOn = useCallback(
    (note: number, velocity: number) => {
      onNoteOn(note, velocity);

      if (targetNotes && currentTarget) {
        const correct = note === currentTarget.midi;
        onNoteHit?.(note, correct);
        if (correct) {
          const nextIdx = sequenceIndex + 1;
          if (nextIdx >= targetNotes.length) {
            setSequenceIndex(nextIdx);
            onSequenceComplete?.();
          } else {
            setSequenceIndex(nextIdx);
          }
        }
      } else if (isExercise && exercise?.config.scaleName) {
        // Scale accuracy mode: check if note is in scale
        const scaleNotes = exerciseHighlightedNotes;
        const correct = scaleNotes.has(note % 12) || scaleNotes.has(note);
        onNoteHit?.(note, correct);
      }
    },
    [onNoteOn, targetNotes, currentTarget, sequenceIndex, onNoteHit, onSequenceComplete, isExercise, exercise, exerciseHighlightedNotes],
  );

  // Reset sequence on exercise change
  useEffect(() => {
    setSequenceIndex(0);
  }, [exercise?.id]);

  const displayHighlight = targetNotes ? exerciseHighlightedNotes : highlightedNotes;
  const effectiveRoot = exercise?.config.scaleRoot ?? selectedRoot;
  const effectiveScale = (exercise?.config.scaleName as ScaleName) ?? selectedScale;

  return (
    <div className="flex flex-1 flex-col gap-6 p-6">
      <div>
        <h2 className="text-sm font-semibold text-muted-foreground">
          {isExercise ? exercise.title : 'Scale Practice'}
        </h2>
        <p className="mt-1 text-xs text-muted-foreground">
          {exercise?.config.instructions ?? 'Select a root note and scale — highlighted keys show the scale tones'}
        </p>
      </div>

      {!isExercise && (
        <div className="flex items-center gap-4">
          <div className="flex items-center gap-2">
            <label className="text-xs text-muted-foreground">Root:</label>
            <select
              className="rounded-md bg-secondary px-2 py-1 text-xs"
              value={selectedRoot}
              onChange={(e) => setSelectedRoot(Number(e.target.value))}
            >
              {rootOptions.map((opt) => (
                <option key={opt.value} value={opt.value}>{opt.label}</option>
              ))}
            </select>
          </div>
          <div className="flex items-center gap-2">
            <label className="text-xs text-muted-foreground">Scale:</label>
            <select
              className="rounded-md bg-secondary px-2 py-1 text-xs"
              value={selectedScale ?? ''}
              onChange={(e) => setSelectedScale((e.target.value || null) as ScaleName | null)}
            >
              <option value="">None</option>
              {scaleOptions.map((opt) => (
                <option key={opt.value} value={opt.value}>{opt.label}</option>
              ))}
            </select>
          </div>
        </div>
      )}

      {/* Sequence progress for target-note exercises */}
      {targetNotes && (
        <div className="flex flex-wrap items-center gap-1.5">
          {targetNotes.map((tn, i) => (
            <span
              key={i}
              className="rounded-md px-2 py-1 text-xs font-medium transition-colors"
              style={{
                backgroundColor:
                  i < sequenceIndex
                    ? 'oklch(0.30 0.15 150)' // completed = green
                    : i === sequenceIndex
                    ? 'oklch(0.30 0.12 270)' // current = blue
                    : 'oklch(0.18 0.01 270)', // pending = dark
                color: i <= sequenceIndex ? 'white' : 'oklch(0.55 0.01 270)',
              }}
            >
              {tn.label}
            </span>
          ))}
        </div>
      )}

      {!targetNotes && effectiveScale && (
        <div className="text-xs text-muted-foreground">
          Notes: {SCALE_DEFINITIONS.find((s) => s.name === effectiveScale)?.intervals.map((i) => {
            const names = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'];
            return names[(effectiveRoot + i) % 12];
          }).join(', ')}
        </div>
      )}

      <div className="overflow-x-auto">
        <PianoRoll
          activeNotes={activeNotes}
          onNoteOn={handleNoteOn}
          onNoteOff={onNoteOff}
          highlightedNotes={displayHighlight}
          compact
        />
      </div>
    </div>
  );
}
