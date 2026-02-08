import { Clock, CheckCircle2, Piano, Drum, BookOpen, Headphones, Music } from 'lucide-react';
import { cn } from '@/lib/utils';
import type { LessonMeta, LessonCategory, LessonDifficulty } from '@/types/appMode';

interface LessonCardProps {
  lesson: LessonMeta;
  isCompleted: boolean;
  onClick: () => void;
}

const CATEGORY_ICON: Record<LessonCategory, typeof Piano> = {
  piano: Piano,
  drums: Drum,
  theory: BookOpen,
  'ear-training': Headphones,
  songs: Music,
};

const DIFFICULTY_COLORS: Record<LessonDifficulty, string> = {
  beginner: 'bg-green-500/20 text-green-400',
  intermediate: 'bg-yellow-500/20 text-yellow-400',
  advanced: 'bg-red-500/20 text-red-400',
};

export function LessonCard({ lesson, isCompleted, onClick }: LessonCardProps) {
  const Icon = CATEGORY_ICON[lesson.category];

  return (
    <button
      onClick={onClick}
      className={cn(
        'group flex flex-col gap-3 rounded-xl border border-border/50 p-4 text-left',
        'transition-all duration-150 hover:border-border hover:bg-card/60',
        isCompleted && 'border-green-500/30',
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
        {isCompleted && <CheckCircle2 className="h-5 w-5 text-green-400" />}
      </div>

      <div>
        <h3 className="text-sm font-semibold text-foreground">{lesson.title}</h3>
        <p className="mt-1 text-xs text-muted-foreground line-clamp-2">{lesson.description}</p>
      </div>

      <div className="flex items-center gap-2">
        <span className={cn('rounded-md px-2 py-0.5 text-[10px] font-medium', DIFFICULTY_COLORS[lesson.difficulty])}>
          {lesson.difficulty}
        </span>
        <span className="flex items-center gap-1 text-[10px] text-muted-foreground">
          <Clock className="h-3 w-3" />
          {lesson.estimatedMinutes}m
        </span>
      </div>
    </button>
  );
}
