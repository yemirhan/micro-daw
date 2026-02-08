import { LineChart, Line, XAxis, YAxis, Tooltip, ResponsiveContainer } from 'recharts';
import { CHART_COLORS } from '@/utils/chartTheme';
import type { AccuracyPoint } from '@/hooks/usePracticeStats';

interface AccuracyTrendChartProps {
  data: AccuracyPoint[];
}

function formatDate(dateStr: string): string {
  const d = new Date(dateStr + 'T00:00:00');
  return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}

export function AccuracyTrendChart({ data }: AccuracyTrendChartProps) {
  const chartData = data.map((d) => ({
    date: formatDate(d.date),
    accuracy: d.accuracy,
  }));

  if (chartData.length === 0) {
    return (
      <div
        className="rounded-xl border border-border/50 p-4"
        style={{ backgroundColor: 'oklch(0.14 0.01 270)' }}
      >
        <h3 className="mb-3 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
          Accuracy Trend
        </h3>
        <p className="py-8 text-center text-xs text-muted-foreground">
          Complete some exercises to see your accuracy trend
        </p>
      </div>
    );
  }

  return (
    <div
      className="rounded-xl border border-border/50 p-4"
      style={{ backgroundColor: 'oklch(0.14 0.01 270)' }}
    >
      <h3 className="mb-3 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
        Accuracy Trend
      </h3>
      <div className="h-48">
        <ResponsiveContainer width="100%" height="100%">
          <LineChart data={chartData} margin={{ top: 4, right: 4, bottom: 0, left: -20 }}>
            <XAxis
              dataKey="date"
              tick={{ fontSize: 10, fill: CHART_COLORS.text }}
              tickLine={false}
              axisLine={{ stroke: CHART_COLORS.grid }}
              interval="preserveStartEnd"
            />
            <YAxis
              domain={[0, 100]}
              tick={{ fontSize: 10, fill: CHART_COLORS.text }}
              tickLine={false}
              axisLine={false}
              unit="%"
            />
            <Tooltip
              contentStyle={{
                backgroundColor: CHART_COLORS.tooltip,
                border: `1px solid ${CHART_COLORS.tooltipBorder}`,
                borderRadius: 8,
                fontSize: 12,
              }}
              labelStyle={{ color: CHART_COLORS.text }}
              formatter={(value: number) => [`${value}%`, 'Accuracy']}
            />
            <Line
              type="monotone"
              dataKey="accuracy"
              stroke={CHART_COLORS.secondary}
              strokeWidth={2}
              dot={{ r: 3, fill: CHART_COLORS.secondary }}
              activeDot={{ r: 5 }}
            />
          </LineChart>
        </ResponsiveContainer>
      </div>
    </div>
  );
}
