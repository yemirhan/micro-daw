import { useEffect, useState } from 'react';
import { Headphones, Play, RotateCcw } from 'lucide-react';
import { cn } from '@/lib/utils';
import { audioPromptPlayer } from '@/services/AudioPromptPlayer';
import type { LessonStep } from '@/types/appMode';
import { AnswerOptions } from './AnswerOptions';
import { StepHint } from './StepStage';

interface ListenAndIdentifyStepProps {
  step: LessonStep;
  completed: boolean;
  onAnswer: (correct: boolean) => void;
}

const PROMPT_KIND: Record<string, string> = {
  interval: 'Two notes',
  chord: 'A chord',
  'note-sequence': 'A short melody',
};

/** Animated "now playing" bars (static when reduced motion is on). */
function PlayingBars({ active }: { active: boolean }) {
  return (
    <span className="flex h-4 items-end gap-[3px]" aria-hidden>
      {[0, 1, 2, 3].map((i) => (
        <span
          key={i}
          className={cn('w-[3px] rounded-full bg-current', active ? 'animate-pulse' : 'opacity-40')}
          style={{ height: active ? `${[60, 100, 45, 80][i]}%` : '30%', animationDelay: `${i * 120}ms`, animationDuration: '600ms' }}
        />
      ))}
    </span>
  );
}

export function ListenAndIdentifyStep({ step, completed, onAnswer }: ListenAndIdentifyStepProps) {
  const [hasPlayed, setHasPlayed] = useState(false);
  const [playing, setPlaying] = useState(false);

  // Track playback; stop the prompt when the step goes away.
  useEffect(() => {
    const off = audioPromptPlayer.onPlaybackChange((p) => setPlaying(p));
    return () => {
      off();
      audioPromptPlayer.stop();
    };
  }, []);

  const handlePlay = () => {
    if (!step.audioPrompt) return;
    audioPromptPlayer.play(step.audioPrompt);
    setHasPlayed(true);
  };

  const showOptions = hasPlayed || completed;
  const kind = step.audioPrompt ? PROMPT_KIND[step.audioPrompt.type] ?? 'A sound' : 'A sound';

  return (
    <div className="flex flex-col gap-5">
      <p className="max-w-[68ch] text-lg font-medium leading-relaxed text-foreground">{step.description}</p>

      <div className="flex items-center gap-5 rounded-xl border border-border bg-surface-1 px-5 py-4 shadow-[inset_0_1px_0_var(--hairline)]">
        <button
          type="button"
          onClick={handlePlay}
          className={cn(
            'relative flex size-14 shrink-0 items-center justify-center rounded-full outline-none transition-[transform,background-color] duration-150',
            'focus-visible:ring-2 focus-visible:ring-ring/70 active:scale-95',
            hasPlayed
              ? 'border border-border bg-surface-3 text-foreground hover:bg-surface-4'
              : 'bg-primary text-primary-foreground hover:bg-primary/90',
          )}
          aria-label={hasPlayed ? 'Play again' : 'Play'}
          title={hasPlayed ? 'Play again' : 'Play'}
        >
          {hasPlayed ? <RotateCcw className="size-5" aria-hidden /> : <Play className="ml-0.5 size-6 fill-current" aria-hidden />}
        </button>
        <div className="min-w-0 flex-1">
          <p className="flex items-center gap-2 text-[15px] font-semibold text-foreground">
            <Headphones className="size-4 text-muted-foreground" aria-hidden />
            {kind}
            <span className={cn('ml-1 text-primary', !playing && 'text-muted-foreground')}>
              <PlayingBars active={playing} />
            </span>
          </p>
          <p className="mt-0.5 text-[13px] text-muted-foreground">
            {playing
              ? 'Listening…'
              : hasPlayed
                ? 'Listen as many times as you like, then pick an answer.'
                : 'Press play to hear it. The answers appear after the first listen.'}
          </p>
        </div>
      </div>

      {showOptions && (
        <div className="animate-in fade-in-0 slide-in-from-bottom-1 duration-200">
          <AnswerOptions options={step.quizOptions ?? []} completed={completed} onAnswer={onAnswer} />
        </div>
      )}

      {step.hint && !completed && <StepHint>{step.hint}</StepHint>}
    </div>
  );
}
