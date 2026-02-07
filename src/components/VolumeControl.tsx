import { Volume2, VolumeX } from 'lucide-react';
import { Slider } from '@/components/ui/slider';
import { MIN_VOLUME, MAX_VOLUME } from '@/utils/constants';

interface VolumeControlProps {
  volume: number;
  onChange: (db: number) => void;
  muted?: boolean;
  onToggleMute?: () => void;
}

export function VolumeControl({ volume, onChange, muted, onToggleMute }: VolumeControlProps) {
  const Icon = muted || volume <= MIN_VOLUME ? VolumeX : Volume2;

  return (
    <div className="flex items-center gap-2">
      <button
        onClick={onToggleMute}
        className="text-muted-foreground transition-colors hover:text-foreground"
        title={muted ? 'Unmute (M)' : 'Mute (M)'}
      >
        <Icon className="size-4" />
      </button>
      <Slider
        value={[volume]}
        onValueChange={([v]) => onChange(v)}
        min={MIN_VOLUME}
        max={MAX_VOLUME}
        step={1}
        className="w-28"
      />
      <span className="whitespace-nowrap text-right font-mono text-xs text-muted-foreground">
        {volume} dB
      </span>
    </div>
  );
}
