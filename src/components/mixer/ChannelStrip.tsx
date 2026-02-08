import { Volume2, VolumeX } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Slider } from '@/components/ui/slider';
import { cn } from '@/lib/utils';
import { LevelMeter } from './LevelMeter';
import type { Track } from '@/types/arrangement';
import { MIN_VOLUME, MAX_VOLUME } from '@/utils/constants';

interface ChannelStripProps {
  track: Track;
  level: number;
  onVolumeChange: (db: number) => void;
  onPanChange: (pan: number) => void;
  onMuteToggle: () => void;
  onSoloToggle: () => void;
}

export function ChannelStrip({
  track,
  level,
  onVolumeChange,
  onPanChange,
  onMuteToggle,
  onSoloToggle,
}: ChannelStripProps) {
  const panLabel = (track.pan ?? 0) > 0
    ? `R${Math.round((track.pan ?? 0) * 100)}`
    : (track.pan ?? 0) < 0
      ? `L${Math.round(Math.abs(track.pan ?? 0) * 100)}`
      : 'C';

  return (
    <div className="flex flex-col items-center gap-1.5 px-2 py-2 min-w-[64px]">
      {/* Track name */}
      <span
        className="text-[10px] font-semibold truncate w-full text-center"
        style={{ color: track.color }}
      >
        {track.name}
      </span>

      {/* Pan knob */}
      <div className="w-full">
        <Slider
          min={-1}
          max={1}
          step={0.01}
          value={[track.pan ?? 0]}
          onValueChange={([v]) => onPanChange(v)}
          className="w-full"
        />
        <span className="block text-center text-[9px] text-muted-foreground">{panLabel}</span>
      </div>

      {/* Level meter + fader */}
      <div className="flex items-end gap-1">
        <LevelMeter level={level} height={80} />
        <div className="h-20 flex items-center">
          <Slider
            orientation="vertical"
            min={MIN_VOLUME}
            max={MAX_VOLUME}
            step={1}
            value={[track.volume]}
            onValueChange={([v]) => onVolumeChange(v)}
            className="h-20"
          />
        </div>
      </div>

      {/* Volume readout */}
      <span className="font-mono text-[9px] text-muted-foreground">
        {track.volume}dB
      </span>

      {/* Mute/Solo */}
      <div className="flex items-center gap-0.5">
        <Button
          variant="ghost"
          size="sm"
          className={cn('h-5 w-5 p-0', track.muted && 'text-red-500')}
          onClick={onMuteToggle}
        >
          {track.muted ? <VolumeX className="h-3 w-3" /> : <Volume2 className="h-3 w-3" />}
        </Button>
        <Button
          variant="ghost"
          size="sm"
          className={cn('h-5 w-5 p-0 text-[9px] font-bold', track.solo && 'text-yellow-500')}
          onClick={onSoloToggle}
        >
          S
        </Button>
      </div>
    </div>
  );
}
