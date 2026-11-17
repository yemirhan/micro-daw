import { ScaleSelector } from '@/components/ScaleSelector';
import { cn } from '@/lib/utils';
import type { ScaleName } from '@/types/music';

const NOTE_NAMES = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'] as const;

interface LearningPanelProps {
  selectedRoot: number;
  selectedScale: ScaleName | null;
  highlightedNotes: Set<number>;
  onRootChange: (root: number) => void;
  onScaleChange: (scale: ScaleName | null) => void;
}

export function LearningPanel({
  selectedRoot,
  selectedScale,
  highlightedNotes,
  onRootChange,
  onScaleChange,
}: LearningPanelProps) {
  return (
    <div className="flex flex-col gap-2 rounded-xl border border-border bg-surface-1 p-3 shadow-[inset_0_1px_0_var(--hairline)]">
      <div className="flex items-center justify-between gap-3">
        <span className="daw-label">Scale</span>
        <ScaleSelector
          selectedRoot={selectedRoot}
          selectedScale={selectedScale}
          onRootChange={onRootChange}
          onScaleChange={onScaleChange}
        />
      </div>

      {selectedScale && (
        <div className="flex flex-wrap gap-1" aria-label="Notes in scale">
          {NOTE_NAMES.map((name, i) => {
            const inScale = highlightedNotes.has(i);
            return (
              <span
                key={i}
                className={cn(
                  'inline-flex h-6 min-w-7 items-center justify-center rounded border px-1 font-mono text-[10.5px] font-semibold',
                  inScale
                    ? i === selectedRoot
                      ? 'border-primary bg-primary text-primary-foreground'
                      : 'border-primary/40 bg-primary/12 text-primary'
                    : 'border-border bg-surface-2 text-muted-foreground/45',
                )}
              >
                {name}
              </span>
            );
          })}
        </div>
      )}
    </div>
  );
}
