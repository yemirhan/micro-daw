import { useCallback, useRef } from 'react';
import { beatToPx, pxToBeat, snapBeat } from '@/utils/arrangementHelpers';
import type { Marker } from '@/types/arrangement';

interface MarkerFlagProps {
  marker: Marker;
  pxPerBeat: number;
  snapValue: number;
  lengthBeats: number;
  onClick: (id: string) => void;
  onContextMenu: (e: React.MouseEvent, id: string) => void;
  onDragEnd: (id: string, newBeat: number) => void;
}

export function MarkerFlag({
  marker,
  pxPerBeat,
  snapValue,
  lengthBeats,
  onClick,
  onContextMenu,
  onDragEnd,
}: MarkerFlagProps) {
  const dragging = useRef(false);

  const handlePointerDown = useCallback(
    (e: React.PointerEvent) => {
      if (e.button !== 0) return;
      e.stopPropagation();
      e.preventDefault();
      dragging.current = false;

      const el = e.currentTarget as HTMLElement;
      const container = el.parentElement!;
      el.setPointerCapture(e.pointerId);

      const onMove = (ev: PointerEvent) => {
        dragging.current = true;
        const rect = container.getBoundingClientRect();
        const x = ev.clientX - rect.left;
        const beat = snapBeat(pxToBeat(x, pxPerBeat), snapValue);
        const clamped = Math.max(0, Math.min(lengthBeats, beat));
        el.style.left = `${beatToPx(clamped, pxPerBeat)}px`;
      };

      const onUp = (ev: PointerEvent) => {
        if (dragging.current) {
          const rect = container.getBoundingClientRect();
          const x = ev.clientX - rect.left;
          const beat = snapBeat(pxToBeat(x, pxPerBeat), snapValue);
          const clamped = Math.max(0, Math.min(lengthBeats, beat));
          onDragEnd(marker.id, clamped);
        } else {
          onClick(marker.id);
        }
        window.removeEventListener('pointermove', onMove);
        window.removeEventListener('pointerup', onUp);
      };

      window.addEventListener('pointermove', onMove);
      window.addEventListener('pointerup', onUp);
    },
    [marker.id, pxPerBeat, snapValue, lengthBeats, onClick, onDragEnd],
  );

  return (
    <div
      className="absolute top-0 z-20 cursor-grab active:cursor-grabbing"
      style={{ left: beatToPx(marker.beat, pxPerBeat) }}
      onPointerDown={handlePointerDown}
      onClick={(e) => e.stopPropagation()}
      onContextMenu={(e) => {
        e.preventDefault();
        e.stopPropagation();
        onContextMenu(e, marker.id);
      }}
      title={`${marker.name}: click to jump, drag to move, right-click to edit`}
    >
      <div className="relative" style={{ marginLeft: -0.5 }}>
        <div className="h-7 w-px" style={{ backgroundColor: marker.color }} />
        <div
          className="absolute top-[2px] left-0 max-w-[96px] truncate rounded-r-[3px] py-[2px] pr-1.5 pl-1 text-[9.5px] leading-none font-bold whitespace-nowrap text-black/85 select-none"
          style={{ backgroundColor: marker.color }}
        >
          {marker.name}
        </div>
      </div>
    </div>
  );
}
