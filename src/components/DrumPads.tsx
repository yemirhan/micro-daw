import { useCallback, useRef } from 'react';
import { cn } from '@/lib/utils';
import { DRUM_SOUNDS } from '@/utils/constants';
import type { DrumPadId } from '@/types/drums';

/** Name + colour shown on a pad (e.g. from the active drum kit). */
export interface DrumPadLook {
  name: string;
  color: string;
}

interface DrumPadsProps {
  activePads: Set<DrumPadId>;
  onHit: (padId: DrumPadId, velocity: number) => void;
  compact?: boolean;
  highlightedPads?: Set<DrumPadId>;
  /** Per-pad name/colour overrides (index = pad); falls back to the built-in labels. */
  looks?: (DrumPadLook | null | undefined)[];
}

/**
 * 2–4 letter pad tag from a sample name: "Kick" → "KCK", "Closed HH" → "CHH",
 * "Sub Kick" → "SKC". Leading vowels are kept, later ones dropped.
 */
export function shortPadName(name: string): string {
  const words = name.trim().split(/[\s_-]+/).filter(Boolean);
  if (words.length === 0) return '—';
  const squeeze = (w: string) => (w.length <= 2 ? w : w[0] + w.slice(1).replace(/[aeiou]/gi, ''));
  const last = squeeze(words[words.length - 1]);
  const head = words.slice(0, -1).map((w) => w[0]).join('');
  // Initials of the leading words + at least two letters of the last one, 3–4 chars.
  return (head + last).slice(0, Math.min(4, Math.max(3, head.length + 2))).toUpperCase();
}

export function DrumPads({ activePads, onHit, compact, highlightedPads, looks }: DrumPadsProps) {
  return (
    <div
      className={cn(
        'grid grid-cols-4 grid-rows-2 border border-border bg-surface-0 shadow-[inset_0_1px_3px_oklch(0_0_0/0.4)]',
        compact ? 'gap-1 rounded-lg p-1' : 'gap-2 rounded-xl p-2',
      )}
    >
      {DRUM_SOUNDS.map((sound) => {
        const look = looks?.[sound.id];
        return (
          <DrumPad
            key={sound.id}
            id={sound.id}
            name={look ? look.name || 'Empty' : sound.name}
            shortName={look ? (look.name ? shortPadName(look.name) : '—') : sound.shortName}
            color={look?.color ?? sound.color}
            isActive={activePads.has(sound.id)}
            isHighlighted={highlightedPads?.has(sound.id) ?? false}
            onHit={onHit}
            compact={compact}
          />
        );
      })}
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
      if (e.button !== 0) return; // primary button / touch / pen only
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
        'select-none outline-none focus-visible:ring-2 focus-visible:ring-ring/70',
        compact ? 'h-[52px] rounded-[5px]' : 'h-16 rounded-lg',
        isHighlighted && !isActive && 'ring-2 ring-inset ring-note-target',
      )}
      style={{
        background: isActive
          ? `color-mix(in oklab, ${color} 82%, oklch(0.2 0 0))`
          : isHighlighted
            ? 'oklch(0.29 0.025 60)'
            : 'var(--surface-3)',
        boxShadow: isActive
          ? 'inset 0 2px 3px oklch(0 0 0 / 0.30)'
          : 'inset 0 1px 0 oklch(1 0 0 / 0.07), inset 0 -2px 0 oklch(0 0 0 / 0.30), 0 1px 2px oklch(0 0 0 / 0.35)',
        transition: 'background-color 80ms ease, box-shadow 80ms ease',
      }}
      type="button"
      aria-label={name}
      data-pad={id}
      data-state={isActive ? 'active' : isHighlighted ? 'target' : 'idle'}
      onPointerDown={handlePointerDown}
      onContextMenu={(e) => e.preventDefault()}
    >
      {/* Pad colour LED */}
      <span
        aria-hidden
        className={cn('absolute rounded-full', compact ? 'top-1.5 left-1.5 h-1 w-2.5' : 'top-2 left-2 h-1 w-3.5')}
        style={{
          background: isActive ? 'oklch(1 0 0 / 0.85)' : color,
          opacity: isActive ? 1 : 0.85,
        }}
      />
      {isHighlighted && !isActive && (
        <span aria-hidden className={cn('absolute rounded-full bg-note-target', compact ? 'top-1.5 right-1.5 size-1.5' : 'top-2 right-2 size-2')} />
      )}

      {/* Pad label */}
      <span
        className={cn(
          'font-mono font-semibold tracking-[0.08em] transition-colors duration-75',
          compact ? 'text-[9.5px]' : 'text-[12px]',
          isActive ? 'text-white' : 'text-foreground/70',
        )}
      >
        {shortName}
      </span>

      {/* Full name (non-compact only) */}
      {!compact && (
        <span className={cn('mt-0.5 text-[10px] font-medium', isActive ? 'text-white/80' : 'text-muted-foreground')}>
          {name}
        </span>
      )}

      {/* Hit flash overlay */}
      <div
        className="drum-pad-flash pointer-events-none absolute inset-0"
        style={{ background: 'oklch(1 0 0 / 0.18)', borderRadius: 'inherit' }}
      />
    </button>
  );
}
