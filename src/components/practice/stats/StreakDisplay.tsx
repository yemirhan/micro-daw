import { Flame } from 'lucide-react';
import type { StreakData } from '@/types/practiceStats';

interface StreakDisplayProps {
  streak: StreakData;
}

function getWeekHeatmap(): { date: string; dayLabel: string }[] {
  const days: { date: string; dayLabel: string }[] = [];
  const labels = ['S', 'M', 'T', 'W', 'T', 'F', 'S'];
  for (let i = 27; i >= 0; i--) {
    const d = new Date();
    d.setDate(d.getDate() - i);
    days.push({
      date: d.toISOString().slice(0, 10),
      dayLabel: labels[d.getDay()],
    });
  }
  return days;
}

export function StreakDisplay({ streak }: StreakDisplayProps) {
  const heatmap = getWeekHeatmap();

  return (
    <div
      className="rounded-xl border border-border/50 p-4"
      style={{ backgroundColor: 'oklch(0.14 0.01 270)' }}
    >
      <div className="flex items-center justify-between mb-3">
        <h3 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
          Streak
        </h3>
        <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
          Best: {streak.longestStreak} days
        </div>
      </div>

      {/* Current streak */}
      <div className="flex items-center gap-2 mb-4">
        <Flame
          className="h-6 w-6"
          style={{ color: streak.currentStreak > 0 ? 'oklch(0.70 0.18 65)' : 'oklch(0.30 0.02 270)' }}
        />
        <span className="text-2xl font-bold tabular-nums">{streak.currentStreak}</span>
        <span className="text-xs text-muted-foreground">day streak</span>
      </div>

      {/* 4-week heatmap grid */}
      <div className="grid grid-cols-7 gap-1">
        {heatmap.map((day) => {
          const isPracticed = streak.lastPracticeDate
            ? day.date <= streak.lastPracticeDate && day.date > (() => {
              // Check if within streak range
              const streakStart = new Date(streak.lastPracticeDate!);
              streakStart.setDate(streakStart.getDate() - streak.currentStreak + 1);
              return streakStart.toISOString().slice(0, 10);
            })()
            : false;
          const isToday = day.date === new Date().toISOString().slice(0, 10);

          return (
            <div
              key={day.date}
              className="aspect-square rounded-sm"
              title={day.date}
              style={{
                backgroundColor: isPracticed
                  ? 'oklch(0.65 0.20 155 / 0.6)'
                  : 'oklch(0.20 0.01 270)',
                outline: isToday ? '1.5px solid oklch(0.55 0.02 270)' : undefined,
              }}
            />
          );
        })}
      </div>
    </div>
  );
}
