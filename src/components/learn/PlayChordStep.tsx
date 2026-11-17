import { PianoRoll } from '@/components/PianoRoll';
import type { LessonStep } from '@/types/appMode';
import type { ActiveNote } from '@/hooks/useMidiNotes';
import type { ChordInfo } from '@/types/music';
import { chordPitchClasses, noteNameToPitchClass, pitchClass } from '@/utils/lessonMatching';
import { cn } from '@/lib/utils';
import { NoteChip } from './kit';
import { InstrumentWell, PlayStatus, StepDescription, StepHint, noteLabel, useWrongNoteFlash } from './StepStage';

const PC_NAMES = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'];

interface PlayChordStepProps {
  step: LessonStep;
  activeNotes: Map<number, ActiveNote>;
  onNoteOn: (note: number, velocity: number) => void;
  onNoteOff: (note: number) => void;
  detectedChord: ChordInfo | null;
  completed: boolean;
  pendingRelease?: boolean;
}

/** Chord tones in order from the root (so C E G, not E G C). */
function orderedTones(pcs: Set<number>, root: number | undefined): number[] {
  const list = [...pcs];
  if (root === undefined) return list.sort((a, b) => a - b);
  return list.sort((a, b) => ((a - root + 12) % 12) - ((b - root + 12) % 12));
}

export function PlayChordStep({
  step,
  activeNotes,
  onNoteOn,
  onNoteOff,
  detectedChord,
  completed,
  pendingRelease = false,
}: PlayChordStepProps) {
  const chord = step.expectedChord;
  const pcs = chord ? chordPitchClasses(chord) : null;
  const root = chord ? noteNameToPitchClass(chord.root) : undefined;
  const tones = pcs ? orderedTones(pcs, root) : [];
  const heldPcs = new Set([...activeNotes.keys()].map(pitchClass));

  const flash = useWrongNoteFlash(
    (note) => !!pcs && !pcs.has(pitchClass(note)),
    !completed && !pendingRelease && !!pcs,
    (note) => noteLabel(note, true),
  );

  const playingMatches = !!detectedChord && !!chord && detectedChord.display === chord.display;
  const prompt = chord ? (
    <>
      Play <span className="font-semibold text-note-target">{chord.display}</span>
      {tones.length > 0 && (
        <span className="text-muted-foreground">
          {' '}
          ({tones.map((pc) => PC_NAMES[pc]).join(' ')}, any inversion)
        </span>
      )}
    </>
  ) : (
    'Play the chord'
  );

  return (
    <div className="flex flex-col gap-5">
      <StepDescription>{step.description}</StepDescription>

      {chord && (
        <div className="flex flex-wrap items-stretch gap-3">
          <div className="flex min-w-36 flex-col justify-center rounded-xl border border-note-target/40 bg-note-target/8 px-5 py-3">
            <span className="text-[11px] font-bold uppercase tracking-[0.08em] text-muted-foreground">Target</span>
            <span className="text-3xl font-semibold tracking-tight text-foreground">{chord.display}</span>
          </div>
          <div
            className={cn(
              'flex min-w-36 flex-col justify-center rounded-xl border px-5 py-3 transition-colors duration-150',
              completed || playingMatches
                ? 'border-note-correct/50 bg-note-correct/10'
                : detectedChord
                  ? 'border-border bg-surface-2'
                  : 'border-dashed border-border bg-transparent',
            )}
          >
            <span className="text-[11px] font-bold uppercase tracking-[0.08em] text-muted-foreground">You're playing</span>
            <span
              className={cn(
                'text-3xl font-semibold tracking-tight',
                completed || playingMatches ? 'text-note-correct' : detectedChord ? 'text-foreground' : 'text-muted-foreground/50',
              )}
            >
              {detectedChord?.display ?? '--'}
            </span>
          </div>
          {tones.length > 0 && (
            <div className="flex flex-col justify-center gap-2 px-2">
              <span className="text-[11px] font-bold uppercase tracking-[0.08em] text-muted-foreground">Chord tones</span>
              <div className="flex gap-2">
                {tones.map((pc) => (
                  <NoteChip key={pc} state={completed ? 'done' : heldPcs.has(pc) ? 'held' : 'current'}>
                    {PC_NAMES[pc]}
                  </NoteChip>
                ))}
              </div>
            </div>
          )}
        </div>
      )}

      <PlayStatus completed={completed} pendingRelease={pendingRelease} flash={flash} prompt={prompt} />

      {step.hint && !completed && <StepHint>{step.hint}</StepHint>}

      <InstrumentWell>
        <PianoRoll
          activeNotes={activeNotes}
          onNoteOn={onNoteOn}
          onNoteOff={onNoteOff}
          highlightedNotes={completed || !pcs ? undefined : pcs}
          compact
        />
      </InstrumentWell>
    </div>
  );
}
