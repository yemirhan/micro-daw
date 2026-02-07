import { PianoRoll } from '@/components/PianoRoll';
import { DrumPads } from '@/components/DrumPads';
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
  return (
    <div className="flex flex-1 flex-col gap-6 p-6">
      <div>
        <h2 className="text-sm font-semibold text-muted-foreground">Free Play</h2>
        <p className="mt-1 text-xs text-muted-foreground">
          Play freely with piano and drums. Chords are detected automatically.
        </p>
      </div>

      {/* Chord display */}
      <div
        className="flex h-16 items-center justify-center rounded-xl"
        style={{ backgroundColor: 'oklch(0.14 0.01 270)' }}
      >
        {detectedChord ? (
          <span className="text-3xl font-bold">{detectedChord.display}</span>
        ) : (
          <span className="text-sm text-muted-foreground">Play notes to detect chords</span>
        )}
      </div>

      <div className="flex gap-4">
        <div className="w-[220px] shrink-0">
          <DrumPads activePads={activePads} onHit={onDrumHit} />
        </div>
        <div className="flex-1 overflow-x-auto">
          <PianoRoll
            activeNotes={activeNotes}
            onNoteOn={onNoteOn}
            onNoteOff={onNoteOff}
            compact
          />
        </div>
      </div>
    </div>
  );
}
