import { CATEGORY_COLORS, CHART_COLORS } from '@/utils/chartTheme';
import type { CategoryTime } from '@/hooks/usePracticeStats';
import { ChartCard, ChartEmpty } from './ChartCard';

interface CategoryBreakdownProps {
  data: CategoryTime[];
}

const CATEGORY_LABELS: Record<string, string> = {
  'piano-basics': 'Piano basics',
  scales: 'Scales',
  chords: 'Chords',
  rhythm: 'Rhythm',
  'drum-patterns': 'Drums',
  'free-play': 'Free play',
};

function formatMinutes(min: number): string {
  if (min >= 60) return `${Math.floor(min / 60)}h ${Math.round(min % 60)}m`;
  return `${Math.round(min)}m`;
}

/** Time per category as a labelled horizontal bar list (sorted, one hue). */
export function CategoryBreakdown({ data }: CategoryBreakdownProps) {
  const max = Math.max(...data.map((d) => d.minutes), 0);

  return (
    <ChartCard title="Time by category" subtitle="All time">
      {data.length === 0 ? (
        <ChartEmpty>No practice data yet</ChartEmpty>
      ) : (
        <ul className="flex flex-col gap-2.5">
          {data.map((d) => (
            <li key={d.category} className="grid grid-cols-[88px_1fr_56px] items-center gap-3 text-xs">
              <span className="truncate text-foreground/85">{CATEGORY_LABELS[d.category] ?? d.category}</span>
              <span className="h-2.5 overflow-hidden rounded-sm bg-surface-2">
                <span
                  className="block h-full rounded-sm"
                  style={{
                    width: `${max > 0 ? Math.max(2, (d.minutes / max) * 100) : 0}%`,
                    backgroundColor: CATEGORY_COLORS[d.category] ?? CHART_COLORS.accent,
                  }}
                />
              </span>
              <span className="text-right font-mono tabular-nums text-muted-foreground">{formatMinutes(d.minutes)}</span>
            </li>
          ))}
        </ul>
      )}
    </ChartCard>
  );
}
