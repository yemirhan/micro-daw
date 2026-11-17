import { StepTrack } from './kit';

interface LessonProgressProps {
  current: number;
  total: number;
  /** Which steps to paint as done (defaults to the steps before `current`). */
  isDone?: (index: number) => boolean;
}

export function LessonProgress({ current, total, isDone }: LessonProgressProps) {
  const done = isDone ?? ((i: number) => i < current);
  return (
    <div
      className="flex items-center gap-3"
      role="progressbar"
      aria-label="Lesson progress"
      aria-valuemin={1}
      aria-valuemax={total}
      aria-valuenow={current + 1}
      aria-valuetext={`Step ${current + 1} of ${total}`}
    >
      <StepTrack total={total} current={current} isDone={done} className="flex-1" />
      <span className="shrink-0 font-mono text-xs tabular-nums text-muted-foreground">
        <span className="font-semibold text-foreground">{current + 1}</span>/{total}
      </span>
    </div>
  );
}
