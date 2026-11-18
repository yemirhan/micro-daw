import { beatToPx } from '@/utils/arrangementHelpers';
import { cn } from '@/lib/utils';
import { usePlaybackPosition } from '@/hooks/useArrangement';

interface PlayheadProps {
  position: number;
  pxPerBeat: number;
  height: number | string;
  /** Stopped: draw as a quieter cue line. */
  idle?: boolean;
  /** Recording: red instead of the accent. */
  recording?: boolean;
}

export function Playhead({ position, pxPerBeat, height, idle, recording }: PlayheadProps) {
  const x = beatToPx(position, pxPerBeat);
  const color = recording ? 'var(--signal-rec)' : 'var(--primary)';

  return (
    <div
      className={cn('pointer-events-none absolute top-0 left-0 z-30 w-px will-change-transform', idle && 'opacity-55')}
      style={{ transform: `translateX(${x}px)`, height, backgroundColor: color }}
      aria-hidden
    >
      {/* Head on the ruler */}
      <svg width="11" height="8" viewBox="0 0 11 8" className="absolute top-0 -left-[5px]">
        <path d="M0 0 H11 V3 L5.5 8 L0 3 Z" fill={color} />
      </svg>
    </div>
  );
}

/** Playhead fed by the engine (re-renders per frame by itself; its host does not). */
export function LivePlayhead(props: Omit<PlayheadProps, 'position'>) {
  return <Playhead position={usePlaybackPosition()} {...props} />;
}
