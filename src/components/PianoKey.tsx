import { cn } from '@/lib/utils';
import { isBlackKey, midiToNoteName } from '@/utils/noteHelpers';

interface PianoKeyProps {
  midiNote: number;
  isActive: boolean;
  onPointerDown: (note: number, event: React.PointerEvent<HTMLButtonElement>) => void;
  onPointerEnter: (note: number, event: React.PointerEvent<HTMLButtonElement>) => void;
  showLabel: boolean;
  compact?: boolean;
}

/*
 * Visual states (design tokens):
 *   idle      ivory / graphite key
 *   held      pressed: shorter bevel, accent fill (isActive)
 */
export function PianoKey({ midiNote, isActive, onPointerDown, onPointerEnter, showLabel, compact }: PianoKeyProps) {
  const black = isBlackKey(midiNote);
  const noteName = midiToNoteName(midiNote);
  const state: 'idle' | 'held' = isActive ? 'held' : 'idle';
  const pressed = isActive;

  return (
    <button
      className={cn(
        'relative flex flex-col items-center justify-end select-none outline-none',
        'transition-[background-color,box-shadow,transform] duration-75',
        black
          ? cn(
              'z-10 rounded-b-[4px] border border-black/70',
              compact ? 'h-[60px] w-[24px]' : 'h-[100px] w-[30px]',
              pressed
                ? 'translate-y-px shadow-[inset_0_-2px_0_oklch(0_0_0/0.35)]'
                : 'shadow-[inset_0_-5px_0_oklch(0_0_0/0.45),inset_0_1px_0_oklch(1_0_0/0.10),0_2px_3px_oklch(0_0_0/0.4)]',
              state === 'idle' && 'bg-[oklch(0.24_0.005_65)] hover:bg-[oklch(0.29_0.006_65)]',
              state === 'held' && 'bg-primary',
            )
          : cn(
              'rounded-b-[6px] border border-black/40',
              compact ? 'h-[100px] w-[32px]' : 'h-[160px] w-[40px]',
              pressed
                ? 'translate-y-px shadow-[inset_0_-2px_0_oklch(0_0_0/0.18)]'
                : 'shadow-[inset_0_-5px_0_oklch(0_0_0/0.14),inset_0_1px_0_oklch(1_0_0/0.6)]',
              state === 'idle' && 'bg-[oklch(0.93_0.006_80)] hover:bg-[oklch(0.97_0.004_80)]',
              state === 'held' && 'bg-[oklch(0.80_0.13_58)]',
            ),
      )}
      type="button"
      tabIndex={-1}
      data-midi={midiNote}
      data-state={state}
      aria-label={noteName}
      aria-pressed={isActive}
      onPointerDown={(e) => onPointerDown(midiNote, e)}
      onPointerEnter={(e) => onPointerEnter(midiNote, e)}
    >
      {showLabel && !black && (
        <span
          className={cn(
            'mb-2.5 font-mono text-[10px] font-semibold',
            state === 'idle' ? 'text-black/45' : 'text-black/70',
          )}
        >
          {noteName}
        </span>
      )}
    </button>
  );
}
