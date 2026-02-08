import { LevelMeter } from './LevelMeter';

interface MasterStripProps {
  level: number;
}

export function MasterStrip({ level }: MasterStripProps) {
  return (
    <div className="flex flex-col items-center gap-1.5 px-3 py-2 min-w-[64px] border-l border-border">
      <span className="text-[10px] font-bold uppercase tracking-wider text-muted-foreground">
        Master
      </span>

      <div className="flex items-end gap-1 mt-auto">
        <LevelMeter level={level} height={80} />
        <LevelMeter level={level} height={80} />
      </div>

      <span className="font-mono text-[9px] text-muted-foreground">
        {level > -Infinity ? `${Math.round(level)}dB` : '-∞'}
      </span>
    </div>
  );
}
