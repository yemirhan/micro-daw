import { Flame } from 'lucide-react';
import { cn } from '@/lib/utils';
import type { StreakData } from '@/types/practiceStats';
import { localDay, type DailySummary } from '@/hooks/usePracticeStats';
import { ChartCard } from './ChartCard';

/** First local day of the current streak (YYYY-MM-DD), or null. */
function streakStartDay(streak: StreakData): string | null {
  if (!streak.lastPracticeDate || streak.currentStreak <= 0) return null;
  const [y, m, d] = streak.lastPracticeDate.split('-').map(Number);
  const start = new Date(y, m - 1, d); // local midnight
  start.setDate(start.getDate() - streak.currentStreak + 1);
  return localDay(start);
}

interface StreakDisplayProps {
  streak: StreakData;
  /** Daily minutes for the last 28 days (heatmap intensity). */
  days?: DailySummary[];
}

const WEEKDAYS = ['S', 'M', 'T', 'W', 'T', 'F', 'S'];

function getHeatmapDays(): { date: string; weekday: number }[] {
  const days: { date: string; weekday: number }[] = [];
  for (let i = 27; i >= 0; i--) {
    const d = new Date();
    d.setDate(d.getDate() - i);
    days.push({ date: localDay(d), weekday: d.getDay() });
  }
  return days;
}

/** 0..4 intensity bucket for a day's minutes. */
function level(minutes: number): number {
  if (minutes <= 0) return 0;
  if (minutes < 5) return 1;
  if (minutes < 15) return 2;
  if (minutes < 30) return 3;
  return 4;
}

const LEVEL_CLASS = ['bg-surface-2', 'bg-note-correct/25', 'bg-note-correct/45', 'bg-note-correct/70', 'bg-note-correct'];

export function StreakDisplay({ streak, days }: StreakDisplayProps) {
  const heatmap = getHeatmapDays();
  const startDay = streakStartDay(streak);
  const today = localDay();
  const minutesByDay = new Map((days ?? []).map((d) => [d.date, d.totalMinutes]));

  return (
    <ChartCard title="Streak" subtitle="Last 4 weeks">
      <div className="mb-4 flex items-end justify-between">
        <div className="flex items-center gap-2">
          <Flame
            className={cn('size-7', streak.currentStreak > 0 ? 'fill-primary/30 text-primary' : 'text-surface-4')}
            aria-hidden
          />
          <span className="font-mono text-3xl font-semibold leading-none tabular-nums">{streak.currentStreak}</span>
          <span className="pb-0.5 text-xs text-muted-foreground">day{streak.currentStreak === 1 ? '' : 's'} in a row</span>
        </div>
        <span className="text-xs text-muted-foreground">
          Best <span className="font-mono font-semibold tabular-nums text-foreground">{streak.longestStreak}</span>
        </span>
      </div>

      <div className="grid grid-cols-7 gap-1.5">
        {heatmap.slice(0, 7).map((d) => (
          <span key={`label-${d.date}`} className="text-center font-mono text-[10px] text-muted-foreground">
            {WEEKDAYS[d.weekday]}
          </span>
        ))}
        {heatmap.map((day) => {
          const minutes = minutesByDay.get(day.date);
          const inStreak =
            startDay !== null && streak.lastPracticeDate !== null && day.date >= startDay && day.date <= streak.lastPracticeDate;
          const lvl = minutes !== undefined ? level(minutes) : inStreak ? 3 : 0;
          const isToday = day.date === today;
          return (
            <div
              key={day.date}
              className={cn('h-5 rounded-[3px]', LEVEL_CLASS[lvl], isToday && 'ring-1 ring-foreground/60 ring-offset-1 ring-offset-surface-1')}
              title={`${day.date}${minutes ? ` · ${Math.round(minutes)} min` : ''}`}
            />
          );
        })}
      </div>
      <div className="mt-3 flex items-center justify-end gap-1 text-[10px] text-muted-foreground">
        Less
        {LEVEL_CLASS.map((c, i) => (
          <span key={i} className={cn('size-2.5 rounded-[2px]', c)} aria-hidden />
        ))}
        More
      </div>
    </ChartCard>
  );
}
