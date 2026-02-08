import { Star, Clock } from 'lucide-react';
import type { PracticeSessionRecord } from '@/types/practiceStats';

interface RecentSessionsProps {
  sessions: PracticeSessionRecord[];
}

function formatDuration(seconds: number): string {
  const m = Math.floor(seconds / 60);
  const s = seconds % 60;
  return m > 0 ? `${m}m ${s}s` : `${s}s`;
}

function formatTime(iso: string): string {
  const d = new Date(iso);
  return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' }) +
    ' ' + d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });
}

export function RecentSessions({ sessions }: RecentSessionsProps) {
  if (sessions.length === 0) {
    return (
      <div
        className="rounded-xl border border-border/50 p-4"
        style={{ backgroundColor: 'oklch(0.14 0.01 270)' }}
      >
        <h3 className="mb-3 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
          Recent Sessions
        </h3>
        <p className="py-4 text-center text-xs text-muted-foreground">
          No sessions recorded yet
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
        Recent Sessions
      </h3>
      <div className="max-h-64 overflow-y-auto space-y-1.5">
        {sessions.map((session, i) => (
          <div
            key={i}
            className="flex items-center justify-between rounded-lg px-3 py-2"
            style={{ backgroundColor: 'oklch(0.18 0.01 270)' }}
          >
            <div className="min-w-0 flex-1">
              <div className="truncate text-xs font-medium">{session.exerciseId}</div>
              <div className="flex items-center gap-2 mt-0.5">
                <span className="flex items-center gap-0.5 text-[10px] text-muted-foreground">
                  <Clock className="h-2.5 w-2.5" />
                  {formatDuration(session.durationSeconds)}
                </span>
                <span className="text-[10px] text-muted-foreground">{formatTime(session.timestamp)}</span>
              </div>
            </div>
            <div className="flex items-center gap-2 shrink-0 ml-2">
              {session.accuracy != null && (
                <span className="text-xs tabular-nums text-muted-foreground">
                  {session.accuracy}%
                </span>
              )}
              {session.stars > 0 && (
                <div className="flex items-center gap-0.5">
                  {Array.from({ length: session.stars }, (_, j) => (
                    <Star key={j} className="h-3 w-3 fill-yellow-400 text-yellow-400" />
                  ))}
                </div>
              )}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
