import { LineChart, Line, XAxis, YAxis, Tooltip, ResponsiveContainer, CartesianGrid } from 'recharts';
import { AXIS_TICK, CHART_COLORS, TOOLTIP_STYLE } from '@/utils/chartTheme';
import type { AccuracyPoint } from '@/hooks/usePracticeStats';
import { ChartCard, ChartEmpty } from './ChartCard';

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
  const latest = data[data.length - 1]?.accuracy;

  return (
    <ChartCard
      title="Accuracy"
      subtitle="Average of scored exercises per day"
      value={
        latest !== undefined ? (
          <>
            {latest}
            <span className="ml-0.5 text-xs text-muted-foreground">% latest</span>
          </>
        ) : undefined
      }
    >
      {chartData.length === 0 ? (
        <ChartEmpty>Finish a scored exercise to see your accuracy trend</ChartEmpty>
      ) : (
        <div className="h-48">
          <ResponsiveContainer width="100%" height="100%">
            <LineChart data={chartData} margin={{ top: 8, right: 8, bottom: 0, left: -18 }}>
              <CartesianGrid vertical={false} stroke={CHART_COLORS.grid} strokeDasharray="2 4" />
              <XAxis
                dataKey="date"
                tick={AXIS_TICK}
                tickLine={false}
                axisLine={{ stroke: CHART_COLORS.grid }}
                interval="preserveStartEnd"
                minTickGap={24}
              />
              <YAxis domain={[0, 100]} ticks={[0, 25, 50, 75, 100]} tick={AXIS_TICK} tickLine={false} axisLine={false} unit="%" />
              <Tooltip
                {...TOOLTIP_STYLE}
                cursor={{ stroke: CHART_COLORS.muted, strokeWidth: 1 }}
                formatter={(value) => [`${value}%`, 'Accuracy']}
              />
              <Line
                type="linear"
                dataKey="accuracy"
                stroke={CHART_COLORS.secondary}
                strokeWidth={2}
                connectNulls
                dot={{ r: 4, fill: CHART_COLORS.secondary, stroke: CHART_COLORS.surface, strokeWidth: 2 }}
                activeDot={{ r: 5, fill: CHART_COLORS.secondary, stroke: CHART_COLORS.surface, strokeWidth: 2 }}
                isAnimationActive={false}
              />
            </LineChart>
          </ResponsiveContainer>
        </div>
      )}
    </ChartCard>
  );
}
