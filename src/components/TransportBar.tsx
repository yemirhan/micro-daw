import { Circle, Play, Square, Repeat } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Slider } from '@/components/ui/slider';
import { Switch } from '@/components/ui/switch';
import { Badge } from '@/components/ui/badge';
import { Label } from '@/components/ui/label';
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

function formatPosition(beats: number): string {
  const bar = Math.floor(beats / 4) + 1;
  const beat = Math.floor(beats % 4) + 1;
  return `${bar}:${beat}`;
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

  return (
    <div data-tour="transport" className="flex items-center gap-4 border-b border-border bg-card/60 backdrop-blur-sm px-4 py-2">
      {/* Transport controls */}
      <div className="flex items-center gap-1">
        <Button
          variant={isRecording ? 'destructive' : 'ghost'}
          size="sm"
          className={cn('h-8 w-8 p-0', isRecording && 'animate-rec-pulse')}
          onClick={isRecording ? onStopRecording : onRecord}
          disabled={!isRecording && recordDisabled}
        >
          <Circle className={cn('h-4 w-4', isRecording ? 'fill-current' : '')} />
        </Button>
        <Button
          variant={isPlaying ? 'default' : 'ghost'}
          size="sm"
          className="h-8 w-8 p-0"
          style={isPlaying ? {
            backgroundColor: 'oklch(0.65 0.20 155)',
            boxShadow: '0 0 12px 2px oklch(0.65 0.20 155 / 0.35)',
          } : undefined}
          onClick={onPlay}
          disabled={isRecording}
        >
          <Play className={cn('h-4 w-4', isPlaying && 'fill-current')} />
        </Button>
        <Button
          variant="ghost"
          size="sm"
          className="h-8 w-8 p-0"
          onClick={isRecording ? onStopRecording : onStop}
          disabled={state === 'stopped'}
        >
          <Square className="h-4 w-4" />
        </Button>
        {onLoopToggle && (
          <Button
            variant="ghost"
            size="sm"
            className={cn('h-8 w-8 p-0', loopEnabled && 'text-primary')}
            style={loopEnabled ? {
              backgroundColor: 'oklch(0.65 0.20 265 / 0.15)',
              boxShadow: '0 0 8px 1px oklch(0.65 0.20 265 / 0.2)',
            } : undefined}
            onClick={onLoopToggle}
            title="Loop (L)"
          >
            <Repeat className="h-4 w-4" />
          </Button>
        )}
      </div>

      {/* Position display */}
      <Badge
        variant="secondary"
        className="min-w-[52px] justify-center font-mono text-xs"
        style={{
          backgroundColor: 'oklch(0.14 0.01 270)',
          border: '1px solid oklch(0.30 0.02 270)',
          boxShadow: 'inset 0 1px 3px oklch(0 0 0 / 0.3)',
        }}
      >
        {formatPosition(positionBeats)}
      </Badge>

      {isRecording && (
        <Badge variant="destructive" className="text-[10px]">
          REC
        </Badge>
      )}

      {isPunchRecording && (
        <Badge
          className="text-[10px]"
          style={{
            backgroundColor: 'oklch(0.55 0.20 280)',
            color: 'oklch(0.95 0 0)',
          }}
        >
          PUNCH
        </Badge>
      )}

      {markers && markers.length > 0 && onSeekToMarker && (
        <MarkerJumpDropdown markers={markers} onSeekToMarker={onSeekToMarker} />
      )}

      <div className="h-4 w-px bg-border/60" />

      {/* BPM */}
      <div className="flex items-center gap-2">
        <Label className="text-[11px] font-semibold text-muted-foreground">BPM</Label>
        <Slider
          min={MIN_BPM}
          max={MAX_BPM}
          step={1}
          value={[bpm]}
          onValueChange={([v]) => onBpmChange(v)}
          className="w-20"
        />
        <span className="w-7 text-center font-mono text-xs text-primary">{bpm}</span>
      </div>

      <div className="h-4 w-px bg-border/60" />

      {/* Metronome */}
      <div className="flex items-center gap-2">
        <Label htmlFor="metronome" className="text-[11px] font-semibold text-muted-foreground">
          Metro
        </Label>
        <Switch
          id="metronome"
          checked={metronomeOn}
          onCheckedChange={onMetronomeToggle}
        />
      </div>
    </div>
  );
}
