import { useState, useEffect, useCallback, type CSSProperties, type ReactNode } from 'react';
import { Maximize2, Minimize2, Undo2, Redo2, PanelLeft, SlidersHorizontal } from 'lucide-react';
import { MidiDeviceSelector } from '@/components/MidiDeviceSelector';
import { VolumeControl } from '@/components/VolumeControl';
import { InstrumentSelector } from '@/components/InstrumentSelector';
import { Button } from '@/components/ui/button';
import { Tip } from '@/components/ui/tip';
import { midiToNoteName } from '@/utils/noteHelpers';
import { cn } from '@/lib/utils';
import type { MidiDeviceInfo } from '@/types/midi';
import type { ChordInfo } from '@/types/music';
import type { ActiveNote } from '@/hooks/useMidiNotes';
import type { AppMode, DawView } from '@/types/appMode';

interface TopBarProps {
  mode: AppMode;
  midi: {
    devices: MidiDeviceInfo[];
    activeDeviceId: string | null;
    onSelectDevice: (id: string) => void;
    activeNotes: Map<number, ActiveNote>;
    detectedChord: ChordInfo | null;
  };
  audio: {
    volume: number;
    onVolumeChange: (db: number) => void;
    presetIndex: number;
    onPresetChange: (index: number) => void;
    muted: boolean;
    onToggleMute: () => void;
    onToggleSynthEditor?: () => void;
    synthEditorOpen?: boolean;
  };
  history: {
    canUndo: boolean;
    canRedo: boolean;
    onUndo: () => void;
    onRedo: () => void;
  };
  project: {
    projectName: string;
    isDirty: boolean;
    onSave: () => void;
  };
  sidebarTrigger?: ReactNode;
  /** Centre slot (the DAW puts its transport here). Defaults to a live-input readout. */
  center?: ReactNode;
  /** Beat | Song switch (DAW mode). */
  view?: { current: DawView; onChange: (view: DawView) => void };
  /** Sample browser panel toggle (DAW mode). */
  browser?: { open: boolean; onToggle: () => void };
  /** Opens the MIDI controller setup (pads + knobs). */
  onOpenControllerSetup?: () => void;
}

const DRAG = { WebkitAppRegion: 'drag' } as CSSProperties;
const NO_DRAG = { WebkitAppRegion: 'no-drag' } as CSSProperties;
const IS_MAC = typeof navigator !== 'undefined' && /Mac/i.test(navigator.platform || navigator.userAgent);
const MOD = IS_MAC ? '⌘' : 'Ctrl+';

/** Small app mark (half sine / half square), matches the launch screen. */
export function AppMark({ className }: { className?: string }) {
  return (
    <span className={cn('inline-flex size-5 items-center justify-center rounded-[5px] bg-primary text-primary-foreground', className)}>
      <svg viewBox="0 0 48 48" className="size-4" aria-hidden="true">
        <path d="M10 25 C 13 12, 20 12, 24 25 L 24 33 L 37 33 L 37 25" fill="none" stroke="currentColor" strokeWidth={4.5} strokeLinecap="round" strokeLinejoin="round" />
      </svg>
    </span>
  );
}

/** Notes + chord currently held, for modes without a transport in the top bar. */
function LiveInputReadout({ activeNotes, chord }: { activeNotes: Map<number, ActiveNote>; chord: ChordInfo | null }) {
  const notes = Array.from(activeNotes.keys()).sort((a, b) => a - b);
  return (
    <div className="daw-lcd flex h-7 min-w-[260px] items-center gap-2 px-2 text-[11px]" aria-live="polite">
      <span
        className={cn(
          'min-w-[44px] rounded-[3px] px-1.5 py-px text-center font-semibold',
          chord ? 'bg-primary/15 text-primary' : 'text-muted-foreground',
        )}
      >
        {chord ? chord.display : '—'}
      </span>
      <span className="h-3.5 w-px bg-border" />
      <span className="flex min-w-0 flex-1 items-center gap-1 overflow-hidden">
        {notes.length > 0 ? (
          notes.slice(0, 8).map((n) => (
            <span key={n} className="animate-note-pop text-foreground">
              {midiToNoteName(n)}
            </span>
          ))
        ) : (
          <span className="text-muted-foreground">Play something…</span>
        )}
      </span>
    </div>
  );
}

const VIEWS: { id: DawView; label: string }[] = [
  { id: 'beat', label: 'Beat' },
  { id: 'song', label: 'Song' },
];

/** Beat | Song segmented switch. */
function ViewSwitch({ view, onChange }: { view: DawView; onChange: (view: DawView) => void }) {
  return (
    <Tip label="Switch Beat / Song view" shortcut="Tab">
      <div role="radiogroup" aria-label="View" className="ml-1 flex h-7 items-center rounded-md border border-border bg-surface-0 p-0.5">
        {VIEWS.map((v) => (
          <button
            key={v.id}
            type="button"
            role="radio"
            aria-checked={view === v.id}
            onClick={() => onChange(v.id)}
            className={cn(
              'h-full rounded-[4px] px-2.5 text-[12px] font-semibold outline-none transition-colors duration-150 focus-visible:ring-2 focus-visible:ring-ring/70',
              view === v.id ? 'bg-surface-3 text-foreground' : 'text-muted-foreground hover:text-foreground',
            )}
          >
            {v.label}
          </button>
        ))}
      </div>
    </Tip>
  );
}

export function TopBar({
  mode,
  midi,
  audio,
  history,
  project,
  sidebarTrigger,
  center,
  view,
  browser,
  onOpenControllerSetup,
}: TopBarProps) {
  const [isFullscreen, setIsFullscreen] = useState(false);

  useEffect(() => {
    if (!window.electronAPI) return;
    window.electronAPI.isFullscreen().then(setIsFullscreen);
    return window.electronAPI.onFullscreenChanged(setIsFullscreen);
  }, []);

  const toggleFullscreen = useCallback(() => {
    window.electronAPI?.toggleFullscreen();
  }, []);

  // Room for the macOS traffic lights (hidden in fullscreen / outside Electron).
  const insetLeft = window.electronAPI && IS_MAC && !isFullscreen;

  return (
    <header
      className={cn(
        'relative z-20 grid h-11 shrink-0 grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-3 min-[1680px]:grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)] border-b border-border bg-surface-1 pr-2 shadow-[inset_0_-1px_0_oklch(0_0_0/0.25)]',
        insetLeft ? 'pl-[76px]' : 'pl-2',
      )}
      style={DRAG}
    >
      {/* Left: navigation + project */}
      <div className="flex min-w-0 items-center gap-1" style={NO_DRAG}>
        {sidebarTrigger}
        <div className="mx-1 h-4 w-px bg-border" />
        <Tip label={project.isDirty ? 'Unsaved changes. Save' : 'Save project'} shortcut={`${MOD}S`}>
          <button
            type="button"
            onClick={project.onSave}
            className="flex h-7 min-w-0 items-center gap-2 rounded-md px-1.5 text-[13px] transition-colors duration-150 hover:bg-surface-3 focus-visible:outline-2 focus-visible:outline-ring"
          >
            <AppMark className="shrink-0" />
            <span className="max-w-[120px] truncate font-semibold text-foreground min-[1280px]:max-w-[180px]">{project.projectName}</span>
            {project.isDirty && (
              <span className="size-1.5 shrink-0 rounded-full bg-signal-warn" aria-label="Unsaved changes" />
            )}
          </button>
        </Tip>
        {mode === 'daw' && (
          <div className="ml-1 flex items-center">
            <Tip label="Undo" shortcut={`${MOD}Z`}>
              <Button variant="ghost" size="icon-sm" disabled={!history.canUndo} onClick={history.onUndo} aria-label="Undo">
                <Undo2 />
              </Button>
            </Tip>
            <Tip label="Redo" shortcut={`${MOD}⇧Z`}>
              <Button variant="ghost" size="icon-sm" disabled={!history.canRedo} onClick={history.onRedo} aria-label="Redo">
                <Redo2 />
              </Button>
            </Tip>
          </div>
        )}
        {mode === 'daw' && (browser || view) && <div className="mx-1 h-4 w-px bg-border" />}
        {mode === 'daw' && browser && (
          <Tip label={browser.open ? 'Hide sample browser' : 'Show sample browser'} shortcut="B">
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label="Sample browser"
              aria-pressed={browser.open}
              onClick={browser.onToggle}
              className={cn(browser.open && 'bg-surface-3 text-foreground')}
            >
              <PanelLeft />
            </Button>
          </Tip>
        )}
        {mode === 'daw' && view && <ViewSwitch view={view.current} onChange={view.onChange} />}
      </div>

      {/* Centre */}
      <div className="flex min-w-0 items-center justify-center-safe overflow-hidden" style={NO_DRAG}>
        {center ?? <LiveInputReadout activeNotes={midi.activeNotes} chord={midi.detectedChord} />}
      </div>

      {/* Right: I/O */}
      <div className="flex min-w-0 items-center justify-end gap-2" style={NO_DRAG}>
        <div className="hidden min-[1060px]:block">
          <MidiDeviceSelector devices={midi.devices} activeDeviceId={midi.activeDeviceId} onSelect={midi.onSelectDevice} />
        </div>
        {onOpenControllerSetup && (
          <Tip label="Controller setup: map pads and knobs">
            <Button variant="ghost" size="icon-sm" aria-label="Controller setup" onClick={onOpenControllerSetup}>
              <SlidersHorizontal />
            </Button>
          </Tip>
        )}
        <InstrumentSelector
          presetIndex={audio.presetIndex}
          onChange={audio.onPresetChange}
          onToggleEditor={audio.onToggleSynthEditor}
          editorOpen={audio.synthEditorOpen}
        />
        <div className="hidden h-4 w-px bg-border min-[1180px]:block" />
        <div className="hidden min-[1180px]:block">
          <VolumeControl volume={audio.volume} onChange={audio.onVolumeChange} muted={audio.muted} onToggleMute={audio.onToggleMute} />
        </div>
        <Tip label={isFullscreen ? 'Exit full screen' : 'Full screen'}>
          <Button variant="ghost" size="icon-sm" onClick={toggleFullscreen} aria-label={isFullscreen ? 'Exit fullscreen' : 'Fullscreen'}>
            {isFullscreen ? <Minimize2 /> : <Maximize2 />}
          </Button>
        </Tip>
      </div>
    </header>
  );
}
