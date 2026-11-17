import { useCallback, useRef, useState } from 'react';
import type { Region, RegionNote } from '@/types/arrangement';
import type { ArrangementTool } from './ArrangementToolbar';
import { beatToPx, pxToBeat, snapBeat } from '@/utils/arrangementHelpers';
import { cn } from '@/lib/utils';
import { WaveformMinimap } from './WaveformMinimap';

type DragMode = 'move' | 'resize-left' | 'resize-right' | null;

interface RegionBlockProps {
  region: Region;
  trackColor: string;
  pxPerBeat: number;
  snapValue: number;
  tool: ArrangementTool;
  selected?: boolean;
  /** Track is muted (region drawn dimmed). */
  muted?: boolean;
  bpm?: number;
  onMove: (regionId: string, newStartBeat: number) => void;
  onResize?: (regionId: string, newStartBeat: number, newLengthBeats: number) => void;
  onCut?: (regionId: string, splitBeat: number) => void;
  onSelect?: (regionId: string, addToSelection: boolean) => void;
  onContextMenu: (e: React.MouseEvent, regionId: string) => void;
  onDoubleClick?: (regionId: string) => void;
}

/** Mini piano-roll preview. Pitch range fits the notes (drums use GM note rows). */
export function NoteMinimap({ notes, lengthBeats, isDrum, color }: { notes: RegionNote[]; lengthBeats: number; isDrum: boolean; color: string }) {
  if (notes.length === 0 || lengthBeats <= 0) return null;

  let minNote = Infinity;
  let maxNote = -Infinity;
  for (const n of notes) {
    if (n.note < minNote) minNote = n.note;
    if (n.note > maxNote) maxNote = n.note;
  }
  // A little headroom so single-pitch lines don't hug the edges.
  const pad = isDrum ? 0 : 2;
  minNote = Math.max(0, minNote - pad);
  maxNote = Math.min(127, maxNote + pad);
  const range = Math.max(maxNote - minNote + 1, isDrum ? 4 : 8);
  const noteColor = `color-mix(in oklab, ${color} 55%, white)`;

  return (
    <div className="absolute inset-x-1 top-1 bottom-1 overflow-hidden">
      {notes.map((note, i) => {
        const left = (note.startBeat / lengthBeats) * 100;
        const width = Math.max((note.durationBeats / lengthBeats) * 100, isDrum ? 0.6 : 0.8);
        const bottom = ((note.note - minNote) / range) * 100;
        const height = Math.min(100 / range, 18);
        return (
          <div
            key={i}
            className="absolute rounded-[1px]"
            style={{
              left: `${left}%`,
              width: `${width}%`,
              bottom: `${bottom}%`,
              height: `max(2px, ${height}%)`,
              backgroundColor: noteColor,
              opacity: 0.55 + 0.45 * Math.min(1, note.velocity > 1 ? note.velocity / 127 : note.velocity),
            }}
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
  muted,
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

  const showHeader = width >= 18;

  return (
    <div
      className={cn(
        'group/region absolute top-[3px] bottom-[3px] flex select-none flex-col overflow-hidden rounded-[5px]',
        'transition-[filter,opacity] duration-100 hover:brightness-[1.12]',
        selected ? 'z-[2]' : 'z-[1]',
      )}
      style={{
        left,
        width: Math.max(width, 4),
        backgroundColor: `color-mix(in oklab, ${color} ${selected ? 34 : 24}%, var(--surface-1))`,
        boxShadow: selected
          ? '0 0 0 1.5px var(--signal-select), 0 2px 6px oklch(0 0 0 / 0.35)'
          : `inset 0 0 0 1px color-mix(in oklab, ${color} 55%, transparent), 0 1px 2px oklch(0 0 0 / 0.3)`,
        opacity: dragMode ? 0.75 : muted ? 0.45 : 1,
        filter: muted ? 'saturate(0.3)' : undefined,
      }}
      data-region-id={region.id}
      data-selected={selected || undefined}
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
      title={region.name || undefined}
    >
      {/* Header strip */}
      {showHeader && (
        <div
          className="flex h-[15px] shrink-0 items-center px-1.5"
          style={{ backgroundColor: `color-mix(in oklab, ${color} ${selected ? 100 : 86}%, var(--surface-1))` }}
        >
          <span className="truncate text-[10px] font-semibold leading-none text-black/80">
            {region.name || (region.audio ? 'Audio' : 'Clip')}
          </span>
        </div>
      )}

      {/* Body */}
      <div className="relative min-h-0 flex-1">
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
          <NoteMinimap notes={region.notes} lengthBeats={region.lengthBeats} isDrum={isDrum} color={color} />
        )}
      </div>

      {/* Resize affordances */}
      {onResize && width >= 24 && (
        <>
          <span aria-hidden className="pointer-events-none absolute inset-y-1 left-0.5 w-[2px] rounded-full bg-white/0 transition-colors group-hover/region:bg-white/25" />
          <span aria-hidden className="pointer-events-none absolute inset-y-1 right-0.5 w-[2px] rounded-full bg-white/0 transition-colors group-hover/region:bg-white/25" />
        </>
      )}
    </div>
  );
}
