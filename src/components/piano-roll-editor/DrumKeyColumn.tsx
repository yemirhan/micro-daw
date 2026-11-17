import { useLayoutEffect, useRef, useState } from 'react';
import type { PianoRollViewport, RowLayout } from '@/types/pianoRollEditor';
import { cn } from '@/lib/utils';
import { KEY_COLUMN_WIDTH } from './PianoKeyColumn';

interface DrumKeyColumnProps {
  viewport: PianoRollViewport;
  layout: RowLayout;
  hoverRow: number | null;
  litPitches: ReadonlySet<number>;
  onPreview: (pitch: number) => void;
  onWheel: (e: WheelEvent) => void;
}

/** Pad names for drum regions. Click a pad to audition it. */
export function DrumKeyColumn({ viewport, layout, hoverRow, litPitches, onPreview, onWheel }: DrumKeyColumnProps) {
  const wrapRef = useRef<HTMLDivElement>(null);
  const [pressed, setPressed] = useState<number | null>(null);
  const { rowHeight, scrollTop, height } = viewport;

  useLayoutEffect(() => {
    const el = wrapRef.current;
    if (!el) return;
    el.addEventListener('wheel', onWheel, { passive: false });
    return () => el.removeEventListener('wheel', onWheel);
  }, [onWheel]);

  const rows = Array.from({ length: layout.rowCount }, (_, row) => row);

  return (
    <div
      ref={wrapRef}
      className="relative shrink-0 select-none overflow-hidden border-r border-border bg-card"
      style={{ width: KEY_COLUMN_WIDTH, height }}
    >
      <div style={{ transform: `translateY(${-scrollTop}px)` }}>
        {rows.map((row) => {
          const pitch = layout.rowToPitch(row);
          const color = layout.rowColor?.(row);
          const lit = litPitches.has(pitch) || pressed === pitch;
          return (
            <button
              key={pitch}
              type="button"
              tabIndex={-1}
              className={cn(
                'flex w-full items-center gap-1.5 border-b border-border/60 px-2 text-left font-mono text-[10px] font-semibold transition-colors',
                row % 2 === 1 ? 'bg-background/60' : 'bg-card',
                hoverRow === row && 'bg-accent',
                lit && 'bg-primary/25 text-foreground',
                !lit && 'text-muted-foreground hover:text-foreground',
              )}
              style={{ height: rowHeight }}
              title={`${layout.rowLabel(row)} (MIDI ${pitch})`}
              onPointerDown={(e) => {
                if (e.button !== 0) return;
                setPressed(pitch);
                onPreview(pitch);
              }}
              onPointerUp={() => setPressed(null)}
              onPointerLeave={() => setPressed(null)}
            >
              <span
                className="h-2 w-2 shrink-0 rounded-full"
                style={{ backgroundColor: color ?? 'var(--muted-foreground)' }}
              />
              {rowHeight >= 12 && <span className="truncate">{layout.rowLabel(row)}</span>}
            </button>
          );
        })}
      </div>
    </div>
  );
}
