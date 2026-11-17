import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { Keyboard, Settings2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Tip } from '@/components/ui/tip';
import type { BeatController } from '@/hooks/useBeat';
import { useControllerMapping } from '@/hooks/useControllerMapping';
import { useNoteRepeat } from '@/hooks/useNoteRepeat';
import { cn } from '@/lib/utils';
import type { PadIndex } from '@/types/sampler';
import { BeatTransport } from './BeatTransport';
import { KitMenu } from './KitMenu';
import { isTypingTarget, padForKeyEvent, type PadLayout, type StepGrid } from './beatHelpers';
import { PadEditor } from './PadEditor';
import { PadGrid } from './PadGrid';
import { StepSequencer } from './StepSequencer';

export interface BeatViewProps {
  /** Beat state + actions (`useBeat()`, owned by the app so its shortcuts share it). */
  beat: BeatController;
  /** Pad pressed on screen or with a computer key (useMidiNotes' drumPress: heard, recorded, note-repeated). */
  onPadHit: (pad: PadIndex, velocity: number) => void;
  /** Pad released (useMidiNotes' drumRelease). */
  onPadRelease: (pad: PadIndex) => void;
  /** One-shot audition from the pad editor (defaults to onPadHit). */
  onPadPreview?: (pad: PadIndex, velocity: number) => void;
  /** Opens the MIDI controller setup dialog. */
  onOpenControllerSetup?: () => void;
  /** Opens the export dialog (WAV / MIDI). */
  onExport?: () => void;
  /** Left panel (the sample browser). */
  browser?: ReactNode;
  className?: string;
}

const LAYOUT_KEY = 'micro-daw:beat-pad-layout';
const GRID_KEY = 'micro-daw:beat-step-grid';
const KEYS_KEY = 'micro-daw:beat-key-hints';

function readPref<T extends string>(key: string, allowed: readonly T[], fallback: T): T {
  try {
    const v = localStorage.getItem(key);
    return v && (allowed as readonly string[]).includes(v) ? (v as T) : fallback;
  } catch {
    return fallback;
  }
}
function writePref(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // ignore (private mode / blocked storage)
  }
}

/** Whether a modal (dialog, open menu) owns the keyboard right now. */
function modalOpen(): boolean {
  return !!document.querySelector('[role="dialog"][data-state="open"], [role="alertdialog"][data-state="open"]');
}

/**
 * Beat mode home screen: transport strip on top; pads (left) and the selected
 * pad's editor (right); the step sequencer fills the rest. Computer keys
 * Z X C V / A S D F / Q W E T / 1 2 3 4 play pads 1–16 (MPC layout); R toggles
 * loop recording.
 */
export function BeatView({ beat, onPadHit, onPadRelease, onPadPreview, onOpenControllerSetup, onExport, browser, className }: BeatViewProps) {
  const { actions } = beat;
  const { profile } = useControllerMapping();
  const noteRepeat = useNoteRepeat();

  const [layout, setLayout] = useState<PadLayout>(() => readPref(LAYOUT_KEY, ['4x4', '2x8'] as const, '4x4'));
  const [grid, setGrid] = useState<StepGrid>(() => readPref(GRID_KEY, ['1/8', '1/16', '1/32'] as const, '1/16'));
  const [showKeys, setShowKeys] = useState(() => readPref(KEYS_KEY, ['on', 'off'] as const, 'off') === 'on');
  const [chopRequest, setChopRequest] = useState(0);

  const changeLayout = (l: PadLayout) => {
    setLayout(l);
    writePref(LAYOUT_KEY, l);
  };
  const changeGrid = (g: StepGrid) => {
    setGrid(g);
    writePref(GRID_KEY, g);
  };
  const toggleKeys = () => {
    setShowKeys((s) => {
      writePref(KEYS_KEY, s ? 'off' : 'on');
      return !s;
    });
  };

  // --- Computer keyboard pads --------------------------------------------------
  const hitRef = useRef({ onPadHit, onPadRelease, toggleRecord: actions.toggleRecord });
  useEffect(() => {
    hitRef.current = { onPadHit, onPadRelease, toggleRecord: actions.toggleRecord };
  });
  useEffect(() => {
    const held = new Map<string, PadIndex>();
    const onDown = (e: KeyboardEvent) => {
      if (isTypingTarget(e.target) || modalOpen()) return;
      // R (or Shift+R) = loop record.
      if (e.code === 'KeyR' && !e.metaKey && !e.ctrlKey && !e.altKey) {
        e.preventDefault();
        e.stopImmediatePropagation();
        if (!e.repeat) hitRef.current.toggleRecord();
        return;
      }
      if (e.shiftKey) return;
      const pad = padForKeyEvent(e);
      if (pad === null) return;
      // Pad keys win over the app's single-key shortcuts while beat mode is up.
      e.preventDefault();
      e.stopImmediatePropagation();
      if (e.repeat || held.has(e.code)) return;
      held.set(e.code, pad);
      hitRef.current.onPadHit(pad, 0.85);
    };
    const onUp = (e: KeyboardEvent) => {
      const pad = held.get(e.code);
      if (pad === undefined) return;
      held.delete(e.code);
      hitRef.current.onPadRelease(pad);
    };
    const releaseAll = () => {
      for (const pad of held.values()) hitRef.current.onPadRelease(pad);
      held.clear();
    };
    // Capture phase: runs before the app's window-level shortcut handler.
    window.addEventListener('keydown', onDown, true);
    window.addEventListener('keyup', onUp, true);
    window.addEventListener('blur', releaseAll);
    return () => {
      window.removeEventListener('keydown', onDown, true);
      window.removeEventListener('keyup', onUp, true);
      window.removeEventListener('blur', releaseAll);
      releaseAll();
    };
  }, []);

  const selected = beat.selectedPad;
  const selectedSettings = beat.kit.pads[selected];
  const playing = beat.transportState !== 'stopped';
  const counting = beat.countInRemaining > 0;

  const onChop = useCallback((pad: PadIndex) => {
    actions.selectPad(pad);
    setChopRequest((n) => n + 1);
  }, [actions]);

  return (
    <div className={cn('flex h-full min-h-0 w-full min-w-0 flex-col bg-surface-0', className)}>
      {/* Transport strip */}
      <div className="shrink-0 border-b border-border bg-surface-1 px-3 py-2">
        <BeatTransport
          playing={playing}
          recording={beat.isLoopRecording && !counting}
          countInRemaining={beat.countInRemaining}
          bpm={beat.bpm}
          loop={beat.loop}
          patternStartBeat={beat.patternStartBeat}
          subscribePosition={actions.subscribePosition}
          onTogglePlay={actions.togglePlay}
          onToggleRecord={actions.toggleRecord}
          onBpmChange={actions.setBpm}
          onLoopChange={actions.setLoopSettings}
          onExport={onExport}
          noteRepeat={{
            enabled: noteRepeat.enabled,
            rate: noteRepeat.rate,
            onToggle: noteRepeat.toggle,
            onRateChange: noteRepeat.setRate,
          }}
        />
      </div>

      <div className="flex min-h-0 flex-1">
        {browser && <aside className="flex w-[260px] shrink-0 flex-col border-r border-border bg-surface-1">{browser}</aside>}

        <div className="flex min-h-0 min-w-0 flex-1 flex-col">
          {/* Pads + editor */}
          <div className="flex h-[min(46vh,372px)] shrink-0 border-b border-border">
            <section
              aria-label="Pads"
              data-show-keys={showKeys || undefined}
              className={cn('relative flex shrink-0 flex-col border-r border-border p-3', layout === '4x4' ? 'w-[392px]' : 'w-[600px]')}
            >
              <header className="mb-2 flex h-6 items-center gap-1.5">
                <KitMenu
                  className="-ml-1.5"
                  kitName={beat.kit.name}
                  userKits={beat.userKits}
                  onLoadFactoryKit={actions.loadFactoryKit}
                  onLoadKit={actions.loadKit}
                  onSaveKit={actions.saveKit}
                  onDeleteKit={actions.deleteKit}
                />
                <div role="radiogroup" aria-label="Pad layout" className="flex h-6 items-center rounded-md border border-border bg-surface-0 p-0.5">
                  {(['4x4', '2x8'] as const).map((l) => (
                    <button
                      key={l}
                      type="button"
                      role="radio"
                      aria-checked={layout === l}
                      onClick={() => changeLayout(l)}
                      className={cn(
                        'h-full rounded-[4px] px-1.5 font-mono text-[10.5px] outline-none focus-visible:ring-2 focus-visible:ring-ring/70',
                        layout === l ? 'bg-surface-3 text-foreground' : 'text-muted-foreground hover:text-foreground',
                      )}
                    >
                      {l === '4x4' ? '4×4' : '2×8'}
                    </button>
                  ))}
                </div>
                <div className="ml-auto flex items-center gap-0.5">
                  <Tip label={showKeys ? 'Hide computer-key hints' : 'Show computer-key hints'}>
                    <Button variant="ghost" size="icon-xs" aria-label="Computer-key hints" aria-pressed={showKeys} onClick={toggleKeys}>
                      <Keyboard />
                    </Button>
                  </Tip>
                  {onOpenControllerSetup && (
                    <Tip label="Controller setup: map pads and knobs">
                      <Button variant="ghost" size="xs" onClick={onOpenControllerSetup}>
                        <Settings2 />
                        {profile.name.length > 18 ? 'Controller' : profile.name}
                      </Button>
                    </Tip>
                  )}
                </div>
              </header>

              <PadGrid
                className={cn('min-h-0', layout === '2x8' ? 'my-auto h-[220px]' : 'flex-1')}
                pads={beat.kit.pads}
                selectedPad={selected}
                layout={layout}
                hardwareNotes={profile.padMap.notes}
                showKeyHints
                canPaste={beat.hasClipboard}
                onHit={onPadHit}
                onRelease={onPadRelease}
                onSelect={actions.selectPad}
                onDropSample={actions.assignFile}
                onClear={actions.clearPad}
                onRename={actions.renamePad}
                onColor={(pad, color) => actions.updatePad(pad, { color })}
                onCopy={actions.copyPad}
                onPaste={actions.pastePad}
                onChop={onChop}
                onChokeGroup={(pad, chokeGroup) => actions.updatePad(pad, { chokeGroup })}
              />

              {/* Count-in: one big number over the pads, never moves anything. */}
              {counting && (
                <div aria-hidden className="pointer-events-none absolute inset-0 flex items-center justify-center bg-surface-0/45">
                  <span key={Math.ceil(beat.countInRemaining)} className="animate-note-pop text-[132px] leading-none font-bold text-signal-rec tabular-nums drop-shadow-[0_4px_24px_oklch(0_0_0/0.6)]">
                    {Math.ceil(beat.countInRemaining)}
                  </span>
                </div>
              )}
            </section>

            <div className="min-w-0 flex-1 p-3">
              {selectedSettings && (
                <PadEditor
                  className="h-full"
                  pad={selected}
                  settings={selectedSettings}
                  onChange={(patch) => actions.editPad(selected, patch)}
                  onChangeEnd={actions.endPadEdit}
                  onPreview={(pad) => (onPadPreview ?? onPadHit)(pad, 0.85)}
                  onChop={actions.chopToPads}
                  chopRequest={chopRequest}
                />
              )}
            </div>
          </div>

          {/* Step sequencer fills the rest */}
          <StepSequencer
            className={cn('min-h-0 flex-1', beat.isLoopRecording && !counting && 'shadow-[inset_0_2px_0_var(--signal-rec)]')}
            pads={beat.kit.pads}
            selectedPad={selected}
            region={beat.patternRegion}
            patternVersion={beat.patternVersion}
            patternStartBeat={beat.patternStartBeat}
            bars={beat.loop.bars}
            swing={beat.loop.swing}
            grid={grid}
            onGridChange={changeGrid}
            onToggleStep={actions.toggleStep}
            onSetVelocity={actions.setStepVelocity}
            onSelectPad={actions.selectPad}
            onClear={beat.patternRegion?.notes.length ? () => actions.clearPattern() : undefined}
            subscribePosition={actions.subscribePosition}
            playing={playing}
          />
        </div>
      </div>
    </div>
  );
}
