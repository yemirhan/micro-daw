import { useCallback, useRef } from 'react';
import { cn } from '@/lib/utils';
import { DRUM_SOUNDS } from '@/utils/constants';
import type { DrumPadId } from '@/types/drums';

interface DrumPadsProps {
  activePads: Set<DrumPadId>;
  onHit: (padId: DrumPadId, velocity: number) => void;
  compact?: boolean;
  highlightedPads?: Set<DrumPadId>;
}

export function DrumPads({ activePads, onHit, compact, highlightedPads }: DrumPadsProps) {
  return (
    <div
      className={cn(
        'grid grid-cols-4 grid-rows-2',
        compact ? 'gap-[3px] p-[3px]' : 'gap-1.5 p-1.5',
      )}
      style={{
        background: 'oklch(0.11 0.008 270)',
        borderRadius: compact ? 6 : 8,
        boxShadow: 'inset 0 1px 3px oklch(0 0 0 / 0.5), 0 1px 0 oklch(1 0 0 / 0.03)',
      }}
    >
      {DRUM_SOUNDS.map((sound) => (
        <DrumPad
          key={sound.id}
          id={sound.id}
          name={sound.name}
          shortName={sound.shortName}
          color={sound.color}
          isActive={activePads.has(sound.id)}
          isHighlighted={highlightedPads?.has(sound.id) ?? false}
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
  isHighlighted: boolean;
  onHit: (padId: DrumPadId, velocity: number) => void;
  compact?: boolean;
}

function DrumPad({ id, name, shortName, color, isActive, isHighlighted, onHit, compact }: DrumPadProps) {
  const padRef = useRef<HTMLButtonElement>(null);

  const handlePointerDown = useCallback(
    (e: React.PointerEvent) => {
      e.preventDefault();
      onHit(id, 0.8);

      const el = padRef.current;
      if (!el) return;

      // Trigger hit animation by toggling the class
      el.classList.remove('drum-pad-hit');
      // Force reflow
      void el.offsetWidth;
      el.classList.add('drum-pad-hit');
    },
    [id, onHit],
  );

  return (
    <button
      ref={padRef}
      className={cn(
        'drum-pad relative flex flex-col items-center justify-center overflow-hidden',
        'select-none outline-none',
        compact ? 'h-[52px]' : 'h-16',
      )}
      style={{
        // Pad surface
        background: isActive
          ? `linear-gradient(160deg, ${color}, color-mix(in oklch, ${color} 70%, oklch(0.08 0 0)))`
          : 'linear-gradient(160deg, oklch(0.19 0.01 270), oklch(0.14 0.008 270))',
        borderRadius: compact ? 4 : 6,
        // Layered shadows: outer recess + inner concavity + bottom edge
        boxShadow: isActive
          ? `inset 0 1px 2px oklch(0 0 0 / 0.3), 0 0 12px 2px color-mix(in oklch, ${color} 50%, transparent)`
          : isHighlighted
            ? `inset 0 2px 4px oklch(0 0 0 / 0.4), inset 0 -1px 0 oklch(1 0 0 / 0.04), 0 0 8px 1px color-mix(in oklch, ${color} 35%, transparent)`
            : 'inset 0 2px 4px oklch(0 0 0 / 0.4), inset 0 -1px 0 oklch(1 0 0 / 0.04)',
        border: isHighlighted && !isActive
          ? `1px solid color-mix(in oklch, ${color} 50%, transparent)`
          : '1px solid oklch(0.22 0.012 270)',
      }}
      onPointerDown={handlePointerDown}
    >
      {/* LED indicator strip */}
      <div
        className="absolute top-0 right-0 left-0"
        style={{
          height: compact ? 2 : 3,
          background: isActive
            ? color
            : isHighlighted
              ? `color-mix(in oklch, ${color} 70%, transparent)`
              : `color-mix(in oklch, ${color} 25%, oklch(0.15 0 0))`,
          boxShadow: isActive
            ? `0 0 8px 1px ${color}, 0 2px 6px color-mix(in oklch, ${color} 40%, transparent)`
            : 'none',
          transition: 'background 60ms ease, box-shadow 60ms ease',
        }}
      />

      {/* Pad label */}
      <span
        className={cn(
          'font-mono font-semibold tracking-wider',
          compact ? 'text-[9px]' : 'text-[11px]',
        )}
        style={{
          color: isActive
            ? 'oklch(0.97 0.005 270)'
            : 'oklch(0.60 0.015 270)',
          textShadow: isActive ? `0 0 8px ${color}` : 'none',
          transition: 'color 60ms ease',
        }}
      >
        {shortName}
      </span>

      {/* Full name (non-compact only) */}
      {!compact && (
        <span
          className="mt-0.5 text-[8px] font-medium tracking-wide"
          style={{
            color: isActive
              ? 'oklch(0.80 0.01 270)'
              : 'oklch(0.40 0.01 270)',
            transition: 'color 60ms ease',
          }}
        >
          {name}
        </span>
      )}

      {/* Hit flash overlay */}
      <div
        className="drum-pad-flash pointer-events-none absolute inset-0"
        style={{
          background: `radial-gradient(circle at 50% 30%, color-mix(in oklch, ${color} 40%, transparent), transparent 70%)`,
          borderRadius: 'inherit',
        }}
      />
    </button>
  );
}
