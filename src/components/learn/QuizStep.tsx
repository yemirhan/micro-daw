import { useState } from 'react';
import { cn } from '@/lib/utils';
import type { LessonStep } from '@/types/appMode';

interface QuizStepProps {
  step: LessonStep;
  completed: boolean;
  onAnswer: (correct: boolean) => void;
}

export function QuizStep({ step, completed, onAnswer }: QuizStepProps) {
  const [selected, setSelected] = useState<number | null>(null);
  const [showResult, setShowResult] = useState(false);

  const handleSelect = (index: number) => {
    if (completed || showResult) return;
    setSelected(index);
    setShowResult(true);
    const isCorrect = step.quizOptions?.[index]?.correct ?? false;
    onAnswer(isCorrect);
  };

  const resetQuiz = () => {
    setSelected(null);
    setShowResult(false);
  };

  return (
    <div className="flex flex-col gap-4">
      <p className="text-sm font-medium text-foreground">{step.description}</p>

      <div className="flex flex-col gap-2">
        {step.quizOptions?.map((opt, i) => {
          const isSelected = selected === i;
          const isCorrectAnswer = opt.correct;
          let bgStyle = 'oklch(0.18 0.01 270)';
          if (showResult && isSelected) {
            bgStyle = isCorrectAnswer
              ? 'oklch(0.30 0.15 150)'
              : 'oklch(0.30 0.15 25)';
          } else if (showResult && isCorrectAnswer) {
            bgStyle = 'oklch(0.25 0.10 150)';
          }

          return (
            <button
              key={i}
              onClick={() => handleSelect(i)}
              disabled={showResult}
              className={cn(
                'rounded-lg px-4 py-2.5 text-left text-sm transition-colors',
                !showResult && 'hover:brightness-125',
              )}
              style={{ backgroundColor: bgStyle }}
            >
              {opt.label}
            </button>
          );
        })}
      </div>

      {showResult && selected !== null && !step.quizOptions?.[selected]?.correct && (
        <button
          onClick={resetQuiz}
          className="self-start rounded-md px-3 py-1.5 text-xs text-muted-foreground hover:text-foreground"
          style={{ backgroundColor: 'oklch(0.20 0.02 270)' }}
        >
          Try again
        </button>
      )}
    </div>
  );
}
