import { Timer, Target, TrendingUp, Hash } from 'lucide-react';
import { StatCard } from './StatCard';
import { PracticeTimeChart } from './PracticeTimeChart';
import { AccuracyTrendChart } from './AccuracyTrendChart';
import { CategoryBreakdown } from './CategoryBreakdown';
import { StreakDisplay } from './StreakDisplay';
import { RecentSessions } from './RecentSessions';
import type { usePracticeStats } from '@/hooks/usePracticeStats';

interface StatsDashboardProps {
  stats: ReturnType<typeof usePracticeStats>;
}

function formatTotalTime(seconds: number): string {
  const hours = Math.floor(seconds / 3600);
  const mins = Math.floor((seconds % 3600) / 60);
  if (hours > 0) return `${hours}h ${mins}m`;
  return `${mins}m`;
}

export function StatsDashboard({ stats }: StatsDashboardProps) {
  const totalTime = stats.getTotalPracticeTime();
  const streak = stats.getStreak();
  const dailySummaries = stats.getDailySummaries(30);
  const accuracyTrend = stats.getAccuracyTrend();
  const categoryBreakdown = stats.getCategoryBreakdown();
  const recentSessions = stats.getRecentSessions(20);

  // Compute average accuracy from recent sessions
  const scoredSessions = recentSessions.filter((s) => s.accuracy != null);
  const avgAccuracy = scoredSessions.length > 0
    ? Math.round(scoredSessions.reduce((sum, s) => sum + (s.accuracy ?? 0), 0) / scoredSessions.length)
    : 0;

  return (
    <div className="space-y-4">
      {/* Summary cards */}
      <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
        <StatCard
          icon={Timer}
          value={formatTotalTime(totalTime)}
          label="Total Practice"
          color="oklch(0.65 0.20 265)"
        />
        <StatCard
          icon={Hash}
          value={stats.totalSessions}
          label="Sessions"
          color="oklch(0.65 0.20 155)"
        />
        <StatCard
          icon={Target}
          value={avgAccuracy > 0 ? `${avgAccuracy}%` : '—'}
          label="Avg Accuracy"
          color="oklch(0.70 0.18 65)"
        />
        <StatCard
          icon={TrendingUp}
          value={streak.currentStreak}
          label="Day Streak"
          color="oklch(0.62 0.22 25)"
        />
      </div>

      {/* Charts row */}
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <PracticeTimeChart data={dailySummaries} />
        <AccuracyTrendChart data={accuracyTrend} />
      </div>

      {/* Bottom row */}
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-3">
        <CategoryBreakdown data={categoryBreakdown} />
        <StreakDisplay streak={streak} />
        <RecentSessions sessions={recentSessions} />
      </div>
    </div>
  );
}
