import { useCallback, useRef } from 'react';
import { beatToPx, pxToBeat, snapBeat } from '@/utils/arrangementHelpers';
import { MarkerFlag } from './MarkerFlag';
import type { LoopMarkers, Marker } from '@/types/arrangement';

interface TimelineRulerProps {
  lengthBeats: number;
  pxPerBeat: number;
  snapValue: number;
  loopEnabled?: boolean;
  loopMarkers?: LoopMarkers;
  markers?: Marker[];
  onSeek: (beat: number) => void;
  onLoopMarkersChange?: (startBeat: number, endBeat: number) => void;
  onMarkerClick?: (id: string) => void;
  onMarkerContextMenu?: (e: React.MouseEvent, id: string) => void;
  onMarkerDragEnd?: (id: string, newBeat: number) => void;
}

export function TimelineRuler({
  lengthBeats,
  pxPerBeat,
  snapValue,
  loopEnabled,
  loopMarkers,
  markers: timelineMarkers,
  onSeek,
  onLoopMarkersChange,
  onMarkerClick,
  onMarkerContextMenu,
  onMarkerDragEnd,
}: TimelineRulerProps) {
  const totalWidth = beatToPx(lengthBeats, pxPerBeat);
  const draggingRef = useRef<'start' | 'end' | null>(null);

  const markers: Array<{ beat: number; isBar: boolean; label: string }> = [];
  for (let beat = 0; beat <= lengthBeats; beat++) {
    if (beat % 4 === 0) {
      markers.push({ beat, isBar: true, label: `${beat / 4 + 1}` });
    } else if (pxPerBeat >= 20) {
      markers.push({ beat, isBar: false, label: '' });
    }
  }

  const handleClick = (e: React.MouseEvent) => {
    if (draggingRef.current) return;
    const rect = e.currentTarget.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const beat = snapBeat(pxToBeat(x, pxPerBeat), snapValue);
    onSeek(Math.max(0, beat));
  };

  const handleMarkerDrag = useCallback(
    (edge: 'start' | 'end', e: React.PointerEvent) => {
      if (!onLoopMarkersChange || !loopMarkers) return;
      e.stopPropagation();
      e.preventDefault();
      draggingRef.current = edge;
      const container = (e.currentTarget as HTMLElement).parentElement!;
      const pointerId = e.pointerId;
      (e.currentTarget as HTMLElement).setPointerCapture(pointerId);

      const onMove = (ev: PointerEvent) => {
        const rect = container.getBoundingClientRect();
        const x = ev.clientX - rect.left;
        const beat = snapBeat(pxToBeat(x, pxPerBeat), snapValue);
        const clamped = Math.max(0, Math.min(lengthBeats, beat));

        if (edge === 'start') {
          onLoopMarkersChange(Math.min(clamped, loopMarkers.endBeat - 1), loopMarkers.endBeat);
        } else {
          onLoopMarkersChange(loopMarkers.startBeat, Math.max(clamped, loopMarkers.startBeat + 1));
        }
      };

      const onUp = () => {
        draggingRef.current = null;
        window.removeEventListener('pointermove', onMove);
        window.removeEventListener('pointerup', onUp);
      };

      window.addEventListener('pointermove', onMove);
      window.addEventListener('pointerup', onUp);
    },
    [onLoopMarkersChange, loopMarkers, pxPerBeat, snapValue, lengthBeats],
  );

  return (
    <div
      className="relative h-6 cursor-pointer border-b border-border bg-card/80"
      style={{ width: totalWidth }}
      onClick={handleClick}
    >
      {/* Loop region shading */}
      {loopEnabled && loopMarkers && (
        <>
          <div
            className="absolute top-0 h-full pointer-events-none"
            style={{
              left: beatToPx(loopMarkers.startBeat, pxPerBeat),
              width: beatToPx(loopMarkers.endBeat - loopMarkers.startBeat, pxPerBeat),
              backgroundColor: 'oklch(0.65 0.20 265 / 0.12)',
              borderLeft: '2px solid oklch(0.65 0.20 265 / 0.6)',
              borderRight: '2px solid oklch(0.65 0.20 265 / 0.6)',
            }}
          />
          {/* Start handle */}
          <div
            className="absolute top-0 h-full w-2 cursor-col-resize z-10"
            style={{ left: beatToPx(loopMarkers.startBeat, pxPerBeat) - 4 }}
            onPointerDown={(e) => handleMarkerDrag('start', e)}
          >
            <div
              className="absolute left-1 top-0 w-0 h-0"
              style={{
                borderLeft: '4px solid transparent',
                borderRight: '4px solid transparent',
                borderTop: '6px solid oklch(0.65 0.20 265)',
              }}
            />
          </div>
          {/* End handle */}
          <div
            className="absolute top-0 h-full w-2 cursor-col-resize z-10"
            style={{ left: beatToPx(loopMarkers.endBeat, pxPerBeat) - 4 }}
            onPointerDown={(e) => handleMarkerDrag('end', e)}
          >
            <div
              className="absolute left-1 top-0 w-0 h-0"
              style={{
                borderLeft: '4px solid transparent',
                borderRight: '4px solid transparent',
                borderTop: '6px solid oklch(0.65 0.20 265)',
              }}
            />
          </div>
        </>
      )}

      {markers.map((m) => (
        <div
          key={m.beat}
          className="absolute top-0"
          style={{ left: beatToPx(m.beat, pxPerBeat) }}
        >
          <div
            className={`w-px ${m.isBar ? 'h-6 bg-border' : 'h-3 bg-border/50'}`}
          />
          {m.isBar && (
            <span className="absolute left-1 top-0 select-none font-mono text-[10px] text-muted-foreground">
              {m.label}
            </span>
          )}
        </div>
      ))}

      {/* Timeline markers */}
      {timelineMarkers && onMarkerClick && onMarkerContextMenu && onMarkerDragEnd &&
        timelineMarkers.map((m) => (
          <MarkerFlag
            key={m.id}
            marker={m}
            pxPerBeat={pxPerBeat}
            snapValue={snapValue}
            lengthBeats={lengthBeats}
            onClick={onMarkerClick}
            onContextMenu={onMarkerContextMenu}
            onDragEnd={onMarkerDragEnd}
          />
        ))
      }
    </div>
  );
}
