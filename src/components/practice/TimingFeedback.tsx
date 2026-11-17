import { cn } from '@/lib/utils';
import { ACCEPTABLE_WINDOW_MS, GOOD_WINDOW_MS, PERFECT_WINDOW_MS } from '@/utils/rhythmScoring';
import type { RhythmFeedback } from '@/types/appMode';

const TIMING_STYLE: Record<RhythmFeedback['timing'], { label: string; text: string; dot: string }> = {
  perfect: { label: 'Perfect', text: 'text-note-correct', dot: 'bg-note-correct' },
  good: { label: 'Good', text: 'text-note-correct', dot: 'bg-note-correct/70' },
  early: { label: 'Early', text: 'text-signal-warn', dot: 'bg-signal-warn' },
  late: { label: 'Late', text: 'text-signal-warn', dot: 'bg-signal-warn' },
  miss: { label: 'Miss', text: 'text-note-wrong', dot: 'bg-note-wrong' },
};

const RANGE_MS = ACCEPTABLE_WINDOW_MS + 30;
const pos = (ms: number) => `${50 + (Math.max(-RANGE_MS, Math.min(RANGE_MS, ms)) / RANGE_MS) * 50}%`;

/**
 * Last judgement in big type plus a timing lane: center = on the beat,
 * left = early, right = late. The last hits stay visible as fading dots.
 */
export function TimingFeedback({ history, isRunning }: { history: RhythmFeedback[]; isRunning: boolean }) {
  const last = history[history.length - 1];
  const recent = history.filter((f) => f.timing !== 'miss').slice(-8);
  const style = last ? TIMING_STYLE[last.timing] : null;

  return (
    <div className="flex items-center gap-6">
      <div className="w-36 shrink-0" role="status" aria-live="polite">
        {last && style ? (
          <div key={history.length} className="animate-in fade-in-0 zoom-in-95 duration-150">
            <div className={cn('text-2xl font-semibold tracking-tight', style.text)}>{style.label}</div>
            <div className="font-mono text-xs tabular-nums text-muted-foreground">
              {last.timing === 'miss'
                ? 'no hit on that beat'
                : `${Math.abs(Math.round(last.offsetMs))} ms ${last.offsetMs < 0 ? 'early' : 'late'}`}
            </div>
          </div>
        ) : (
          <div>
            <div className="text-2xl font-semibold tracking-tight text-muted-foreground/50">--</div>
            <div className="text-xs text-muted-foreground">{isRunning ? 'Play on the beat' : 'Press Start'}</div>
          </div>
        )}
      </div>

      <div className="flex-1">
        <div className="relative h-10 rounded-lg border border-border bg-surface-0">
          {/* windows */}
          <div
            className="absolute inset-y-1.5 rounded bg-signal-warn/8"
            style={{ left: pos(-ACCEPTABLE_WINDOW_MS), right: `calc(100% - ${pos(ACCEPTABLE_WINDOW_MS)})` }}
            aria-hidden
          />
          <div
            className="absolute inset-y-1.5 rounded bg-note-correct/10"
            style={{ left: pos(-GOOD_WINDOW_MS), right: `calc(100% - ${pos(GOOD_WINDOW_MS)})` }}
            aria-hidden
          />
          <div
            className="absolute inset-y-1.5 rounded bg-note-correct/18"
            style={{ left: pos(-PERFECT_WINDOW_MS), right: `calc(100% - ${pos(PERFECT_WINDOW_MS)})` }}
            aria-hidden
          />
          <div className="absolute inset-y-0 left-1/2 w-px bg-foreground/40" aria-hidden />
          {recent.map((f, i) => (
            <span
              key={`${f.beat}-${i}-${history.length}`}
              className={cn('absolute top-1/2 size-2.5 -translate-x-1/2 -translate-y-1/2 rounded-full ring-2 ring-surface-0', TIMING_STYLE[f.timing].dot)}
              style={{ left: pos(f.offsetMs), opacity: 0.25 + ((i + 1) / recent.length) * 0.75 }}
              aria-hidden
            />
          ))}
        </div>
        <div className="mt-1 flex justify-between font-mono text-[10px] text-muted-foreground">
          <span>early</span>
          <span>on the beat</span>
          <span>late</span>
        </div>
      </div>
    </div>
  );
}
