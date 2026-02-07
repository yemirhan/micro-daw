import { useCallback, useEffect, useRef, useState } from 'react';
import { PianoRoll } from '@/components/PianoRoll';
import type { ActiveNote } from '@/hooks/useMidiNotes';
import type { ChordInfo } from '@/types/music';
import type { PracticeExercise, ExpectedChord } from '@/types/appMode';

interface ChordDetectorProps {
  activeNotes: Map<number, ActiveNote>;
  onNoteOn: (note: number, velocity: number) => void;
  onNoteOff: (note: number) => void;
  detectedChord: ChordInfo | null;
  exercise?: PracticeExercise;
  onChordMatch?: (correct: boolean) => void;
  onSequenceComplete?: () => void;
}

export function ChordDetector({
  activeNotes,
  onNoteOn,
  onNoteOff,
  detectedChord,
  exercise,
  onChordMatch,
  onSequenceComplete,
}: ChordDetectorProps) {
  const isExercise = !!exercise;
  const targetChords = exercise?.config.targetChords;
  const [chordIndex, setChordIndex] = useState(0);
  const prevChordRef = useRef<string | null>(null);
  const wasEmptyRef = useRef(true);

  const currentTarget = targetChords?.[chordIndex];

  // Reset on exercise change
  useEffect(() => {
    setChordIndex(0);
    prevChordRef.current = null;
    wasEmptyRef.current = true;
  }, [exercise?.id]);

  // Check detected chord against target
  useEffect(() => {
    if (!targetChords || !currentTarget || !detectedChord) return;
    if (chordIndex >= targetChords.length) return;

    // Only register when notes were released in between (wasEmpty)
    if (activeNotes.size === 0) {
      wasEmptyRef.current = true;
      return;
    }

    if (!wasEmptyRef.current) return;

    const chordKey = `${detectedChord.root}-${detectedChord.quality}`;
    if (chordKey === prevChordRef.current) return;
    prevChordRef.current = chordKey;

    const correct = detectedChord.root === currentTarget.root && detectedChord.quality === currentTarget.quality;
    onChordMatch?.(correct);
    wasEmptyRef.current = false;

    if (correct) {
      const nextIdx = chordIndex + 1;
      if (nextIdx >= targetChords.length) {
        setChordIndex(nextIdx);
        onSequenceComplete?.();
      } else {
        setChordIndex(nextIdx);
      }
    }
  }, [detectedChord, activeNotes.size, targetChords, currentTarget, chordIndex, onChordMatch, onSequenceComplete]);

  return (
    <div className="flex flex-1 flex-col gap-6 p-6">
      <div>
        <h2 className="text-sm font-semibold text-muted-foreground">
          {isExercise ? exercise.title : 'Detected Chord'}
        </h2>
        {exercise?.config.instructions && (
          <p className="mt-1 text-xs text-muted-foreground">{exercise.config.instructions}</p>
        )}
        <div
          className="mt-2 flex h-24 items-center justify-center rounded-xl"
          style={{ backgroundColor: 'oklch(0.14 0.01 270)' }}
        >
          {detectedChord ? (
            <span className="text-4xl font-bold">{detectedChord.display}</span>
          ) : (
            <span className="text-lg text-muted-foreground">Play a chord...</span>
          )}
        </div>
        {detectedChord && (
          <div className="mt-2 flex items-center gap-2">
            <span className="text-xs text-muted-foreground">Root: {detectedChord.root}</span>
            <span className="text-xs text-muted-foreground">Quality: {detectedChord.quality}</span>
            <span className="text-xs text-muted-foreground">
              Notes: {Array.from(activeNotes.keys()).sort((a, b) => a - b).map((n) => {
                const names = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'];
                return names[n % 12];
              }).join(', ')}
            </span>
          </div>
        )}
      </div>

      {/* Chord sequence progress */}
      {targetChords && (
        <div className="flex flex-wrap items-center gap-1.5">
          {targetChords.map((tc, i) => (
            <span
              key={i}
              className="rounded-md px-3 py-1.5 text-xs font-medium transition-colors"
              style={{
                backgroundColor:
                  i < chordIndex
                    ? 'oklch(0.30 0.15 150)'
                    : i === chordIndex
                    ? 'oklch(0.30 0.12 270)'
                    : 'oklch(0.18 0.01 270)',
                color: i <= chordIndex ? 'white' : 'oklch(0.55 0.01 270)',
              }}
            >
              {tc.display}
            </span>
          ))}
        </div>
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
