import type { PracticeSessionRecord } from '@/types/practiceStats';
import { StarRating } from '@/components/learn/kit';
import { sessionTitle } from '../practiceMeta';
import { ChartCard, ChartEmpty } from './ChartCard';

interface RecentSessionsProps {
  sessions: PracticeSessionRecord[];
}

function formatDuration(seconds: number): string {
  const m = Math.floor(seconds / 60);
  const s = seconds % 60;
  return m > 0 ? `${m}m ${String(s).padStart(2, '0')}s` : `${s}s`;
}

function formatWhen(iso: string): string {
  const d = new Date(iso);
  const today = new Date();
  const yesterday = new Date();
  yesterday.setDate(today.getDate() - 1);
  const time = d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });
  if (d.toDateString() === today.toDateString()) return `Today ${time}`;
  if (d.toDateString() === yesterday.toDateString()) return `Yesterday ${time}`;
  return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}

export function RecentSessions({ sessions }: RecentSessionsProps) {
  return (
    <ChartCard title="Recent sessions" subtitle={sessions.length ? `Last ${sessions.length}` : undefined}>
      {sessions.length === 0 ? (
        <ChartEmpty>No sessions recorded yet</ChartEmpty>
      ) : (
        <div className="-mx-1 max-h-72 overflow-y-auto px-1">
          <table className="w-full text-xs">
            <thead className="sr-only">
              <tr>
                <th>Session</th>
                <th>Accuracy</th>
                <th>Stars</th>
              </tr>
            </thead>
            <tbody>
              {sessions.map((session, i) => (
                <tr key={`${session.timestamp}-${i}`} className="border-b border-border/60 last:border-0">
                  <td className="max-w-0 py-2 pr-2">
                    <div className="truncate font-medium text-foreground">{sessionTitle(session.exerciseId)}</div>
                    <div className="mt-0.5 text-[11px] text-muted-foreground">
                      {formatWhen(session.timestamp)} · <span className="font-mono tabular-nums">{formatDuration(session.durationSeconds)}</span>
                    </div>
                  </td>
                  <td className="w-12 py-2 text-right font-mono tabular-nums text-foreground/85">
                    {session.accuracy != null ? `${session.accuracy}%` : <span className="text-muted-foreground/60">--</span>}
                  </td>
                  <td className="w-16 py-2 pl-2 text-right">
                    {session.accuracy != null ? <StarRating stars={session.stars} /> : null}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </ChartCard>
  );
}
