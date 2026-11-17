import { useState } from 'react';
import { ChevronDown, ChevronUp, Piano, Drum } from 'lucide-react';
import { PadGrid } from '@/components/beat/PadGrid';
import { useActiveKitPads } from '@/hooks/useBeat';
import { arrangementEngine } from '@/services/ArrangementEngine';
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
  onDrumHit: (padId: DrumPadId, velocity: number) => void;
  routingMode: RoutingMode;
  onRoutingModeChange: (mode: RoutingMode) => void;
  /** Chord recognised from the held notes (shown in the header readout). */
  detectedChord?: ChordInfo | null;
  /** Offer the small drum pads (off in the beat view, which has its own pads). */
  padsAvailable?: boolean;
  /** Start with the keys hidden (just the header strip). */
  defaultCollapsed?: boolean;
}

type Bank = 'A' | 'B';
const BANK_KEY = 'micro-daw:dock-pad-bank';

function readBank(): Bank {
  try {
    return localStorage.getItem(BANK_KEY) === 'B' ? 'B' : 'A';
  } catch {
    return 'A';
  }
}

/**
 * The active kit's 16 pads as the MPK Mini shows them: one bank of 8 (2×4) at a
 * time with an A/B bank switch. Same pads (names, colours, A1–B8 labels, hit
 * flashes) as the beat view's PadGrid.
 */
function DockPads({ onHit }: { onHit: (pad: DrumPadId, velocity: number) => void }) {
  const { pads, selectedPad } = useActiveKitPads();
  const [bank, setBankState] = useState<Bank>(readBank);
  const setBank = (b: Bank) => {
    setBankState(b);
    try {
      localStorage.setItem(BANK_KEY, b);
    } catch {
      // ignore (private mode / blocked storage)
    }
  };
  return (
    <div className="flex w-[340px] shrink-0 items-stretch gap-1.5">
      <div role="radiogroup" aria-label="Pad bank" className="flex w-7 shrink-0 flex-col gap-0.5 rounded-md border border-border bg-surface-0 p-0.5">
        {(['B', 'A'] as const).map((b) => (
          <button
            key={b}
            type="button"
            role="radio"
            aria-checked={bank === b}
            aria-label={`Bank ${b} (pads ${b}1–${b}8)`}
            title={`Bank ${b}: pads ${b}1–${b}8`}
            onClick={() => setBank(b)}
            className={cn(
              'flex-1 rounded-[4px] font-mono text-[11px] font-semibold outline-none focus-visible:ring-2 focus-visible:ring-ring/70',
              bank === b ? 'bg-surface-3 text-foreground' : 'text-muted-foreground hover:text-foreground',
            )}
          >
            {b}
          </button>
        ))}
      </div>
      <PadGrid
        className="min-w-0 flex-1"
        pads={pads}
        selectedPad={selectedPad}
        bank={bank}
        compact
        showKeyHints={false}
        onHit={onHit}
        onSelect={(pad) => arrangementEngine.setSelectedPad(pad)}
      />
    </div>
  );
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
  onDrumHit,
  routingMode,
  onRoutingModeChange,
  detectedChord,
  padsAvailable = true,
  defaultCollapsed = false,
}: InstrumentDockProps) {
  const [showPadsState, setShowPads] = useState(!defaultCollapsed);
  const [showKeys, setShowKeys] = useState(!defaultCollapsed);
  const showPads = padsAvailable && showPadsState;

  const collapsed = !showPads && !showKeys;
  const notes = Array.from(activeNotes.keys()).sort((a, b) => a - b);

  return (
    <div className="shrink-0 border-t border-border bg-surface-1">
      {/* Header */}
      <div className={cn('flex h-9 items-center gap-2 px-3', !collapsed && 'border-b border-hairline')}>
        <span className="daw-label mr-1 text-[11px]">Play</span>
        <div className="flex items-center gap-0.5">
          {padsAvailable && (
            <ViewToggle on={showPads} onClick={() => setShowPads((p) => !p)} icon={Drum}>
              Pads
            </ViewToggle>
          )}
          <ViewToggle on={showKeys} onClick={() => setShowKeys((p) => !p)} icon={Piano}>
            Keys
          </ViewToggle>
        </div>

        <div className="mx-1 h-4 w-px bg-border" />
        <span className="daw-label text-[11px]">MIDI route</span>
        <RoutingModeSelector mode={routingMode} onChange={onRoutingModeChange} />

        {/* Live readout */}
        <div className="ml-auto flex min-w-0 items-center gap-2 font-mono text-[11px]" aria-live="polite">
          <span
            className={cn(
              'min-w-[52px] rounded-[3px] px-1.5 py-0.5 text-center font-semibold',
              detectedChord ? 'bg-primary/15 text-primary' : 'text-muted-foreground',
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
              : <span className="text-muted-foreground">no notes held</span>}
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
          {showPads && <DockPads onHit={onDrumHit} />}
          {showKeys && (
            <div className="flex min-w-0 flex-1 items-end overflow-x-auto overflow-y-hidden">
              <PianoRoll
                activeNotes={activeNotes}
                onNoteOn={onNoteOn}
                onNoteOff={onNoteOff}
                compact
              />
            </div>
          )}
        </div>
      )}
    </div>
  );
}
