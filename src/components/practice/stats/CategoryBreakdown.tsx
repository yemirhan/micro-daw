import { BarChart, Bar, XAxis, YAxis, Tooltip, ResponsiveContainer, Cell } from 'recharts';
import { CHART_COLORS, CATEGORY_COLORS } from '@/utils/chartTheme';
import type { CategoryTime } from '@/hooks/usePracticeStats';

interface CategoryBreakdownProps {
  data: CategoryTime[];
}

const CATEGORY_LABELS: Record<string, string> = {
  'piano-basics': 'Piano',
  'scales': 'Scales',
  'chords': 'Chords',
  'rhythm': 'Rhythm',
  'drum-patterns': 'Drums',
  'free-play': 'Free Play',
};

export function CategoryBreakdown({ data }: CategoryBreakdownProps) {
  if (data.length === 0) {
    return (
      <div
        className="rounded-xl border border-border/50 p-4"
        style={{ backgroundColor: 'oklch(0.14 0.01 270)' }}
      >
        <h3 className="mb-3 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
          Category Breakdown
        </h3>
        <p className="py-8 text-center text-xs text-muted-foreground">
          No practice data yet
        </p>
      </div>
    );
  }

  const chartData = data.map((d) => ({
    category: CATEGORY_LABELS[d.category] ?? d.category,
    minutes: d.minutes,
    originalCategory: d.category,
  }));

  return (
    <div
      className="rounded-xl border border-border/50 p-4"
      style={{ backgroundColor: 'oklch(0.14 0.01 270)' }}
    >
      <h3 className="mb-3 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
        Category Breakdown
      </h3>
      <div className="h-48">
        <ResponsiveContainer width="100%" height="100%">
          <BarChart data={chartData} margin={{ top: 4, right: 4, bottom: 0, left: -20 }}>
            <XAxis
              dataKey="category"
              tick={{ fontSize: 10, fill: CHART_COLORS.text }}
              tickLine={false}
              axisLine={{ stroke: CHART_COLORS.grid }}
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
              formatter={(value: number) => [`${value} min`, 'Time']}
            />
            <Bar dataKey="minutes" radius={[4, 4, 0, 0]}>
              {chartData.map((entry, index) => (
                <Cell
                  key={index}
                  fill={CATEGORY_COLORS[entry.originalCategory] ?? CHART_COLORS.primary}
                />
              ))}
            </Bar>
          </BarChart>
        </ResponsiveContainer>
      </div>
    </div>
  );
}
