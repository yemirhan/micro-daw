import type { LessonStep } from '@/types/appMode';

interface InfoStepProps {
  step: LessonStep;
}

export function InfoStep({ step }: InfoStepProps) {
  return (
    <div className="flex flex-col gap-4">
      <p className="whitespace-pre-line text-sm leading-relaxed text-foreground/90">
        {step.description}
      </p>
    </div>
  );
}
