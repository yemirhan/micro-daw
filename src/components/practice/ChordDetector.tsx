import { useRef, useState } from 'react';
import { useNoteEvents } from '@/hooks/useNoteEvents';
import { createChordJudge } from '@/utils/chordJudge';
import { PianoRoll } from '@/components/PianoRoll';
import { NoteChip, StatusLine } from '@/components/learn/kit';
import { InstrumentWell, StepDescription } from '@/components/learn/StepStage';
import { cn } from '@/lib/utils';
import { midiToNoteName } from '@/utils/noteHelpers';
import type { ActiveNote } from '@/hooks/useMidiNotes';
import type { ChordInfo } from '@/types/music';
import type { PracticeExercise } from '@/types/appMode';

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
  const targetChords = exercise?.config.targetChords;
  const [chordIndex, setChordIndex] = useState(0);
  const [lastAttempt, setLastAttempt] = useState<{ correct: boolean; display: string | null; n: number } | null>(null);
  const judgeRef = useRef(createChordJudge());
  const attemptsRef = useRef(0);

  const currentTarget = targetChords?.[chordIndex];

  // (Mounted fresh per exercise attempt, see ExerciseRunner.)

  // Judge chord attempts from every note event (MIDI keyboard or on-screen keys).
  useNoteEvents((event) => {
    if (!targetChords) return;
    const judge = judgeRef.current;
    const target = targetChords[judge.index];
    const attempt = judge.handle(event, target);
    if (!attempt) return;
    onChordMatch?.(attempt.correct);
    attemptsRef.current += 1;
    setLastAttempt({ ...attempt, n: attemptsRef.current });
    if (attempt.correct) {
      setChordIndex(judge.index);
      if (judge.index >= targetChords.length) onSequenceComplete?.();
    }
  }, !!targetChords);

  const held = Array.from(activeNotes.keys()).sort((a, b) => a - b);
  const matchesTarget = !!detectedChord && !!currentTarget && detectedChord.display === currentTarget.display;

  return (
    <div className="mx-auto flex w-full max-w-[1080px] flex-col gap-5 px-10 py-8">
      <StepDescription>
        {exercise?.config.instructions ??
          'Hold any combination of keys. The chord name, root and quality update as you play.'}
      </StepDescription>

      {/* Chord sequence progress */}
      {targetChords && (
        <div className="flex flex-col gap-2">
          <span className="text-[11px] font-bold uppercase tracking-[0.08em] text-muted-foreground">
            Chords to play · {Math.min(chordIndex, targetChords.length)}/{targetChords.length}
          </span>
          <div className="flex flex-wrap items-center gap-2">
            {targetChords.map((tc, i) => (
              <NoteChip
                key={i}
                size="lg"
                className="font-sans"
                state={i < chordIndex ? 'done' : i === chordIndex ? 'current' : 'todo'}
              >
                {tc.display}
              </NoteChip>
            ))}
          </div>
        </div>
      )}

      {targetChords && currentTarget && (
        <div data-testid="chord-target">
          {lastAttempt && !lastAttempt.correct ? (
            <StatusLine tone="error" flashKey={lastAttempt.n}>
              {lastAttempt.display ? (
                <>
                  That was <span className="font-semibold">{lastAttempt.display}</span>.
                </>
              ) : (
                'Not quite.'
              )}{' '}
              Release and try <span className="font-semibold">{currentTarget.display}</span> again.
            </StatusLine>
          ) : (
            <StatusLine tone={lastAttempt?.correct ? 'success' : 'prompt'} flashKey={chordIndex}>
              {lastAttempt?.correct ? 'Yes! ' : ''}
              {lastAttempt?.correct ? 'Next: ' : 'Play '}
              <span className="font-semibold text-note-target">{currentTarget.display}</span>
              <span className="text-muted-foreground"> (any inversion)</span>
            </StatusLine>
          )}
        </div>
      )}

      {/* Live readout */}
      <div
        className={cn(
          'flex items-center gap-8 rounded-xl border px-6 py-5 transition-colors duration-150',
          matchesTarget ? 'border-note-correct/50 bg-note-correct/8' : 'border-border bg-surface-1',
        )}
      >
        <div className="min-w-48">
          <span className="text-[11px] font-bold uppercase tracking-[0.08em] text-muted-foreground">
            {targetChords ? "You're playing" : 'Detected chord'}
          </span>
          <div
            className={cn(
              'font-semibold leading-none tracking-tight',
              targetChords ? 'mt-2 text-5xl' : 'mt-3 text-7xl',
              detectedChord ? (matchesTarget ? 'text-note-correct' : 'text-foreground') : 'text-muted-foreground/40',
            )}
            aria-live="polite"
          >
            {detectedChord?.display ?? '--'}
          </div>
        </div>
        <div className="flex flex-col gap-2">
          {detectedChord && (
            <div className="flex gap-4 text-xs text-muted-foreground">
              <span>
                Root <span className="font-mono font-semibold text-foreground">{detectedChord.root}</span>
              </span>
              <span>
                Quality <span className="font-semibold capitalize text-foreground">{detectedChord.quality}</span>
              </span>
            </div>
          )}
          <div className="flex min-h-9 flex-wrap items-center gap-1.5">
            {held.length > 0 ? (
              held.map((n) => (
                <NoteChip key={n} size="sm" state="active">
                  {midiToNoteName(n)}
                </NoteChip>
              ))
            ) : (
              <span className="text-sm text-muted-foreground">Hold three or more keys…</span>
            )}
          </div>
        </div>
      </div>

      <InstrumentWell>
        <PianoRoll activeNotes={activeNotes} onNoteOn={onNoteOn} onNoteOff={onNoteOff} compact />
      </InstrumentWell>
    </div>
  );
}
