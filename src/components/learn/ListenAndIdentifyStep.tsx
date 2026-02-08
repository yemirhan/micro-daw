import { useState, useEffect } from 'react';
import { Play, RotateCcw } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';
import { audioPromptPlayer } from '@/services/AudioPromptPlayer';
import type { LessonStep } from '@/types/appMode';

interface ListenAndIdentifyStepProps {
  step: LessonStep;
  completed: boolean;
  onAnswer: (correct: boolean) => void;
}

export function ListenAndIdentifyStep({ step, completed, onAnswer }: ListenAndIdentifyStepProps) {
  const [hasPlayed, setHasPlayed] = useState(false);
  const [selected, setSelected] = useState<number | null>(null);
  const [showResult, setShowResult] = useState(false);

  // Clean up on unmount
  useEffect(() => {
    return () => audioPromptPlayer.stop();
  }, []);

  const handlePlay = () => {
    if (!step.audioPrompt) return;
    audioPromptPlayer.play(step.audioPrompt);
    setHasPlayed(true);
  };

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

      {/* Audio playback controls */}
      <div className="flex items-center gap-2">
        <Button
          variant="outline"
          size="sm"
          onClick={handlePlay}
          className="gap-1.5"
          style={!hasPlayed ? { borderColor: 'oklch(0.65 0.20 265 / 0.5)' } : undefined}
        >
          <Play className="h-3.5 w-3.5" />
          {hasPlayed ? 'Listen Again' : 'Play'}
        </Button>
        {!hasPlayed && (
          <span className="text-xs text-muted-foreground">Press play to hear the audio</span>
        )}
      </div>

      {/* Quiz options — shown after first play */}
      {hasPlayed && (
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
      )}

      {showResult && selected !== null && !step.quizOptions?.[selected]?.correct && (
        <button
          onClick={resetQuiz}
          className="self-start flex items-center gap-1 rounded-md px-3 py-1.5 text-xs text-muted-foreground hover:text-foreground"
          style={{ backgroundColor: 'oklch(0.20 0.02 270)' }}
        >
          <RotateCcw className="h-3 w-3" />
          Try again
        </button>
      )}

      {step.hint && !showResult && (
        <p className="text-xs text-muted-foreground italic">{step.hint}</p>
      )}
    </div>
  );
}
