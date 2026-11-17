import { cn } from '@/lib/utils';
import type { ChordInfo } from '@/types/music';

interface ChordDisplayProps {
  chord: ChordInfo | null;
}

/** Compact chord readout (LCD style). */
export function ChordDisplay({ chord }: ChordDisplayProps) {
  return (
    <span
      className={cn(
        'daw-lcd inline-flex h-7 min-w-14 items-center justify-center px-2 text-xs font-semibold',
        chord ? 'text-primary' : 'text-muted-foreground/50',
      )}
      aria-live="polite"
      aria-label={chord ? `Chord ${chord.display}` : 'No chord'}
    >
      {chord ? chord.display : '---'}
    </span>
  );
}
