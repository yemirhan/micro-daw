import { Card } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import { ScaleSelector } from '@/components/ScaleSelector';
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
    <Card className="flex !flex-col gap-2 border-border bg-card/60 backdrop-blur-sm p-3">
      <div className="flex items-center justify-between">
        <span className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">Scale</span>
        <ScaleSelector
          selectedRoot={selectedRoot}
          selectedScale={selectedScale}
          onRootChange={onRootChange}
          onScaleChange={onScaleChange}
        />
      </div>

      {selectedScale && (
        <div className="flex flex-wrap gap-1">
          {NOTE_NAMES.map((name, i) => {
            const inScale = highlightedNotes.has(i);
            return (
              <Badge
                key={i}
                variant={inScale ? 'default' : 'secondary'}
                className={`font-mono text-[10px] ${!inScale ? 'opacity-30' : ''}`}
              >
                {name}
              </Badge>
            );
          })}
        </div>
      )}
    </Card>
  );
}
