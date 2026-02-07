import { cn } from '@/lib/utils';
import { isBlackKey, midiToNoteName } from '@/utils/noteHelpers';

interface PianoKeyProps {
  midiNote: number;
  isActive: boolean;
  isHighlighted: boolean;
  onPointerDown: (note: number) => void;
  onPointerUp: (note: number) => void;
  onPointerEnter: (note: number) => void;
  showLabel: boolean;
  compact?: boolean;
}

export function PianoKey({ midiNote, isActive, isHighlighted, onPointerDown, onPointerUp, onPointerEnter, showLabel, compact }: PianoKeyProps) {
  const black = isBlackKey(midiNote);
  const noteName = midiToNoteName(midiNote);

  return (
    <button
      className={cn(
        'relative flex flex-col items-center justify-end select-none transition-colors duration-75',
        black
          ? cn(
              'z-10 rounded-b-md border border-border/50',
              compact ? 'h-[60px] w-[24px]' : 'h-[100px] w-[30px]',
              isActive
                ? 'shadow-[0_0_12px_2px] shadow-primary/40'
                : isHighlighted
                  ? 'ring-1 ring-primary/40'
                  : 'hover:brightness-125',
            )
          : cn(
              'rounded-b-lg border border-border/30',
              compact ? 'h-[100px] w-[32px]' : 'h-[160px] w-[40px]',
              isActive
                ? 'shadow-[0_0_12px_2px] shadow-primary/30'
                : isHighlighted
                  ? 'ring-1 ring-primary/30'
                  : '',
            ),
      )}
      style={{
        backgroundColor: black
          ? isActive
            ? 'var(--primary)'
            : isHighlighted
              ? 'oklch(0.20 0.04 265)'
              : 'oklch(0.15 0.02 270)'
          : isActive
            ? 'oklch(0.75 0.12 265)'
            : isHighlighted
              ? 'oklch(0.85 0.03 265)'
              : 'oklch(0.90 0.008 270)',
      }}
      onPointerDown={(e) => {
        e.preventDefault();
        onPointerDown(midiNote);
      }}
      onPointerUp={(e) => {
        e.preventDefault();
        onPointerUp(midiNote);
      }}
      onPointerEnter={() => onPointerEnter(midiNote)}
    >
      {showLabel && !black && (
        <span
          className={cn(
            'mb-2 font-mono text-[10px] font-medium',
            isActive ? 'text-primary-foreground' : 'text-muted-foreground',
          )}
        >
          {noteName}
        </span>
      )}
    </button>
  );
}
