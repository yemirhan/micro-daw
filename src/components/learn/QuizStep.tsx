import type { LessonStep } from '@/types/appMode';
import { AnswerOptions } from './AnswerOptions';

interface QuizStepProps {
  step: LessonStep;
  completed: boolean;
  onAnswer: (correct: boolean) => void;
}

export function QuizStep({ step, completed, onAnswer }: QuizStepProps) {
  return (
    <div className="flex flex-col gap-5">
      <p className="max-w-[68ch] text-lg font-medium leading-relaxed text-foreground">{step.description}</p>
      <AnswerOptions options={step.quizOptions ?? []} completed={completed} onAnswer={onAnswer} />
    </div>
  );
}
