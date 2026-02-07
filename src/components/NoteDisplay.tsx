import { midiToNoteName } from '@/utils/noteHelpers';
import type { ActiveNote } from '@/hooks/useMidiNotes';

interface NoteDisplayProps {
  activeNotes: Map<number, ActiveNote>;
}

export function NoteDisplay({ activeNotes }: NoteDisplayProps) {
  const noteNames = Array.from(activeNotes.keys())
    .sort((a, b) => a - b)
    .map(midiToNoteName);

  return (
    <div className="flex items-center gap-2">
      <span className="text-muted-foreground text-[11px] font-semibold uppercase tracking-wider">
        Notes
      </span>
      <div className="flex min-w-[120px] items-center gap-1.5 rounded-md bg-secondary/50 px-3 py-1.5">
        {noteNames.length > 0 ? (
          noteNames.map((name, i) => (
            <span
              key={i}
              className="animate-note-pop rounded bg-primary/20 px-1.5 py-0.5 font-mono text-xs font-semibold text-primary"
            >
              {name}
            </span>
          ))
        ) : (
          <span className="text-muted-foreground/50 font-mono text-xs">---</span>
        )}
      </div>
    </div>
  );
}
