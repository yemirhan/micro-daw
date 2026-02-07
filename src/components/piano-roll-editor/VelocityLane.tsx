import { useCallback, useRef, useState } from 'react';
import type { RegionNote } from '@/types/arrangement';
import { beatToGridX } from '@/utils/pianoRollHelpers';

interface VelocityLaneProps {
  notes: RegionNote[];
  selectedIndices: Set<number>;
  pxPerBeat: number;
  color: string;
  height: number;
  onVelocityChange: (noteIndex: number, velocity: number) => void;
}

export function VelocityLane({
  notes,
  selectedIndices,
  pxPerBeat,
  color,
  height,
  onVelocityChange,
}: VelocityLaneProps) {
  const dragging = useRef<number | null>(null);
  const laneRef = useRef<HTMLDivElement>(null);
  const [dragVelocity, setDragVelocity] = useState<{ index: number; value: number; x: number } | null>(null);

  const getVelocityFromY = useCallback((clientY: number) => {
    if (!laneRef.current) return 0.8;
    const rect = laneRef.current.getBoundingClientRect();
    const relY = clientY - rect.top;
    return Math.max(0.05, Math.min(1, 1 - relY / rect.height));
  }, []);

  const handlePointerDown = useCallback((e: React.PointerEvent, noteIndex: number) => {
    e.stopPropagation();
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
    dragging.current = noteIndex;
    const vel = getVelocityFromY(e.clientY);
    onVelocityChange(noteIndex, vel);
    setDragVelocity({ index: noteIndex, value: vel, x: e.clientX });
  }, [onVelocityChange, getVelocityFromY]);

  const handlePointerMove = useCallback((e: React.PointerEvent) => {
    if (dragging.current !== null) {
      const vel = getVelocityFromY(e.clientY);
      onVelocityChange(dragging.current, vel);
      setDragVelocity({ index: dragging.current, value: vel, x: e.clientX });
    }
  }, [onVelocityChange, getVelocityFromY]);

  const handlePointerUp = useCallback(() => {
    dragging.current = null;
    setDragVelocity(null);
  }, []);

  return (
    <div
      ref={laneRef}
      className="relative border-t border-border bg-background/30 select-none"
      style={{ height }}
      onPointerMove={handlePointerMove}
      onPointerUp={handlePointerUp}
    >
      {/* "Vel" label */}
      <span className="absolute top-1 left-1.5 text-[9px] font-medium text-muted-foreground/50 pointer-events-none z-10">
        Vel
      </span>

      {/* Guide lines with labels */}
      <div className="absolute inset-0 pointer-events-none">
        {[
          { pct: '25%', label: '75%' },
          { pct: '50%', label: '50%' },
          { pct: '75%', label: '25%' },
        ].map(({ pct, label }) => (
          <div key={pct} className="absolute w-full" style={{ top: pct }}>
            <div className="w-full border-b border-dashed border-border/30" />
            <span className="absolute right-1.5 -top-2.5 text-[8px] text-muted-foreground/40">
              {label}
            </span>
          </div>
        ))}
      </div>

      {/* Velocity bars */}
      {notes.map((note, i) => {
        const x = beatToGridX(note.startBeat, pxPerBeat);
        const barWidth = Math.max(beatToGridX(note.durationBeats, pxPerBeat), 3);
        const barHeight = note.velocity * height;
        const isSelected = selectedIndices.has(i);
        const isDragging = dragVelocity?.index === i;

        return (
          <div
            key={i}
            className="absolute bottom-0 cursor-ns-resize rounded-t-sm"
            style={{
              left: x,
              width: Math.min(barWidth, 12),
              height: barHeight,
              background: isSelected
                ? 'linear-gradient(to top, rgba(255,255,255,0.7), rgba(255,255,255,0.95))'
                : `linear-gradient(to top, ${color}B3, ${color}F0)`,
              opacity: isSelected ? 0.9 : 0.7,
            }}
            onPointerDown={(e) => handlePointerDown(e, i)}
          >
            {/* Top highlight */}
            <div
              className="absolute top-0 left-0 right-0 h-px rounded-t-sm"
              style={{ backgroundColor: isSelected ? 'rgba(255,255,255,0.5)' : 'rgba(255,255,255,0.2)' }}
            />
          </div>
        );
      })}

      {/* Drag tooltip */}
      {dragVelocity && laneRef.current && (() => {
        const rect = laneRef.current.getBoundingClientRect();
        const tooltipX = dragVelocity.x - rect.left;
        const tooltipY = (1 - dragVelocity.value) * height - 18;
        return (
          <div
            className="absolute pointer-events-none z-20 bg-popover text-popover-foreground text-[9px] font-mono px-1.5 py-0.5 rounded shadow-sm border border-border/60 whitespace-nowrap"
            style={{
              left: tooltipX,
              top: Math.max(0, tooltipY),
              transform: 'translateX(-50%)',
            }}
          >
            {dragVelocity.value.toFixed(2)}
          </div>
        );
      })()}
    </div>
  );
}
