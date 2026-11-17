import { Timer, Target, Flame, Hash } from 'lucide-react';
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

  // Average accuracy over recent scored sessions
  const scoredSessions = recentSessions.filter((s) => s.accuracy != null);
  const avgAccuracy = scoredSessions.length > 0
    ? Math.round(scoredSessions.reduce((sum, s) => sum + (s.accuracy ?? 0), 0) / scoredSessions.length)
    : null;
  const weekMinutes = Math.round(dailySummaries.slice(-7).reduce((sum, d) => sum + d.totalMinutes, 0));

  return (
    <div className="flex flex-col gap-4">
      {/* Summary tiles */}
      <div className="grid grid-cols-4 gap-3">
        <StatCard icon={Timer} value={formatTotalTime(totalTime)} label="Total practice" detail={`${weekMinutes} min this week`} />
        <StatCard icon={Hash} value={stats.totalSessions} label="Sessions" detail={`${scoredSessions.length} scored recently`} />
        <StatCard
          icon={Target}
          value={avgAccuracy !== null ? `${avgAccuracy}%` : '--'}
          label="Avg accuracy"
          detail={avgAccuracy !== null ? `Over the last ${scoredSessions.length} scored` : 'Finish a scored exercise'}
        />
        <StatCard icon={Flame} value={`${streak.currentStreak}d`} label="Streak" detail={`Best ${streak.longestStreak} days`} />
      </div>

      {/* Charts row */}
      <div className="grid grid-cols-2 gap-4">
        <PracticeTimeChart data={dailySummaries} />
        <AccuracyTrendChart data={accuracyTrend} />
      </div>

      {/* Bottom row */}
      <div className="grid grid-cols-3 gap-4">
        <CategoryBreakdown data={categoryBreakdown} />
        <StreakDisplay streak={streak} days={dailySummaries.slice(-28)} />
        <RecentSessions sessions={recentSessions} />
      </div>
    </div>
  );
}
