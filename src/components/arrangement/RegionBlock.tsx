import { useCallback, useRef, useState } from 'react';
import type { Region, RegionNote } from '@/types/arrangement';
import type { ArrangementTool } from './ArrangementToolbar';
import { beatToPx, pxToBeat, snapBeat } from '@/utils/arrangementHelpers';
import { PIANO_START, PIANO_END } from '@/utils/constants';
import { WaveformMinimap } from './WaveformMinimap';

type DragMode = 'move' | 'resize-left' | 'resize-right' | null;

interface RegionBlockProps {
  region: Region;
  trackColor: string;
  pxPerBeat: number;
  snapValue: number;
  tool: ArrangementTool;
  selected?: boolean;
  bpm?: number;
  onMove: (regionId: string, newStartBeat: number) => void;
  onResize?: (regionId: string, newStartBeat: number, newLengthBeats: number) => void;
  onCut?: (regionId: string, splitBeat: number) => void;
  onSelect?: (regionId: string, addToSelection: boolean) => void;
  onContextMenu: (e: React.MouseEvent, regionId: string) => void;
  onDoubleClick?: (regionId: string) => void;
}

function NoteMinimap({ notes, lengthBeats, isDrum }: { notes: RegionNote[]; lengthBeats: number; isDrum: boolean }) {
  if (notes.length === 0) return null;

  const minNote = isDrum ? 36 : Math.min(...notes.map((n) => n.note), PIANO_START);
  const maxNote = isDrum ? 43 : Math.max(...notes.map((n) => n.note), PIANO_START + 12);
  const range = Math.max(maxNote - minNote + 1, 4);

  return (
    <div className="absolute inset-0.5 overflow-hidden">
      {notes.map((note, i) => {
        const left = `${(note.startBeat / lengthBeats) * 100}%`;
        const width = `${Math.max((note.durationBeats / lengthBeats) * 100, 1)}%`;
        const bottom = `${((note.note - minNote) / range) * 100}%`;
        const height = `${Math.max(100 / range, 4)}%`;

        return (
          <div
            key={i}
            className="absolute rounded-[1px] bg-white/60"
            style={{ left, width, bottom, height: Math.max(2, parseFloat(height)) }}
          />
        );
      })}
    </div>
  );
}

export function RegionBlock({
  region,
  trackColor,
  pxPerBeat,
  snapValue,
  selected,
  bpm,
  onMove,
  onResize,
  onCut,
  onSelect,
  onContextMenu,
  onDoubleClick,
  tool,
}: RegionBlockProps) {
  const [dragMode, setDragMode] = useState<DragMode>(null);
  const dragStart = useRef({ x: 0, startBeat: 0, lengthBeats: 0 });

  const left = beatToPx(region.startBeat, pxPerBeat);
  const width = beatToPx(region.lengthBeats, pxPerBeat);
  const color = region.color || trackColor;
  const isDrum = region.notes.some((n) => n.isDrum);
  const HANDLE_WIDTH = 6;

  const getEdge = (e: React.PointerEvent): DragMode => {
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const localX = e.clientX - rect.left;
    if (localX < HANDLE_WIDTH && onResize) return 'resize-left';
    if (localX > rect.width - HANDLE_WIDTH && onResize) return 'resize-right';
    return 'move';
  };

  const handlePointerDown = useCallback((e: React.PointerEvent) => {
    if (e.button !== 0) return;
    e.stopPropagation();

    // Cut tool — split at click position
    if (tool === 'cut' && onCut) {
      const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
      const localX = e.clientX - rect.left;
      const localBeat = pxToBeat(localX, pxPerBeat);
      const snappedBeat = snapBeat(localBeat, snapValue);
      const absoluteBeat = region.startBeat + snappedBeat;
      onCut(region.id, absoluteBeat);
      return;
    }

    // Select on click
    onSelect?.(region.id, e.shiftKey);

    e.currentTarget.setPointerCapture(e.pointerId);
    const mode = getEdge(e);
    dragStart.current = { x: e.clientX, startBeat: region.startBeat, lengthBeats: region.lengthBeats };
    setDragMode(mode);
  }, [region.startBeat, region.lengthBeats, onResize, tool, onCut, onSelect, pxPerBeat, snapValue]);

  const handlePointerMove = useCallback((e: React.PointerEvent) => {
    if (!dragMode) {
      const el = e.currentTarget as HTMLElement;
      if (tool === 'cut') {
        el.style.cursor = 'crosshair';
      } else {
        const edge = getEdge(e);
        if (edge === 'resize-left' || edge === 'resize-right') {
          el.style.cursor = 'col-resize';
        } else {
          el.style.cursor = 'grab';
        }
      }
      return;
    }

    const dx = e.clientX - dragStart.current.x;
    const deltaBeat = pxToBeat(dx, pxPerBeat);

    if (dragMode === 'move') {
      const newBeat = snapBeat(dragStart.current.startBeat + deltaBeat, snapValue);
      onMove(region.id, Math.max(0, newBeat));
    } else if (dragMode === 'resize-right' && onResize) {
      const newLength = snapBeat(dragStart.current.lengthBeats + deltaBeat, snapValue);
      onResize(region.id, region.startBeat, Math.max(snapValue || 0.25, newLength));
    } else if (dragMode === 'resize-left' && onResize) {
      const newStart = snapBeat(dragStart.current.startBeat + deltaBeat, snapValue);
      const clampedStart = Math.max(0, newStart);
      const diff = dragStart.current.startBeat - clampedStart;
      const newLength = dragStart.current.lengthBeats + diff;
      if (newLength >= (snapValue || 0.25)) {
        onResize(region.id, clampedStart, newLength);
      }
    }
  }, [dragMode, pxPerBeat, snapValue, onMove, onResize, region.id, region.startBeat]);

  const handlePointerUp = useCallback(() => {
    setDragMode(null);
  }, []);

  return (
    <div
      className="absolute top-0.5 bottom-0.5 select-none rounded-md transition-[filter] hover:brightness-110"
      style={{
        left,
        width: Math.max(width, 4),
        background: `linear-gradient(to bottom, ${color}, color-mix(in oklch, ${color} 80%, black))`,
        opacity: dragMode ? 0.7 : 1,
        border: selected ? '2px solid white' : '1px solid rgba(255,255,255,0.2)',
        boxShadow: selected ? '0 0 8px rgba(255,255,255,0.3)' : undefined,
      }}
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={handlePointerUp}
      onDoubleClick={() => {
        // Audio regions have no piano roll editor
        if (!region.audio) onDoubleClick?.(region.id);
      }}
      onContextMenu={(e) => {
        e.preventDefault();
        onContextMenu(e, region.id);
      }}
    >
      {region.name && (
        <span className="absolute left-1 top-0 truncate text-[9px] font-medium text-white/80 z-10">
          {region.name}
        </span>
      )}
      {region.audio && bpm ? (
        <WaveformMinimap
          sampleId={region.audio.sampleId}
          lengthBeats={region.lengthBeats}
          offsetSeconds={region.audio.offsetSeconds}
          pxPerBeat={pxPerBeat}
          color={color}
          bpm={bpm}
        />
      ) : (
        <NoteMinimap notes={region.notes} lengthBeats={region.lengthBeats} isDrum={isDrum} />
      )}
    </div>
  );
}
