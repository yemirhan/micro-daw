import { Volume2, VolumeX } from 'lucide-react';
import { Slider } from '@/components/ui/slider';
import { MIN_VOLUME, MAX_VOLUME } from '@/utils/constants';

interface VolumeControlProps {
  volume: number;
  onChange: (db: number) => void;
}

export function VolumeControl({ volume, onChange }: VolumeControlProps) {
  return (
    <div className="flex items-center gap-2">
      {volume <= MIN_VOLUME ? (
        <VolumeX className="size-4 text-muted-foreground" />
      ) : (
        <Volume2 className="size-4 text-muted-foreground" />
      )}
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
