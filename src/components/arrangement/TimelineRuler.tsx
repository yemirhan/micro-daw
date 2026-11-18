import { useCallback, useRef } from 'react';
import { beatToPx, pxToBeat, snapBeat } from '@/utils/arrangementHelpers';
import { MarkerFlag } from './MarkerFlag';
import type { LoopMarkers, Marker } from '@/types/arrangement';
import { arrangementEngine } from '@/services/ArrangementEngine';

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

  // Label every N bars so labels stay >= ~32px apart when zoomed out.
  const barPx = pxPerBeat * 4;
  const labelEvery = [1, 2, 4, 8, 16, 32].find((n) => n * barPx >= 32) ?? 64;
  const markers: Array<{ beat: number; isBar: boolean; label: string }> = [];
  for (let beat = 0; beat <= lengthBeats; beat++) {
    if (beat % 4 === 0) {
      const bar = beat / 4;
      markers.push({ beat, isBar: true, label: bar % labelEvery === 0 ? `${bar + 1}` : '' });
    } else if (pxPerBeat >= 14) {
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
      // The whole handle drag is one undo step.
      arrangementEngine.beginUndoGroup('Set Loop Markers');
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
        arrangementEngine.endUndoGroup();
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
      className="relative h-7 cursor-pointer border-b border-border bg-surface-1"
      style={{ width: totalWidth }}
      onClick={handleClick}
      title="Click to move the playhead"
    >
      {/* Loop region */}
      {loopMarkers && (
        <>
          <div
            className={`pointer-events-none absolute inset-y-0 ${loopEnabled ? 'bg-primary/14' : 'bg-foreground/[0.04]'}`}
            style={{
              left: beatToPx(loopMarkers.startBeat, pxPerBeat),
              width: beatToPx(loopMarkers.endBeat - loopMarkers.startBeat, pxPerBeat),
            }}
          >
            <div className={`absolute inset-x-0 top-0 h-[3px] ${loopEnabled ? 'bg-primary' : 'bg-muted-foreground/40'}`} />
          </div>
          {loopEnabled && (
            <>
              {/* Start handle */}
              <div
                className="absolute top-0 z-10 h-full w-2.5 cursor-col-resize"
                style={{ left: beatToPx(loopMarkers.startBeat, pxPerBeat) - 1 }}
                onPointerDown={(e) => handleMarkerDrag('start', e)}
                onClick={(e) => e.stopPropagation()}
                title="Drag loop start"
              >
                <div className="absolute top-0 left-0 h-full w-[2px] bg-primary" />
                <div className="absolute top-0 left-0 h-2 w-2 bg-primary [clip-path:polygon(0_0,100%_0,0_100%)]" />
              </div>
              {/* End handle */}
              <div
                className="absolute top-0 z-10 h-full w-2.5 cursor-col-resize"
                style={{ left: beatToPx(loopMarkers.endBeat, pxPerBeat) - 9 }}
                onPointerDown={(e) => handleMarkerDrag('end', e)}
                onClick={(e) => e.stopPropagation()}
                title="Drag loop end"
              >
                <div className="absolute top-0 right-0 h-full w-[2px] bg-primary" />
                <div className="absolute top-0 right-0 h-2 w-2 bg-primary [clip-path:polygon(0_0,100%_0,100%_100%)]" />
              </div>
            </>
          )}
        </>
      )}

      {markers.map((m) => (
        <div
          key={m.beat}
          className="pointer-events-none absolute bottom-0"
          style={{ left: beatToPx(m.beat, pxPerBeat) }}
        >
          <div className={m.isBar ? 'h-[12px] w-px bg-muted-foreground/45' : 'h-[5px] w-px bg-muted-foreground/25'} />
          {m.label && (
            <span className="absolute bottom-[2px] left-[4px] select-none font-mono text-[10px] leading-none text-muted-foreground tabular-nums">
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
