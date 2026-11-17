import type { LessonStep } from '@/types/appMode';
import { StepHint } from './StepStage';

interface InfoStepProps {
  step: LessonStep;
}

export function InfoStep({ step }: InfoStepProps) {
  const paragraphs = step.description.split(/\n{2,}/);
  return (
    <div className="flex max-w-[68ch] flex-col gap-4">
      {paragraphs.map((p, i) => (
        <p key={i} className="whitespace-pre-line text-[17px] leading-[1.7] text-foreground/90">
          {p}
        </p>
      ))}
      {step.hint && <StepHint>{step.hint}</StepHint>}
    </div>
  );
}
