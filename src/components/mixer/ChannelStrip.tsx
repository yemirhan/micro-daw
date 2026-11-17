import { Knob } from '@/components/ui/Knob';
import { TrackToggle } from '@/components/arrangement/TrackToggles';
import { Fader } from './Fader';
import { LevelMeter, MeterScale } from './LevelMeter';
import type { Track } from '@/types/arrangement';
import { MIN_VOLUME, MAX_VOLUME } from '@/utils/constants';
import { cn } from '@/lib/utils';

interface ChannelStripProps {
  track: Track;
  level: number;
  onVolumeChange: (db: number) => void;
  onPanChange: (pan: number) => void;
  onMuteToggle: () => void;
  onSoloToggle: () => void;
}

export const STRIP_FADER_HEIGHT = 104;

const fmtPan = (p: number) => (p > 0.005 ? `R${Math.round(p * 100)}` : p < -0.005 ? `L${Math.round(Math.abs(p) * 100)}` : 'C');

export function ChannelStrip({
  track,
  level,
  onVolumeChange,
  onPanChange,
  onMuteToggle,
  onSoloToggle,
}: ChannelStripProps) {
  return (
    <div
      className={cn(
        'relative flex w-[92px] shrink-0 flex-col items-center gap-2 border-r border-border bg-surface-1 px-2 pt-2.5 pb-2',
        track.muted && 'opacity-70',
      )}
    >
      <span aria-hidden className="absolute inset-x-0 top-0 h-[3px]" style={{ backgroundColor: track.color }} />

      {/* Name */}
      <span className="w-full truncate text-center text-[11px] font-semibold" title={track.name}>
        {track.name}
      </span>

      {/* Pan */}
      <Knob
        label="Pan"
        value={track.pan ?? 0}
        min={-1}
        max={1}
        step={0.01}
        defaultValue={0}
        size="sm"
        formatValue={fmtPan}
        onChange={onPanChange}
      />

      {/* Fader + meter */}
      <div className="flex items-start gap-1">
        <Fader
          value={track.volume}
          min={MIN_VOLUME}
          max={MAX_VOLUME}
          defaultValue={0}
          height={STRIP_FADER_HEIGHT}
          onChange={onVolumeChange}
          label={`${track.name} volume`}
        />
        <LevelMeter level={level} height={STRIP_FADER_HEIGHT} width={6} />
        <MeterScale height={STRIP_FADER_HEIGHT} align="left" />
      </div>

      {/* Volume readout */}
      <span className="daw-lcd w-full py-0.5 text-center text-[10.5px] text-foreground/85">
        {track.volume > 0 ? `+${track.volume}` : track.volume} dB
      </span>

      {/* Mute / Solo */}
      <div className="flex items-center gap-1">
        <TrackToggle kind="mute" size="md" active={track.muted} onClick={onMuteToggle} title={track.muted ? 'Unmute' : 'Mute'} />
        <TrackToggle kind="solo" size="md" active={track.solo} onClick={onSoloToggle} title={track.solo ? 'Unsolo' : 'Solo'} />
      </div>
    </div>
  );
}
