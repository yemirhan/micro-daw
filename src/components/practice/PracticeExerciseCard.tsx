import { Star, Piano, Waves, Music2, Timer, Drum } from 'lucide-react';
import { cn } from '@/lib/utils';
import type { PracticeExercise, PracticeCategory, PracticeScore } from '@/types/appMode';

interface PracticeExerciseCardProps {
  exercise: PracticeExercise;
  score?: PracticeScore;
  onClick: () => void;
}

const CATEGORY_ICON: Record<PracticeCategory, typeof Piano> = {
  'piano-basics': Piano,
  scales: Waves,
  chords: Music2,
  rhythm: Timer,
  'drum-patterns': Drum,
};

const DIFFICULTY_COLORS: Record<string, string> = {
  beginner: 'bg-green-500/20 text-green-400',
  intermediate: 'bg-yellow-500/20 text-yellow-400',
};

function StarRating({ stars }: { stars: number }) {
  return (
    <div className="flex gap-0.5">
      {[1, 2, 3].map((i) => (
        <Star
          key={i}
          className={cn('h-3.5 w-3.5', i <= stars ? 'fill-yellow-400 text-yellow-400' : 'text-muted-foreground/30')}
        />
      ))}
    </div>
  );
}

export function PracticeExerciseCard({ exercise, score, onClick }: PracticeExerciseCardProps) {
  const Icon = CATEGORY_ICON[exercise.category];

  return (
    <button
      onClick={onClick}
      className={cn(
        'group flex flex-col gap-3 rounded-xl border border-border/50 p-4 text-left',
        'transition-all duration-150 hover:border-border hover:bg-card/60',
        score && score.bestStars > 0 && 'border-yellow-500/20',
      )}
      style={{ backgroundColor: 'oklch(0.14 0.01 270)' }}
    >
      <div className="flex items-start justify-between">
        <div
          className="flex h-9 w-9 items-center justify-center rounded-lg"
          style={{ backgroundColor: 'oklch(0.20 0.02 270)' }}
        >
          <Icon className="h-4.5 w-4.5 text-muted-foreground" />
        </div>
        {score && score.bestStars > 0 && <StarRating stars={score.bestStars} />}
      </div>

      <div>
        <h3 className="text-sm font-semibold text-foreground">{exercise.title}</h3>
        <p className="mt-1 text-xs text-muted-foreground line-clamp-1">{exercise.description}</p>
      </div>

      <div className="flex items-center gap-2">
        <span className={cn('rounded-md px-2 py-0.5 text-[10px] font-medium', DIFFICULTY_COLORS[exercise.difficulty])}>
          {exercise.difficulty}
        </span>
        {score && score.attempts > 0 && (
          <span className="text-[10px] text-muted-foreground">
            {score.attempts} attempt{score.attempts !== 1 ? 's' : ''}
          </span>
        )}
      </div>
    </button>
  );
}
