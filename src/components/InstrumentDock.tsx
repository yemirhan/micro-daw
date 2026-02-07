import { useState } from 'react';
import { ChevronDown, ChevronUp, Piano, Drum } from 'lucide-react';
import { DrumPads } from '@/components/DrumPads';
import { PianoRoll } from '@/components/PianoRoll';
import { RoutingModeSelector } from '@/components/RoutingModeSelector';
import { cn } from '@/lib/utils';
import type { ActiveNote } from '@/hooks/useMidiNotes';
import type { DrumPadId } from '@/types/drums';
import type { RoutingMode } from '@/utils/constants';

interface InstrumentDockProps {
  activeNotes: Map<number, ActiveNote>;
  onNoteOn: (note: number, velocity: number) => void;
  onNoteOff: (note: number) => void;
  highlightedNotes?: Set<number>;
  activePads: Set<DrumPadId>;
  onDrumHit: (padId: DrumPadId, velocity: number) => void;
  routingMode: RoutingMode;
  onRoutingModeChange: (mode: RoutingMode) => void;
}

export function InstrumentDock({
  activeNotes,
  onNoteOn,
  onNoteOff,
  highlightedNotes,
  activePads,
  onDrumHit,
  routingMode,
  onRoutingModeChange,
}: InstrumentDockProps) {
  const [showPads, setShowPads] = useState(true);
  const [showKeys, setShowKeys] = useState(true);

  const collapsed = !showPads && !showKeys;

  return (
    <div
      className="shrink-0 border-t"
      style={{
        backgroundColor: 'oklch(0.15 0.012 270 / 0.6)',
        backdropFilter: 'blur(12px)',
        borderColor: 'oklch(0.28 0.02 270)',
      }}
    >
      {/* Toggle bar */}
      <div
        className="flex items-center gap-1 px-3 py-1"
        style={{ borderBottom: collapsed ? 'none' : '1px solid oklch(0.28 0.02 270 / 0.5)' }}
      >
        <button
          onClick={() => setShowPads((p) => !p)}
          className={cn(
            'flex items-center gap-1 rounded px-2 py-0.5 text-[11px] font-medium transition-colors',
            showPads
              ? 'bg-white/10 text-white/90'
              : 'text-white/40 hover:text-white/60',
          )}
        >
          <Drum className="h-3 w-3" />
          Pads
        </button>
        <button
          onClick={() => setShowKeys((p) => !p)}
          className={cn(
            'flex items-center gap-1 rounded px-2 py-0.5 text-[11px] font-medium transition-colors',
            showKeys
              ? 'bg-white/10 text-white/90'
              : 'text-white/40 hover:text-white/60',
          )}
        >
          <Piano className="h-3 w-3" />
          Keys
        </button>

        <div className="mx-1 h-4 w-px bg-white/10" />
        <RoutingModeSelector mode={routingMode} onChange={onRoutingModeChange} />

        <button
          onClick={() => {
            if (collapsed) {
              setShowPads(true);
              setShowKeys(true);
            } else {
              setShowPads(false);
              setShowKeys(false);
            }
          }}
          className="ml-auto flex items-center text-white/40 hover:text-white/60"
        >
          {collapsed ? <ChevronUp className="h-3.5 w-3.5" /> : <ChevronDown className="h-3.5 w-3.5" />}
        </button>
      </div>

      {/* Instruments */}
      {!collapsed && (
        <div className="flex h-[130px] items-end gap-3 px-3 py-2">
          {showPads && (
            <div className="w-[200px] shrink-0">
              <DrumPads activePads={activePads} onHit={onDrumHit} compact />
            </div>
          )}
          {showKeys && (
            <div className="flex-1 overflow-x-auto overflow-y-hidden">
              <PianoRoll
                activeNotes={activeNotes}
                onNoteOn={onNoteOn}
                onNoteOff={onNoteOff}
                highlightedNotes={highlightedNotes}
                compact
              />
            </div>
          )}
        </div>
      )}
    </div>
  );
}
