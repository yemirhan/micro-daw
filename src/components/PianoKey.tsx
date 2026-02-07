import { useCallback, useRef } from 'react';
import { cn } from '@/lib/utils';
import { isBlackKey, midiToNoteName } from '@/utils/noteHelpers';

interface PianoKeyProps {
  midiNote: number;
  isActive: boolean;
  isHighlighted: boolean;
  onNoteOn: (note: number, velocity: number) => void;
  onNoteOff: (note: number) => void;
  showLabel: boolean;
  compact?: boolean;
}

export function PianoKey({ midiNote, isActive, isHighlighted, onNoteOn, onNoteOff, showLabel, compact }: PianoKeyProps) {
  const isPressed = useRef(false);
  const black = isBlackKey(midiNote);
  const noteName = midiToNoteName(midiNote);

  const handlePointerDown = useCallback(
    (e: React.PointerEvent) => {
      e.preventDefault();
      (e.target as HTMLElement).setPointerCapture(e.pointerId);
      isPressed.current = true;
      onNoteOn(midiNote, 0.7);
    },
    [midiNote, onNoteOn],
  );

  const handlePointerUp = useCallback(
    (e: React.PointerEvent) => {
      e.preventDefault();
      if (isPressed.current) {
        isPressed.current = false;
        onNoteOff(midiNote);
      }
    },
    [midiNote, onNoteOff],
  );

  const handlePointerLeave = useCallback(
    (e: React.PointerEvent) => {
      if (isPressed.current) {
        isPressed.current = false;
        onNoteOff(midiNote);
      }
    },
    [midiNote, onNoteOff],
  );

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
      onPointerDown={handlePointerDown}
      onPointerUp={handlePointerUp}
      onPointerLeave={handlePointerLeave}
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
