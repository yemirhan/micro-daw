import { useEffect, useRef, useState } from 'react';
import { Circle, Play, Square, Repeat } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Tip } from '@/components/ui/tip';
import { MarkerJumpDropdown } from '@/components/arrangement/MarkerJumpDropdown';
import type { TransportState } from '@/types/recorder';
import type { Marker } from '@/types/arrangement';
import { MIN_BPM, MAX_BPM } from '@/utils/constants';
import { cn } from '@/lib/utils';

interface TransportBarProps {
  state: TransportState;
  bpm: number;
  metronomeOn: boolean;
  positionBeats: number;
  recordDisabled?: boolean;
  loopEnabled?: boolean;
  isPunchRecording?: boolean;
  markers?: Marker[];
  onLoopToggle?: () => void;
  onSeekToMarker?: (id: string) => void;
  onRecord: () => void;
  onStopRecording: () => void;
  onPlay: () => void;
  onStop: () => void;
  onBpmChange: (bpm: number) => void;
  onMetronomeToggle: () => void;
}

/** Bars.beats.sixteenths, 4/4. */
function formatBBS(beats: number): [string, string, string] {
  const b = Math.max(0, beats);
  const bar = Math.floor(b / 4) + 1;
  const beat = Math.floor(b % 4) + 1;
  const six = Math.floor((b % 1) * 4) + 1;
  return [String(bar).padStart(3, ' '), String(beat), String(six)];
}

function formatTime(beats: number, bpm: number): string {
  const sec = Math.max(0, (beats * 60) / bpm);
  const m = Math.floor(sec / 60);
  const s = sec - m * 60;
  return `${m}:${s.toFixed(1).padStart(4, '0')}`;
}

function MetronomeIcon({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round" className={className} aria-hidden="true">
      <path d="M9 3h6l4 18H5L9 3z" />
      <path d="M12 16l5-9" />
      <path d="M7 16h10" />
    </svg>
  );
}

/** Tempo readout: type a value, drag vertically, wheel, or use arrow keys. */
function BpmField({ bpm, onChange }: { bpm: number; onChange: (bpm: number) => void }) {
  const [draft, setDraft] = useState<string | null>(null);
  const drag = useRef<{ y: number; start: number; moved: boolean } | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const clamp = (v: number) => Math.round(Math.min(MAX_BPM, Math.max(MIN_BPM, v)));

  const commit = () => {
    if (draft !== null) {
      const v = Number.parseFloat(draft);
      if (Number.isFinite(v)) onChange(clamp(v));
    }
    setDraft(null);
  };

  // Wheel needs a non-passive listener to prevent page scroll.
  useEffect(() => {
    const el = inputRef.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      onChange(clamp(bpm + (e.deltaY < 0 ? 1 : -1)));
    };
    el.addEventListener('wheel', onWheel, { passive: false });
    return () => el.removeEventListener('wheel', onWheel);
  });

  return (
    <Tip label="Tempo: drag, scroll or type">
      <label className="daw-lcd flex h-7 cursor-ns-resize items-center gap-1.5 pr-2 pl-2">
        <input
          ref={inputRef}
          aria-label="Tempo (BPM)"
          inputMode="numeric"
          value={draft ?? String(bpm)}
          onChange={(e) => setDraft(e.target.value.replace(/[^0-9.]/g, '').slice(0, 5))}
          onFocus={(e) => e.currentTarget.select()}
          onBlur={commit}
          onKeyDown={(e) => {
            if (e.key === 'Enter') {
              commit();
              e.currentTarget.blur();
            } else if (e.key === 'Escape') {
              setDraft(null);
              e.currentTarget.blur();
            } else if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
              e.preventDefault();
              setDraft(null);
              onChange(clamp(bpm + (e.key === 'ArrowUp' ? 1 : -1) * (e.shiftKey ? 10 : 1)));
            }
          }}
          onPointerDown={(e) => {
            if (document.activeElement === e.currentTarget) return; // editing: let the caret work
            e.preventDefault();
            drag.current = { y: e.clientY, start: bpm, moved: false };
            e.currentTarget.setPointerCapture(e.pointerId);
          }}
          onPointerMove={(e) => {
            const d = drag.current;
            if (!d) return;
            const dy = d.y - e.clientY;
            if (Math.abs(dy) > 2) d.moved = true;
            if (d.moved) onChange(clamp(d.start + dy / (e.shiftKey ? 8 : 2)));
          }}
          onPointerUp={(e) => {
            const d = drag.current;
            drag.current = null;
            if (d && !d.moved) e.currentTarget.focus();
          }}
          onPointerCancel={() => {
            drag.current = null;
          }}
          className="w-[30px] cursor-ns-resize bg-transparent text-right text-[13px] font-semibold text-foreground outline-none focus:cursor-text focus:text-primary"
        />
        <span className="text-[9.5px] font-bold tracking-[0.08em] text-muted-foreground">BPM</span>
      </label>
    </Tip>
  );
}

export function TransportBar({
  state,
  bpm,
  metronomeOn,
  positionBeats,
  recordDisabled,
  loopEnabled,
  isPunchRecording,
  markers,
  onLoopToggle,
  onSeekToMarker,
  onRecord,
  onStopRecording,
  onPlay,
  onStop,
  onBpmChange,
  onMetronomeToggle,
}: TransportBarProps) {
  const isRecording = state === 'recording';
  const isPlaying = state === 'playing';
  const [bar, beat, six] = formatBBS(positionBeats);

  return (
    <div data-tour="transport" className="flex items-center gap-2">
      {/* Transport cluster */}
      <div className="flex h-8 items-center gap-0.5 rounded-md border border-border bg-surface-0 p-0.5">
        <Tip label={isPlaying ? 'Playing' : 'Play'} shortcut="Space">
          <Button
            variant="ghost"
            size="icon-sm"
            className={cn(
              'h-[26px]',
              isPlaying && 'bg-signal-play/15 text-signal-play hover:bg-signal-play/20 hover:text-signal-play',
            )}
            onClick={onPlay}
            disabled={isRecording}
            aria-label="Play"
            aria-pressed={isPlaying}
          >
            <Play className={cn(isPlaying && 'fill-current')} />
          </Button>
        </Tip>
        <Tip label="Stop" shortcut="Space">
          <Button
            variant="ghost"
            size="icon-sm"
            className="h-[26px]"
            onClick={isRecording ? onStopRecording : onStop}
            disabled={state === 'stopped'}
            aria-label="Stop"
          >
            <Square className="size-3.5 fill-current" />
          </Button>
        </Tip>
        <Tip
          label={isRecording ? 'Stop recording' : recordDisabled ? 'Arm a track to record' : loopEnabled ? 'Punch record (loop)' : 'Record'}
          shortcut="R"
        >
          {/* span keeps the tooltip working while the button is disabled */}
          <span className="inline-flex">
            <Button
              variant="ghost"
              size="icon-sm"
              className={cn(
                'h-[26px]',
                isRecording
                  ? 'animate-rec-pulse bg-signal-rec text-white hover:bg-signal-rec hover:text-white'
                  : 'text-signal-rec/80 hover:text-signal-rec',
              )}
              onClick={isRecording ? onStopRecording : onRecord}
              disabled={!isRecording && recordDisabled}
              aria-label={isRecording ? 'Stop recording' : 'Record'}
              aria-pressed={isRecording}
            >
              <Circle className="size-3.5 fill-current" />
            </Button>
          </span>
        </Tip>
        <div className="mx-0.5 h-4 w-px bg-border" />
        {onLoopToggle && (
          <Tip label={loopEnabled ? 'Loop on' : 'Loop off'} shortcut="L">
            <Button
              variant="ghost"
              size="icon-sm"
              className={cn('h-[26px]', loopEnabled && 'bg-primary/15 text-primary hover:bg-primary/20 hover:text-primary')}
              onClick={onLoopToggle}
              aria-label="Loop"
              aria-pressed={!!loopEnabled}
            >
              <Repeat />
            </Button>
          </Tip>
        )}
        <Tip label={metronomeOn ? 'Metronome on' : 'Metronome off'} shortcut="M">
          <Button
            variant="ghost"
            size="icon-sm"
            className={cn('h-[26px]', metronomeOn && 'bg-primary/15 text-primary hover:bg-primary/20 hover:text-primary')}
            onClick={onMetronomeToggle}
            aria-label="Metronome"
            aria-pressed={metronomeOn}
          >
            <MetronomeIcon className="size-4" />
          </Button>
        </Tip>
      </div>

      {/* Position LCD */}
      <div
        className={cn(
          'daw-lcd flex h-8 items-center gap-3 pr-2.5 pl-2',
          isRecording && 'border-signal-rec/50',
        )}
        aria-label="Song position"
      >
        <span className="flex items-baseline text-[15px] font-semibold leading-none tracking-tight text-foreground">
          <span className="whitespace-pre">{bar}</span>
          <span className="text-muted-foreground/60">.</span>
          <span>{beat}</span>
          <span className="text-muted-foreground/60">.</span>
          <span className="text-foreground/70">{six}</span>
        </span>
        <span className="hidden text-[11px] leading-none text-muted-foreground min-[1100px]:inline">{formatTime(positionBeats, bpm)}</span>
        {isRecording && (
          <span
            className={cn(
              'rounded-[3px] px-1 py-px text-[9.5px] font-bold tracking-[0.08em] text-white',
              isPunchRecording ? 'bg-signal-warn text-black' : 'bg-signal-rec',
            )}
          >
            {isPunchRecording ? 'PUNCH' : 'REC'}
          </span>
        )}
      </div>

      <BpmField bpm={bpm} onChange={onBpmChange} />

      {markers && markers.length > 0 && onSeekToMarker && (
        <MarkerJumpDropdown markers={markers} onSeekToMarker={onSeekToMarker} />
      )}
    </div>
  );
}
