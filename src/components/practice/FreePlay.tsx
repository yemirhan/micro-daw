import { PianoRoll } from '@/components/PianoRoll';
import { DrumPads } from '@/components/DrumPads';
import { NoteChip, Panel } from '@/components/learn/kit';
import { InstrumentWell, StepDescription } from '@/components/learn/StepStage';
import { midiToNoteName } from '@/utils/noteHelpers';
import type { ActiveNote } from '@/hooks/useMidiNotes';
import type { ChordInfo } from '@/types/music';
import type { DrumPadId } from '@/types/drums';

interface FreePlayProps {
  activeNotes: Map<number, ActiveNote>;
  onNoteOn: (note: number, velocity: number) => void;
  onNoteOff: (note: number) => void;
  activePads: Set<DrumPadId>;
  onDrumHit: (padId: DrumPadId, velocity: number) => void;
  detectedChord: ChordInfo | null;
}

export function FreePlay({ activeNotes, onNoteOn, onNoteOff, activePads, onDrumHit, detectedChord }: FreePlayProps) {
  const held = Array.from(activeNotes.keys()).sort((a, b) => a - b);

  return (
    <div className="mx-auto flex w-full max-w-[1080px] flex-col gap-5 px-10 py-8">
      <StepDescription>No score, no clock. Play the keys and pads; chords are named as you play them.</StepDescription>

      <div className="grid grid-cols-[1fr_auto] gap-5">
        <Panel className="flex flex-col justify-between gap-4 px-6 py-5">
          <div>
            <span className="text-[11px] font-bold uppercase tracking-[0.08em] text-muted-foreground">Chord</span>
            <div
              className={
                detectedChord
                  ? 'mt-3 text-7xl font-semibold leading-none tracking-tight text-foreground'
                  : 'mt-3 text-7xl font-semibold leading-none tracking-tight text-muted-foreground/30'
              }
              aria-live="polite"
            >
              {detectedChord?.display ?? '--'}
            </div>
          </div>
          <div className="flex min-h-7 flex-wrap items-center gap-1.5">
            {held.length > 0 ? (
              held.map((n) => (
                <NoteChip key={n} size="sm" state="active">
                  {midiToNoteName(n)}
                </NoteChip>
              ))
            ) : (
              <span className="text-sm text-muted-foreground">Notes you hold show up here</span>
            )}
          </div>
        </Panel>
        <InstrumentWell label={<span>Pads</span>} className="w-[340px]">
          <div className="w-full">
            <DrumPads activePads={activePads} onHit={onDrumHit} />
          </div>
        </InstrumentWell>
      </div>

      <InstrumentWell>
        <PianoRoll activeNotes={activeNotes} onNoteOn={onNoteOn} onNoteOff={onNoteOff} compact />
      </InstrumentWell>
    </div>
  );
}
