import { Volume2, VolumeX, Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Slider } from '@/components/ui/slider';
import { cn } from '@/lib/utils';
import type { Track, TrackInstrument } from '@/types/arrangement';
import { SYNTH_PRESETS, MIN_VOLUME, MAX_VOLUME } from '@/utils/constants';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';

interface TrackHeaderProps {
  track: Track;
  isArmed: boolean;
  isRecording: boolean;
  onMuteToggle: () => void;
  onSoloToggle: () => void;
  onVolumeChange: (db: number) => void;
  onInstrumentChange: (instrument: TrackInstrument) => void;
  onDelete: () => void;
  onArmToggle: () => void;
}

export function TrackHeader({
  track,
  isArmed,
  isRecording,
  onMuteToggle,
  onSoloToggle,
  onVolumeChange,
  onInstrumentChange,
  onDelete,
  onArmToggle,
}: TrackHeaderProps) {
  return (
    <div
      className="flex h-16 shrink-0 flex-col justify-center border-b border-border px-2"
      style={{
        borderLeft: `3px solid ${track.color}`,
        boxShadow: `inset 4px 0 12px -4px ${track.color}33`,
      }}
    >
      <div className="flex items-center gap-1">
        <button
          className={cn(
            'h-4 w-4 rounded-full border-2 transition-colors',
            isRecording
              ? 'border-red-500 bg-red-500 animate-pulse'
              : isArmed
                ? 'border-red-500 bg-red-500'
                : 'border-muted-foreground/50 bg-transparent',
          )}
          onClick={onArmToggle}
          title={isArmed ? 'Disarm track' : 'Arm track for recording'}
        />
        <Select
          value={`${track.instrument.type}-${track.instrument.presetIndex}`}
          onValueChange={(val) => {
            const [type, idx] = val.split('-');
            onInstrumentChange({ type: type as 'synth' | 'drums', presetIndex: Number(idx) });
          }}
        >
          <SelectTrigger className="h-5 flex-1 border-0 bg-transparent px-1 text-[11px] font-semibold">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {SYNTH_PRESETS.map((p, i) => (
              <SelectItem key={`synth-${i}`} value={`synth-${i}`}>{p.name}</SelectItem>
            ))}
            <SelectItem value="drums-0">Drums</SelectItem>
          </SelectContent>
        </Select>
        <Button variant="ghost" size="sm" className="h-5 w-5 p-0" onClick={onDelete}>
          <Trash2 className="h-3 w-3 text-muted-foreground" />
        </Button>
      </div>
      <div className="flex items-center gap-1 mt-0.5">
        <Button
          variant="ghost"
          size="sm"
          className={cn('h-5 w-5 p-0', track.muted && 'text-red-500')}
          onClick={onMuteToggle}
          title="Mute"
        >
          {track.muted ? <VolumeX className="h-3 w-3" /> : <Volume2 className="h-3 w-3" />}
        </Button>
        <Button
          variant="ghost"
          size="sm"
          className={cn('h-5 w-5 p-0 text-[10px] font-bold', track.solo && 'text-yellow-500')}
          onClick={onSoloToggle}
          title="Solo"
        >
          S
        </Button>
        <Slider
          min={MIN_VOLUME}
          max={MAX_VOLUME}
          step={1}
          value={[track.volume]}
          onValueChange={([v]) => onVolumeChange(v)}
          className="w-16"
        />
      </div>
    </div>
  );
}
