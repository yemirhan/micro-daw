import { useCallback, useState } from 'react';
import { cn } from '@/lib/utils';
import { DRUM_SOUNDS } from '@/utils/constants';
import type { DrumPadId } from '@/types/drums';

interface DrumPadsProps {
  activePads: Set<DrumPadId>;
  onHit: (padId: DrumPadId, velocity: number) => void;
  compact?: boolean;
}

export function DrumPads({ activePads, onHit, compact }: DrumPadsProps) {
  return (
    <div className={cn('grid grid-cols-4 grid-rows-2', compact ? 'gap-1' : 'gap-2')}>
      {DRUM_SOUNDS.map((sound) => (
        <DrumPad
          key={sound.id}
          id={sound.id}
          name={sound.name}
          shortName={sound.shortName}
          color={sound.color}
          isActive={activePads.has(sound.id)}
          onHit={onHit}
          compact={compact}
        />
      ))}
    </div>
  );
}

interface DrumPadProps {
  id: DrumPadId;
  name: string;
  shortName: string;
  color: string;
  isActive: boolean;
  onHit: (padId: DrumPadId, velocity: number) => void;
  compact?: boolean;
}

function DrumPad({ id, name, shortName, color, isActive, onHit, compact }: DrumPadProps) {
  const [animating, setAnimating] = useState(false);

  const handlePointerDown = useCallback(
    (e: React.PointerEvent) => {
      e.preventDefault();
      setAnimating(true);
      onHit(id, 0.8);
      setTimeout(() => setAnimating(false), 120);
    },
    [id, onHit],
  );

  return (
    <button
      className={cn(
        'flex flex-col items-center justify-center rounded-xl border border-border/50',
        'select-none transition-all duration-75',
        compact ? 'h-10' : 'h-16',
        animating && 'animate-pad-hit',
        !isActive && 'hover:brightness-110',
      )}
      style={{
        backgroundColor: isActive ? color : `color-mix(in oklch, ${color} 20%, var(--secondary))`,
        boxShadow: isActive
          ? `0 0 16px 3px ${color}`
          : `inset 0 2px 4px rgba(0,0,0,0.3)`,
      }}
      onPointerDown={handlePointerDown}
    >
      <span className={cn('font-bold text-foreground', compact ? 'text-[10px]' : 'text-xs')}>{shortName}</span>
      {!compact && <span className="text-[9px] text-muted-foreground">{name}</span>}
    </button>
  );
}
