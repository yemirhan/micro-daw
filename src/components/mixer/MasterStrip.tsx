import { LevelMeter, MeterScale } from './LevelMeter';
import { STRIP_FADER_HEIGHT } from './ChannelStrip';

interface MasterStripProps {
  level: number;
}

export function MasterStrip({ level }: MasterStripProps) {
  const clipping = Number.isFinite(level) && level > -0.5;
  return (
    <div className="relative flex w-[104px] shrink-0 flex-col items-center gap-2 border-l border-border bg-surface-2 px-2 pt-2.5 pb-2">
      <span aria-hidden className="absolute inset-x-0 top-0 h-[3px] bg-primary" />
      <span className="daw-label text-foreground">Master</span>

      {/* Spacer matching the channel pan knob */}
      <div className="flex h-[62px] items-center">
        <span
          className={`rounded-[3px] px-1.5 py-0.5 font-mono text-[9.5px] font-bold tracking-[0.06em] ${clipping ? 'bg-signal-rec text-white' : 'bg-surface-3 text-muted-foreground'}`}
          title="Lights up when the output is near 0 dBFS"
        >
          CLIP
        </span>
      </div>

      <div className="flex items-start gap-1">
        <LevelMeter level={level} height={STRIP_FADER_HEIGHT} width={9} />
        <LevelMeter level={level} height={STRIP_FADER_HEIGHT} width={9} />
        <MeterScale height={STRIP_FADER_HEIGHT} align="left" />
      </div>

      <span className="daw-lcd w-full py-0.5 text-center text-[10.5px] text-foreground/85">
        {Number.isFinite(level) ? `${Math.round(level)} dB` : '-∞ dB'}
      </span>
      <div className="h-6" />
    </div>
  );
}
