import { useEffect, useRef } from 'react';
import { Pause, Play, Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { useMidiMonitor, type FilterType } from '@/hooks/useMidiMonitor';
import type { MidiDeviceInfo } from '@/types/midi';
import type { MidiMonitorEvent } from '@/types/midi';

interface DevViewProps {
  activeDeviceId: string | null;
  devices: MidiDeviceInfo[];
}

const TYPE_COLORS: Record<string, string> = {
  noteon: 'text-green-400',
  noteoff: 'text-red-400',
  controlchange: 'text-blue-400',
  pitchbend: 'text-yellow-400',
  aftertouch: 'text-purple-400',
  programchange: 'text-orange-400',
};

const FILTER_OPTIONS: { value: FilterType; label: string }[] = [
  { value: 'all', label: 'All Messages' },
  { value: 'noteon', label: 'Note On' },
  { value: 'noteoff', label: 'Note Off' },
  { value: 'controlchange', label: 'CC' },
  { value: 'pitchbend', label: 'Pitch Bend' },
  { value: 'aftertouch', label: 'Aftertouch' },
  { value: 'programchange', label: 'Program Change' },
];

function formatHex(bytes: number[]): string {
  return bytes.map((b) => b.toString(16).toUpperCase().padStart(2, '0')).join(' ');
}

function formatTimestamp(ts: number): string {
  const s = (ts / 1000) % 1000;
  return s.toFixed(3).padStart(8, ' ');
}

function EventRow({ event }: { event: MidiMonitorEvent }) {
  const colorClass = TYPE_COLORS[event.type] ?? 'text-muted-foreground';

  return (
    <div className="grid grid-cols-[5rem_6rem_2.5rem_1fr_8rem] gap-2 px-3 py-0.5 text-xs font-mono hover:bg-muted/30">
      <span className="text-muted-foreground tabular-nums">{formatTimestamp(event.timestamp)}</span>
      <span className={colorClass}>{event.label}</span>
      <span className="text-muted-foreground text-center">{event.channel ?? '-'}</span>
      <span className="truncate">{event.detail}</span>
      <span className="text-muted-foreground/60">{formatHex(event.rawData)}</span>
    </div>
  );
}

export function DevView({ activeDeviceId, devices }: DevViewProps) {
  const { events, paused, togglePause, clear, filterType, setFilterType } = useMidiMonitor(true);
  const scrollRef = useRef<HTMLDivElement>(null);
  const autoScrollRef = useRef(true);

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
  };

  return (
    <div className="flex flex-1 flex-col min-h-0 bg-background">
      {/* Header */}
      <div className="flex items-center gap-3 border-b px-4 py-2">
        <h2 className="text-sm font-semibold">MIDI Monitor</h2>
        {activeDevice ? (
          <Badge variant="secondary" className="text-xs font-normal">
            {activeDevice.name}
          </Badge>
        ) : (
          <Badge variant="outline" className="text-xs font-normal text-muted-foreground">
            No device
          </Badge>
        )}

        <div className="ml-auto flex items-center gap-2">
          <Select value={filterType} onValueChange={(v) => setFilterType(v as FilterType)}>
            <SelectTrigger className="h-7 w-[140px] text-xs">
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

          <Button variant="ghost" size="icon" className="h-7 w-7" onClick={togglePause} title={paused ? 'Resume' : 'Pause'}>
            {paused ? <Play className="h-3.5 w-3.5" /> : <Pause className="h-3.5 w-3.5" />}
          </Button>
          <Button variant="ghost" size="icon" className="h-7 w-7" onClick={clear} title="Clear">
            <Trash2 className="h-3.5 w-3.5" />
          </Button>
        </div>
      </div>

      {/* Column headers */}
      <div className="grid grid-cols-[5rem_6rem_2.5rem_1fr_8rem] gap-2 border-b px-3 py-1 text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
        <span>Time</span>
        <span>Type</span>
        <span className="text-center">Ch</span>
        <span>Detail</span>
        <span>Raw Hex</span>
      </div>

      {/* Event log */}
      <div
        ref={scrollRef}
        onScroll={handleScroll}
        className="flex-1 overflow-y-auto min-h-0"
      >
        {events.length === 0 ? (
          <div className="flex h-full items-center justify-center text-muted-foreground text-sm">
            {activeDevice
              ? 'Waiting for MIDI input...'
              : 'Connect a MIDI device to see messages'}
          </div>
        ) : (
          events.map((event) => <EventRow key={event.id} event={event} />)
        )}
      </div>

      {/* Footer status */}
      <div className="flex items-center justify-between border-t px-3 py-1 text-[10px] text-muted-foreground">
        <span>{events.length} events</span>
        {paused && <span className="text-yellow-500">Paused</span>}
      </div>
    </div>
  );
}
