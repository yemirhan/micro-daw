import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { Check, X } from 'lucide-react';
import { cn } from '@/lib/utils';
import type { QuizOption } from '@/types/appMode';
import { StatusLine } from './kit';

const LETTERS = 'ABCDEFGHI';

interface AnswerOptionsProps {
  options: QuizOption[];
  /** Step already completed (e.g. revisited): show the right answer, locked. */
  completed: boolean;
  onAnswer: (correct: boolean) => void;
  /** Accept 1-9 / A-I keyboard shortcuts. */
  keyboard?: boolean;
}

/**
 * Multiple-choice answers. A wrong pick is marked and disabled, the rest stay
 * available, so the learner can try again right away without the answer
 * being given away. The correct pick locks the question.
 */
export function AnswerOptions({ options, completed, onAnswer, keyboard = true }: AnswerOptionsProps) {
  const [wrong, setWrong] = useState<Set<number>>(() => new Set());
  const [correct, setCorrect] = useState<number | null>(null);
  const [lastWrong, setLastWrong] = useState<number | null>(null);
  const solvedIndex = correct ?? (completed ? options.findIndex((o) => o.correct) : null);
  const locked = solvedIndex !== null && solvedIndex !== -1;

  const choose = (index: number) => {
    if (locked || wrong.has(index) || !options[index]) return;
    const isCorrect = options[index].correct;
    if (isCorrect) setCorrect(index);
    else {
      setWrong((prev) => new Set(prev).add(index));
      setLastWrong(index);
    }
    onAnswer(isCorrect);
  };

  const chooseRef = useRef(choose);
  useLayoutEffect(() => {
    chooseRef.current = choose;
  });
  useEffect(() => {
    if (!keyboard) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented || e.repeat || e.metaKey || e.ctrlKey || e.altKey) return;
      const t = e.target;
      if (t instanceof HTMLInputElement || t instanceof HTMLTextAreaElement || (t instanceof HTMLElement && t.isContentEditable)) return;
      let index = -1;
      if (/^[1-9]$/.test(e.key)) index = Number(e.key) - 1;
      else if (/^[a-i]$/i.test(e.key)) index = LETTERS.indexOf(e.key.toUpperCase());
      if (index < 0 || index >= options.length) return;
      e.preventDefault();
      chooseRef.current(index);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [keyboard, options.length]);

  return (
    <div className="flex flex-col gap-3">
      <div className="grid gap-2 sm:grid-cols-2" role="group" aria-label="Answers">
        {options.map((opt, i) => {
          const isWrong = wrong.has(i);
          const isRight = locked && i === solvedIndex;
          const dim = locked && !isRight;
          return (
            <button
              key={i}
              type="button"
              data-quiz-option={i}
              aria-label={opt.label}
              aria-pressed={isRight || isWrong}
              disabled={locked || isWrong}
              onClick={() => choose(i)}
              className={cn(
                'group flex min-h-14 items-center gap-3 rounded-xl border px-4 py-3 text-left text-[15px] font-medium',
                'outline-none transition-[background-color,border-color,opacity,transform] duration-150 focus-visible:ring-2 focus-visible:ring-ring/60',
                !locked && !isWrong && 'border-border bg-surface-1 text-foreground hover:border-primary/50 hover:bg-surface-2 active:translate-y-px',
                isRight && 'border-note-correct/60 bg-note-correct/12 text-foreground',
                isWrong && 'border-note-wrong/50 bg-note-wrong/10 text-muted-foreground',
                isWrong && i === lastWrong && 'animate-in fade-in-0 zoom-in-[0.98] duration-150',
                dim && 'border-border bg-surface-1 opacity-45',
              )}
            >
              <span
                className={cn(
                  'flex size-7 shrink-0 items-center justify-center rounded-md border font-mono text-xs font-semibold',
                  isRight
                    ? 'border-note-correct bg-note-correct text-background'
                    : isWrong
                      ? 'border-note-wrong/60 text-note-wrong'
                      : 'border-border bg-surface-2 text-muted-foreground group-hover:border-primary/50 group-hover:text-primary',
                )}
                aria-hidden
              >
                {isRight ? <Check className="size-4" strokeWidth={3} /> : isWrong ? <X className="size-4" strokeWidth={3} /> : LETTERS[i]}
              </span>
              <span className={cn('flex-1', isWrong && 'line-through decoration-note-wrong/60')}>{opt.label}</span>
            </button>
          );
        })}
      </div>

      {locked ? (
        <StatusLine tone="success" flashKey="correct">
          Correct!{completed && correct === null ? '' : ' Well done.'}
        </StatusLine>
      ) : lastWrong !== null ? (
        <StatusLine tone="error" flashKey={`wrong-${lastWrong}`}>
          Not quite, &ldquo;{options[lastWrong]?.label}&rdquo; isn&rsquo;t it. Try another answer.
        </StatusLine>
      ) : keyboard ? (
        <p className="text-xs text-muted-foreground">
          Click an answer or press{' '}
          <span className="font-mono">
            {LETTERS[0]}–{LETTERS[Math.max(0, options.length - 1)]}
          </span>{' '}
          on your keyboard.
        </p>
      ) : null}
    </div>
  );
}
