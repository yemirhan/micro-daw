import { useEffect, useRef, useState } from 'react';
import { ArrowDownToLine, Pause, Play, Terminal, Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { cn } from '@/lib/utils';
import { useMidiMonitor, type FilterType } from '@/hooks/useMidiMonitor';
import type { MidiDeviceInfo } from '@/types/midi';
import type { MidiMonitorEvent } from '@/types/midi';

interface DevViewProps {
  activeDeviceId: string | null;
  devices: MidiDeviceInfo[];
}

/** Message type -> text color + dot color (semantic signal tokens). */
const TYPE_STYLE: Record<string, { text: string; dot: string }> = {
  noteon: { text: 'text-signal-play', dot: 'bg-signal-play' },
  noteoff: { text: 'text-signal-rec', dot: 'bg-signal-rec' },
  controlchange: { text: 'text-signal-select', dot: 'bg-signal-select' },
  pitchbend: { text: 'text-signal-warn', dot: 'bg-signal-warn' },
  aftertouch: { text: 'text-[oklch(0.74_0.13_300)]', dot: 'bg-[oklch(0.74_0.13_300)]' },
  programchange: { text: 'text-primary', dot: 'bg-primary' },
};

const FILTER_OPTIONS: { value: FilterType; label: string }[] = [
  { value: 'all', label: 'All messages' },
  { value: 'noteon', label: 'Note On' },
  { value: 'noteoff', label: 'Note Off' },
  { value: 'controlchange', label: 'Control Change' },
  { value: 'pitchbend', label: 'Pitch Bend' },
  { value: 'aftertouch', label: 'Aftertouch' },
  { value: 'programchange', label: 'Program Change' },
];

const GRID = 'grid grid-cols-[6rem_8.5rem_3rem_minmax(0,1fr)_9rem] gap-3';

function formatHex(bytes: number[]): string {
  return bytes.map((b) => b.toString(16).toUpperCase().padStart(2, '0')).join(' ');
}

function formatTimestamp(ts: number): string {
  const s = (ts / 1000) % 1000;
  return s.toFixed(3).padStart(8, ' ');
}

function EventRow({ event }: { event: MidiMonitorEvent }) {
  const style = TYPE_STYLE[event.type];

  return (
    <div className={cn(GRID, 'h-6 items-center px-4 font-mono text-xs hover:bg-surface-2')} role="row">
      <span className="tabular-nums text-muted-foreground" role="cell">{formatTimestamp(event.timestamp)}</span>
      <span className={cn('flex items-center gap-2 font-medium', style?.text ?? 'text-muted-foreground')} role="cell">
        <span className={cn('size-1.5 rounded-full', style?.dot ?? 'bg-muted-foreground')} aria-hidden />
        {event.label}
      </span>
      <span className="text-center tabular-nums text-muted-foreground" role="cell">{event.channel ?? '-'}</span>
      <span className="truncate text-foreground/90" role="cell">{event.detail}</span>
      <span className="tabular-nums text-muted-foreground/70" role="cell">{formatHex(event.rawData)}</span>
    </div>
  );
}

export function DevView({ activeDeviceId, devices }: DevViewProps) {
  const { events, paused, togglePause, clear, filterType, setFilterType } = useMidiMonitor(true);
  const scrollRef = useRef<HTMLDivElement>(null);
  const autoScrollRef = useRef(true);
  const [following, setFollowing] = useState(true);

  const activeDevice = devices.find((d) => d.id === activeDeviceId);

  // Auto-scroll to bottom unless user scrolled up
  useEffect(() => {
    const el = scrollRef.current;
    if (!el || !autoScrollRef.current) return;
    el.scrollTop = el.scrollHeight;
  }, [events]);

  const handleScroll = () => {
    const el = scrollRef.current;
    if (!el) return;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
    autoScrollRef.current = atBottom;
    if (atBottom !== following) setFollowing(atBottom);
  };

  const jumpToLatest = () => {
    const el = scrollRef.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
    autoScrollRef.current = true;
    setFollowing(true);
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col bg-background">
      {/* Toolbar */}
      <header className="flex h-12 shrink-0 items-center gap-3 border-b border-border bg-surface-1 px-4">
        <Terminal className="size-4 text-muted-foreground" aria-hidden />
        <h2 className="text-sm font-semibold">MIDI Monitor</h2>
        <span
          className={cn(
            'inline-flex h-6 items-center gap-1.5 rounded-md border px-2 text-xs',
            activeDevice ? 'border-border bg-surface-2 text-foreground' : 'border-dashed border-border text-muted-foreground',
          )}
        >
          <span
            className={cn('size-1.5 rounded-full', activeDevice ? 'bg-signal-play' : 'bg-muted-foreground/50')}
            aria-hidden
          />
          {activeDevice ? activeDevice.name : 'No device'}
        </span>

        <div className="ml-auto flex items-center gap-2">
          <Select value={filterType} onValueChange={(v) => setFilterType(v as FilterType)}>
            <SelectTrigger className="h-7 w-[150px] text-xs" aria-label="Message type filter">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {FILTER_OPTIONS.map((opt) => (
                <SelectItem key={opt.value} value={opt.value} className="text-xs">
                  {opt.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>

          <Button
            variant={paused ? 'secondary' : 'ghost'}
            size="sm"
            className={cn('h-7 gap-1.5 px-2.5 text-xs', paused && 'text-signal-warn')}
            onClick={togglePause}
            title={paused ? 'Resume capture' : 'Pause capture'}
            aria-pressed={paused}
          >
            {paused ? <Play className="size-3.5" /> : <Pause className="size-3.5" />}
            {paused ? 'Resume' : 'Pause'}
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            className="size-7 text-muted-foreground hover:text-foreground"
            onClick={clear}
            title="Clear log"
            aria-label="Clear log"
          >
            <Trash2 className="size-3.5" />
          </Button>
        </div>
      </header>

      {/* Column headers */}
      <div
        className={cn(GRID, 'h-7 shrink-0 items-center border-b border-border bg-surface-1 px-4 text-[10px] font-bold uppercase tracking-[0.08em] text-muted-foreground')}
        role="row"
      >
        <span role="columnheader">Time (s)</span>
        <span role="columnheader">Type</span>
        <span className="text-center" role="columnheader">Ch</span>
        <span role="columnheader">Detail</span>
        <span role="columnheader">Raw hex</span>
      </div>

      {/* Event log */}
      <div ref={scrollRef} onScroll={handleScroll} className="relative min-h-0 flex-1 overflow-y-auto py-1" role="table" aria-label="MIDI messages">
        {events.length === 0 ? (
          <div className="flex h-full flex-col items-center justify-center gap-2 text-center">
            <div className="flex size-10 items-center justify-center rounded-lg border border-border bg-surface-1">
              <Terminal className="size-5 text-muted-foreground" aria-hidden />
            </div>
            <p className="text-sm font-medium text-foreground">
              {activeDevice ? 'Waiting for MIDI input…' : 'No MIDI device connected'}
            </p>
            <p className="max-w-xs text-xs text-muted-foreground">
              {activeDevice
                ? 'Play a key, move a knob or press a pad. Clock and active-sensing messages are hidden.'
                : 'Connect a keyboard or controller and pick it in the top bar to see its raw messages here.'}
            </p>
          </div>
        ) : (
          events.map((event) => <EventRow key={event.id} event={event} />)
        )}
      </div>

      {/* Footer status */}
      <footer className="flex h-7 shrink-0 items-center gap-3 border-t border-border bg-surface-1 px-4 text-[11px] text-muted-foreground">
        <span>
          <span className="font-mono tabular-nums text-foreground">{events.length}</span> events
          {filterType !== 'all' && ' (filtered)'}
        </span>
        {paused && (
          <span className="inline-flex items-center gap-1 font-semibold text-signal-warn">
            <Pause className="size-3" aria-hidden />
            Capture paused
          </span>
        )}
        {!following && events.length > 0 && (
          <button
            type="button"
            onClick={jumpToLatest}
            className="ml-auto inline-flex items-center gap-1 rounded px-1.5 py-0.5 font-medium text-primary outline-none hover:bg-surface-2 focus-visible:ring-2 focus-visible:ring-ring/60"
          >
            <ArrowDownToLine className="size-3" aria-hidden />
            Jump to latest
          </button>
        )}
        {following && events.length > 0 && <span className="ml-auto">Following live</span>}
      </footer>
    </div>
  );
}
