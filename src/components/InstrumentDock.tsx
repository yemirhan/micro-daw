import { DrumPads } from '@/components/DrumPads';
import { PianoRoll } from '@/components/PianoRoll';
import type { ActiveNote } from '@/hooks/useMidiNotes';
import type { DrumPadId } from '@/types/drums';

interface InstrumentDockProps {
  activeNotes: Map<number, ActiveNote>;
  onNoteOn: (note: number, velocity: number) => void;
  onNoteOff: (note: number) => void;
  highlightedNotes?: Set<number>;
  activePads: Set<DrumPadId>;
  onDrumHit: (padId: DrumPadId, velocity: number) => void;
}

export function InstrumentDock({
  activeNotes,
  onNoteOn,
  onNoteOff,
  highlightedNotes,
  activePads,
  onDrumHit,
}: InstrumentDockProps) {
  return (
    <div
      className="flex h-[140px] shrink-0 items-end gap-3 border-t px-3 py-2"
      style={{
        backgroundColor: 'oklch(0.15 0.012 270 / 0.6)',
        backdropFilter: 'blur(12px)',
        borderColor: 'oklch(0.28 0.02 270)',
      }}
    >
      <div className="w-[200px] shrink-0">
        <DrumPads activePads={activePads} onHit={onDrumHit} compact />
      </div>
      <div className="flex-1 overflow-x-auto overflow-y-hidden">
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
