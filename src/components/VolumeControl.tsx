import { Volume2, VolumeX } from 'lucide-react';
import { Slider } from '@/components/ui/slider';
import { Button } from '@/components/ui/button';
import { Tip } from '@/components/ui/tip';
import { MIN_VOLUME, MAX_VOLUME } from '@/utils/constants';
import { cn } from '@/lib/utils';

interface VolumeControlProps {
  volume: number;
  onChange: (db: number) => void;
  muted?: boolean;
  onToggleMute?: () => void;
}

export function VolumeControl({ volume, onChange, muted, onToggleMute }: VolumeControlProps) {
  const silent = muted || volume <= MIN_VOLUME;
  const Icon = silent ? VolumeX : Volume2;

  return (
    <div className="flex items-center gap-1.5">
      <Tip label={muted ? 'Unmute output' : 'Mute output'}>
        <Button
          variant="ghost"
          size="icon-sm"
          onClick={onToggleMute}
          aria-pressed={!!muted}
          aria-label={muted ? 'Unmute' : 'Mute'}
          className={cn(muted && 'text-signal-rec hover:text-signal-rec')}
        >
          <Icon />
        </Button>
      </Tip>
      <Slider
        value={[volume]}
        onValueChange={([v]) => onChange(v)}
        min={MIN_VOLUME}
        max={MAX_VOLUME}
        step={1}
        className={cn('w-20', muted && 'opacity-50')}
        aria-label="Output volume"
        title={`Output ${volume > 0 ? `+${volume}` : volume} dB`}
      />
      <span className="hidden w-[46px] text-right font-mono text-[11px] whitespace-nowrap text-muted-foreground tabular-nums min-[1500px]:inline">
        {volume > 0 ? `+${volume}` : volume} dB
      </span>
    </div>
  );
}
