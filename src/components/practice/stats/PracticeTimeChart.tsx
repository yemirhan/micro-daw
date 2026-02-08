import { AreaChart, Area, XAxis, YAxis, Tooltip, ResponsiveContainer } from 'recharts';
import { CHART_COLORS } from '@/utils/chartTheme';
import type { DailySummary } from '@/hooks/usePracticeStats';

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
  }));

  return (
    <div
      className="rounded-xl border border-border/50 p-4"
      style={{ backgroundColor: 'oklch(0.14 0.01 270)' }}
    >
      <h3 className="mb-3 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
        Practice Time (Last 30 Days)
      </h3>
      <div className="h-48">
        <ResponsiveContainer width="100%" height="100%">
          <AreaChart data={chartData} margin={{ top: 4, right: 4, bottom: 0, left: -20 }}>
            <defs>
              <linearGradient id="practiceTimeGradient" x1="0" y1="0" x2="0" y2="1">
                <stop offset="0%" stopColor={CHART_COLORS.primary} stopOpacity={0.3} />
                <stop offset="100%" stopColor={CHART_COLORS.primary} stopOpacity={0} />
              </linearGradient>
            </defs>
            <XAxis
              dataKey="date"
              tick={{ fontSize: 10, fill: CHART_COLORS.text }}
              tickLine={false}
              axisLine={{ stroke: CHART_COLORS.grid }}
              interval="preserveStartEnd"
            />
            <YAxis
              tick={{ fontSize: 10, fill: CHART_COLORS.text }}
              tickLine={false}
              axisLine={false}
              unit="m"
            />
            <Tooltip
              contentStyle={{
                backgroundColor: CHART_COLORS.tooltip,
                border: `1px solid ${CHART_COLORS.tooltipBorder}`,
                borderRadius: 8,
                fontSize: 12,
              }}
              labelStyle={{ color: CHART_COLORS.text }}
              formatter={(value: number) => [`${value} min`, 'Practice']}
            />
            <Area
              type="monotone"
              dataKey="minutes"
              stroke={CHART_COLORS.primary}
              fill="url(#practiceTimeGradient)"
              strokeWidth={2}
            />
          </AreaChart>
        </ResponsiveContainer>
      </div>
    </div>
  );
}
