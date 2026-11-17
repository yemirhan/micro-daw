import { cn } from '@/lib/utils';
import { isBlackKey, midiToNoteName } from '@/utils/noteHelpers';

export type KeyFeedback = 'correct' | 'wrong';

interface PianoKeyProps {
  midiNote: number;
  isActive: boolean;
  isHighlighted: boolean;
  onPointerDown: (note: number, event: React.PointerEvent<HTMLButtonElement>) => void;
  onPointerEnter: (note: number, event: React.PointerEvent<HTMLButtonElement>) => void;
  showLabel: boolean;
  compact?: boolean;
  /** Optional explicit result for Learn/Practice (overrides the colour). */
  feedback?: KeyFeedback;
}

/*
 * Visual states (design tokens):
 *   idle      ivory / graphite key
 *   target    accent outline + dot (isHighlighted)
 *   held      pressed: shorter bevel, accent fill (isActive)
 *   correct   held on a target, or feedback="correct": green
 *   wrong     feedback="wrong": red
 */
export function PianoKey({ midiNote, isActive, isHighlighted, onPointerDown, onPointerEnter, showLabel, compact, feedback }: PianoKeyProps) {
  const black = isBlackKey(midiNote);
  const noteName = midiToNoteName(midiNote);
  const state: 'idle' | 'target' | 'held' | 'correct' | 'wrong' =
    feedback === 'wrong'
      ? 'wrong'
      : feedback === 'correct' || (isActive && isHighlighted)
        ? 'correct'
        : isActive
          ? 'held'
          : isHighlighted
            ? 'target'
            : 'idle';
  const pressed = isActive || state === 'correct' || state === 'wrong';

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
              state === 'target' && 'bg-[oklch(0.27_0.03_60)] ring-2 ring-inset ring-note-target',
              state === 'held' && 'bg-primary',
              state === 'correct' && 'bg-note-correct',
              state === 'wrong' && 'bg-note-wrong',
            )
          : cn(
              'rounded-b-[6px] border border-black/40',
              compact ? 'h-[100px] w-[32px]' : 'h-[160px] w-[40px]',
              pressed
                ? 'translate-y-px shadow-[inset_0_-2px_0_oklch(0_0_0/0.18)]'
                : 'shadow-[inset_0_-5px_0_oklch(0_0_0/0.14),inset_0_1px_0_oklch(1_0_0/0.6)]',
              state === 'idle' && 'bg-[oklch(0.93_0.006_80)] hover:bg-[oklch(0.97_0.004_80)]',
              state === 'target' && 'bg-[oklch(0.92_0.05_65)] ring-2 ring-inset ring-note-target',
              state === 'held' && 'bg-[oklch(0.80_0.13_58)]',
              state === 'correct' && 'bg-note-correct',
              state === 'wrong' && 'bg-note-wrong',
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
      {/* Target marker */}
      {state === 'target' && (
        <span
          aria-hidden
          className={cn('rounded-full bg-note-target', black ? 'mb-2 size-1.5' : showLabel ? 'mb-1 size-2' : 'mb-2.5 size-2')}
        />
      )}
      {showLabel && !black && (
        <span
          className={cn(
            'mb-2.5 font-mono text-[10px] font-semibold',
            state === 'idle' || state === 'target' ? 'text-black/45' : 'text-black/70',
          )}
        >
          {noteName}
        </span>
      )}
    </button>
  );
}
