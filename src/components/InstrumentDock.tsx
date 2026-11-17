import { useState } from 'react';
import { ChevronDown, ChevronUp, Piano, Drum } from 'lucide-react';
import { DrumPads } from '@/components/DrumPads';
import { PianoRoll } from '@/components/PianoRoll';
import { RoutingModeSelector } from '@/components/RoutingModeSelector';
import { Button } from '@/components/ui/button';
import { Tip } from '@/components/ui/tip';
import { midiToNoteName } from '@/utils/noteHelpers';
import { cn } from '@/lib/utils';
import type { ActiveNote } from '@/hooks/useMidiNotes';
import type { DrumPadId } from '@/types/drums';
import type { ChordInfo } from '@/types/music';
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
  /** Chord recognised from the held notes (shown in the header readout). */
  detectedChord?: ChordInfo | null;
}

function ViewToggle({ on, onClick, icon: Icon, children }: { on: boolean; onClick: () => void; icon: typeof Piano; children: string }) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-pressed={on}
      className={cn(
        'flex h-6 items-center gap-1.5 rounded-[4px] px-2 text-[11px] font-semibold transition-colors duration-150 focus-visible:outline-2 focus-visible:outline-ring',
        on ? 'bg-surface-3 text-foreground' : 'text-muted-foreground hover:bg-surface-3 hover:text-foreground',
      )}
    >
      <Icon className="size-3.5" />
      {children}
    </button>
  );
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
  detectedChord,
}: InstrumentDockProps) {
  const [showPads, setShowPads] = useState(true);
  const [showKeys, setShowKeys] = useState(true);

  const collapsed = !showPads && !showKeys;
  const notes = Array.from(activeNotes.keys()).sort((a, b) => a - b);

  return (
    <div data-tour="instrument-dock" className="shrink-0 border-t border-border bg-surface-1">
      {/* Header */}
      <div className={cn('flex h-9 items-center gap-2 px-3', !collapsed && 'border-b border-hairline')}>
        <span className="daw-label mr-1">Play</span>
        <div className="flex items-center gap-0.5">
          <ViewToggle on={showPads} onClick={() => setShowPads((p) => !p)} icon={Drum}>
            Pads
          </ViewToggle>
          <ViewToggle on={showKeys} onClick={() => setShowKeys((p) => !p)} icon={Piano}>
            Keys
          </ViewToggle>
        </div>

        <div className="mx-1 h-4 w-px bg-border" />
        <span className="daw-label">MIDI route</span>
        <RoutingModeSelector mode={routingMode} onChange={onRoutingModeChange} />

        {/* Live readout */}
        <div className="ml-auto flex min-w-0 items-center gap-2 font-mono text-[11px]" aria-live="polite">
          <span
            className={cn(
              'min-w-[52px] rounded-[3px] px-1.5 py-0.5 text-center font-semibold',
              detectedChord ? 'bg-primary/15 text-primary' : 'text-muted-foreground/50',
            )}
            title="Detected chord"
          >
            {detectedChord ? detectedChord.display : '—'}
          </span>
          <span className="flex min-w-[140px] items-center gap-1.5 overflow-hidden text-muted-foreground">
            {notes.length > 0
              ? notes.slice(0, 8).map((n) => (
                  <span key={n} className="animate-note-pop text-foreground">
                    {midiToNoteName(n)}
                  </span>
                ))
              : <span className="text-muted-foreground/50">no notes held</span>}
          </span>
        </div>

        <Tip label={collapsed ? 'Show instruments' : 'Hide instruments'}>
          <Button
            variant="ghost"
            size="icon-xs"
            aria-label={collapsed ? 'Expand instrument dock' : 'Collapse instrument dock'}
            onClick={() => {
              if (collapsed) {
                setShowPads(true);
                setShowKeys(true);
              } else {
                setShowPads(false);
                setShowKeys(false);
              }
            }}
          >
            {collapsed ? <ChevronUp /> : <ChevronDown />}
          </Button>
        </Tip>
      </div>

      {/* Instruments */}
      {!collapsed && (
        <div className="flex h-[156px] items-stretch gap-3 px-3 py-2.5">
          {showPads && (
            <div className="flex w-[216px] shrink-0 items-end">
              <div className="w-full">
                <DrumPads activePads={activePads} onHit={onDrumHit} compact />
              </div>
            </div>
          )}
          {showKeys && (
            <div className="flex min-w-0 flex-1 items-end overflow-x-auto overflow-y-hidden">
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
