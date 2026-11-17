import { BarChart, Bar, XAxis, YAxis, Tooltip, ResponsiveContainer, CartesianGrid } from 'recharts';
import { AXIS_TICK, CHART_COLORS, TOOLTIP_STYLE } from '@/utils/chartTheme';
import type { DailySummary } from '@/hooks/usePracticeStats';
import { ChartCard, ChartEmpty } from './ChartCard';

interface PracticeTimeChartProps {
  data: DailySummary[];
}

function formatDate(dateStr: string): string {
  const d = new Date(dateStr + 'T00:00:00');
  return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}

export function PracticeTimeChart({ data }: PracticeTimeChartProps) {
  const chartData = data.map((d) => ({
    date: formatDate(d.date),
    minutes: Math.round(d.totalMinutes * 10) / 10,
    sessions: d.sessions,
  }));
  const total = Math.round(data.reduce((sum, d) => sum + d.totalMinutes, 0));
  const hasData = data.some((d) => d.totalMinutes > 0);

  return (
    <ChartCard
      title="Practice time"
      subtitle={`Minutes per day, last ${data.length} days`}
      value={
        <>
          {total}
          <span className="ml-0.5 text-xs text-muted-foreground">min</span>
        </>
      }
    >
      {hasData ? (
        <div className="h-48">
          <ResponsiveContainer width="100%" height="100%">
            <BarChart data={chartData} margin={{ top: 4, right: 4, bottom: 0, left: -18 }} barCategoryGap={2}>
              <CartesianGrid vertical={false} stroke={CHART_COLORS.grid} strokeDasharray="2 4" />
              <XAxis
                dataKey="date"
                tick={AXIS_TICK}
                tickLine={false}
                axisLine={{ stroke: CHART_COLORS.grid }}
                interval="preserveStartEnd"
                minTickGap={24}
              />
              <YAxis tick={AXIS_TICK} tickLine={false} axisLine={false} unit="m" allowDecimals={false} />
              <Tooltip
                {...TOOLTIP_STYLE}
                cursor={{ fill: CHART_COLORS.cursor }}
                formatter={(value) => [`${value} min`, 'Practice']}
              />
              <Bar dataKey="minutes" fill={CHART_COLORS.primary} radius={[3, 3, 0, 0]} maxBarSize={18} />
            </BarChart>
          </ResponsiveContainer>
        </div>
      ) : (
        <ChartEmpty>No practice in this period yet</ChartEmpty>
      )}
    </ChartCard>
  );
}
