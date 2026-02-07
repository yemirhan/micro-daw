import { DrumPads } from '@/components/DrumPads';
import type { LessonStep } from '@/types/appMode';
import type { DrumPadId } from '@/types/drums';

interface PlayDrumStepProps {
  step: LessonStep;
  activePads: Set<DrumPadId>;
  onDrumHit: (padId: DrumPadId, velocity: number) => void;
  completed: boolean;
}

export function PlayDrumStep({ step, activePads, onDrumHit, completed }: PlayDrumStepProps) {
  const highlightedPads = new Set(
    (step.expectedDrumPattern?.pads ?? []) as DrumPadId[],
  );

  return (
    <div className="flex flex-col gap-4">
      <p className="text-sm text-foreground/90">{step.description}</p>

      {step.hint && !completed && (
        <p className="text-xs text-muted-foreground italic">{step.hint}</p>
      )}

      <div className="w-[280px]">
        <DrumPads
          activePads={activePads}
          onHit={onDrumHit}
          highlightedPads={highlightedPads}
        />
      </div>
    </div>
  );
}
