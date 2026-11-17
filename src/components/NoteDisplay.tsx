import { midiToNoteName } from '@/utils/noteHelpers';
import type { ActiveNote } from '@/hooks/useMidiNotes';

interface NoteDisplayProps {
  activeNotes: Map<number, ActiveNote>;
}

/** Held notes as mono chips in an LCD well. */
export function NoteDisplay({ activeNotes }: NoteDisplayProps) {
  const noteNames = Array.from(activeNotes.keys())
    .sort((a, b) => a - b)
    .map(midiToNoteName);

  return (
    <div className="flex items-center gap-2">
      <span className="daw-label">Notes</span>
      <div className="daw-lcd flex h-7 min-w-[120px] items-center gap-1 px-2" aria-live="polite">
        {noteNames.length > 0 ? (
          noteNames.map((name, i) => (
            <span
              key={i}
              className="animate-note-pop rounded-sm bg-primary/15 px-1.5 py-px text-[11px] font-semibold text-primary"
            >
              {name}
            </span>
          ))
        ) : (
          <span className="text-xs text-muted-foreground/50">---</span>
        )}
      </div>
    </div>
  );
}
