import React, { useCallback, useRef, useState, useMemo, useEffect } from 'react';
import type { RegionNote } from '@/types/arrangement';
import type { PianoRollTool, DragState } from '@/types/pianoRollEditor';
import { NoteRect } from './NoteRect';
import { isBlackKey } from '@/utils/noteHelpers';
import {
  PR_NOTE_MIN,
  PR_NOTE_MAX,
  DRUM_ROWS,
  beatToGridX,
  gridXToBeat,
  noteToGridY,
  gridYToNote,
  drumRowToGridY,
  gridYToDrumRow,
  drumRowToMidi,
  drumMidiToRow,
  snapBeatPR,
  DEFAULT_NOTE_VELOCITY,
  DEFAULT_NOTE_DURATION,
  DEFAULT_DRUM_DURATION,
} from '@/utils/pianoRollHelpers';

interface NoteGridProps {
  notes: RegionNote[];
  selectedIndices: Set<number>;
  regionLengthBeats: number;
  pxPerBeat: number;
  rowHeight: number;
  snapValue: number;
  tool: PianoRollTool;
  isDrum: boolean;
  trackColor: string;
  onNotesChange: (notes: RegionNote[]) => void;
  onSelectionChange: (indices: Set<number>) => void;
}

export function NoteGrid({
  notes,
  selectedIndices,
  regionLengthBeats,
  pxPerBeat,
  rowHeight,
  snapValue,
  tool,
  isDrum,
  trackColor,
  onNotesChange,
  onSelectionChange,
}: NoteGridProps) {
  const gridRef = useRef<HTMLDivElement>(null);
  const [dragState, setDragState] = useState<DragState | null>(null);

  const totalRows = isDrum ? DRUM_ROWS.length : PR_NOTE_MAX - PR_NOTE_MIN;
  const gridWidth = beatToGridX(regionLengthBeats, pxPerBeat);
  const gridHeight = totalRows * rowHeight;

  // Map from grid-local coordinates to beat/note
  const getGridPosition = useCallback(
    (clientX: number, clientY: number) => {
      if (!gridRef.current) return { beat: 0, note: 0, row: 0 };
      const rect = gridRef.current.getBoundingClientRect();
      const x = clientX - rect.left + gridRef.current.scrollLeft;
      const y = clientY - rect.top + gridRef.current.scrollTop;
      const beat = gridXToBeat(x, pxPerBeat);
      if (isDrum) {
        const row = gridYToDrumRow(y, rowHeight);
        return { beat, note: drumRowToMidi(row), row };
      }
      return { beat, note: gridYToNote(y, rowHeight), row: 0 };
    },
    [pxPerBeat, rowHeight, isDrum],
  );

  // Find which note index is at a position
  const hitTestNote = useCallback(
    (clientX: number, clientY: number): { index: number; edge: 'body' | 'right' | 'left' } | null => {
      const { beat, note } = getGridPosition(clientX, clientY);
      for (let i = notes.length - 1; i >= 0; i--) {
        const n = notes[i];
        if (n.note !== note) continue;
        if (beat >= n.startBeat && beat <= n.startBeat + n.durationBeats) {
          if (!gridRef.current) return { index: i, edge: 'body' };
          const rect = gridRef.current.getBoundingClientRect();
          const mouseX = clientX - rect.left + gridRef.current.scrollLeft;
          const leftEdgeX = beatToGridX(n.startBeat, pxPerBeat);
          const rightEdgeX = beatToGridX(n.startBeat + n.durationBeats, pxPerBeat);
          // Check if near right edge (within 6px)
          if (rightEdgeX - mouseX < 6 && rightEdgeX - mouseX >= 0) {
            return { index: i, edge: 'right' };
          }
          // Check if near left edge (within 6px)
          if (mouseX - leftEdgeX < 6 && mouseX - leftEdgeX >= 0) {
            return { index: i, edge: 'left' };
          }
          return { index: i, edge: 'body' };
        }
      }
      return null;
    },
    [notes, getGridPosition, isDrum, pxPerBeat],
  );

  // --- Pointer handlers ---

  const handlePointerDown = useCallback(
    (e: React.PointerEvent) => {
      if (e.button !== 0) return;
      e.preventDefault();
      (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);

      const { beat, note } = getGridPosition(e.clientX, e.clientY);
      const hit = hitTestNote(e.clientX, e.clientY);

      if (tool === 'eraser') {
        if (hit) {
          const newNotes = notes.filter((_, i) => i !== hit.index);
          onNotesChange(newNotes);
          onSelectionChange(new Set());
        }
        return;
      }

      if (tool === 'draw') {
        if (hit && hit.edge === 'body') {
          // Click on note body in draw mode = delete
          const newNotes = notes.filter((_, i) => i !== hit.index);
          onNotesChange(newNotes);
          onSelectionChange(new Set());
        } else if (hit && (hit.edge === 'left' || hit.edge === 'right') && !isDrum) {
          // Click on note edge in draw mode = resize instead of delete
          const newSelection = new Set([hit.index]);
          onSelectionChange(newSelection);
          const resizeType = hit.edge === 'right' ? 'resize-right' : 'resize-left';
          setDragState({
            type: resizeType,
            startX: e.clientX,
            startY: e.clientY,
            originBeat: beat,
            originNote: note,
            initialDurations: [notes[hit.index].durationBeats],
            initialPositions: [{
              startBeat: notes[hit.index].startBeat,
              note: notes[hit.index].note,
            }],
          });
        } else if (!hit) {
          // Create new note and start draw-extend drag
          const snappedBeat = snapBeatPR(beat, snapValue);
          const duration = isDrum ? DEFAULT_DRUM_DURATION : snapValue || DEFAULT_NOTE_DURATION;
          const newNote: RegionNote = {
            note,
            velocity: DEFAULT_NOTE_VELOCITY,
            startBeat: Math.max(0, snappedBeat),
            durationBeats: duration,
            isDrum,
          };
          const newNotes = [...notes, newNote];
          const newIndex = newNotes.length - 1;
          onNotesChange(newNotes);
          onSelectionChange(new Set([newIndex]));

          // Start draw-extend so user can drag to resize the new note
          setDragState({
            type: 'draw-extend',
            startX: e.clientX,
            startY: e.clientY,
            originBeat: snappedBeat,
            originNote: note,
            drawNoteIndex: newIndex,
          });
        }
        return;
      }

      // Pointer tool
      if (hit) {
        const isShift = e.shiftKey;
        let newSelection: Set<number>;
        if (isShift) {
          newSelection = new Set(selectedIndices);
          if (newSelection.has(hit.index)) {
            newSelection.delete(hit.index);
          } else {
            newSelection.add(hit.index);
          }
        } else if (!selectedIndices.has(hit.index)) {
          newSelection = new Set([hit.index]);
        } else {
          newSelection = selectedIndices;
        }
        onSelectionChange(newSelection);

        const selectedArray = Array.from(newSelection);
        if ((hit.edge === 'right' || hit.edge === 'left') && !isDrum) {
          // Start resize
          setDragState({
            type: hit.edge === 'right' ? 'resize-right' : 'resize-left',
            startX: e.clientX,
            startY: e.clientY,
            originBeat: beat,
            originNote: note,
            initialDurations: selectedArray.map((i) => notes[i].durationBeats),
            initialPositions: selectedArray.map((i) => ({
              startBeat: notes[i].startBeat,
              note: notes[i].note,
            })),
          });
        } else {
          // Start move
          setDragState({
            type: 'move',
            startX: e.clientX,
            startY: e.clientY,
            originBeat: beat,
            originNote: note,
            initialPositions: selectedArray.map((i) => ({
              startBeat: notes[i].startBeat,
              note: notes[i].note,
            })),
          });
        }
      } else {
        // Start rubber-band selection
        if (!e.shiftKey) {
          onSelectionChange(new Set());
        }
        setDragState({
          type: 'rubber-band',
          startX: e.clientX,
          startY: e.clientY,
          originBeat: beat,
          originNote: note,
          currentX: e.clientX,
          currentY: e.clientY,
        });
      }
    },
    [tool, notes, selectedIndices, getGridPosition, hitTestNote, onNotesChange, onSelectionChange, snapValue, isDrum],
  );

  const handlePointerMove = useCallback(
    (e: React.PointerEvent) => {
      if (!dragState) {
        // Update cursor based on hit
        if (tool === 'pointer' && gridRef.current) {
          const hit = hitTestNote(e.clientX, e.clientY);
          if ((hit?.edge === 'right' || hit?.edge === 'left') && !isDrum) {
            gridRef.current.style.cursor = 'col-resize';
          } else if (hit) {
            gridRef.current.style.cursor = 'grab';
          } else {
            gridRef.current.style.cursor = 'crosshair';
          }
        }
        return;
      }

      const { beat, note } = getGridPosition(e.clientX, e.clientY);
      const selectedArray = Array.from(selectedIndices);

      if (dragState.type === 'move' && dragState.initialPositions) {
        const deltaBeat = snapBeatPR(beat - dragState.originBeat, snapValue);
        const deltaNote = isDrum ? 0 : note - dragState.originNote;
        const newNotes = [...notes];
        selectedArray.forEach((idx, si) => {
          const init = dragState.initialPositions![si];
          newNotes[idx] = {
            ...newNotes[idx],
            startBeat: Math.max(0, init.startBeat + deltaBeat),
            note: isDrum ? newNotes[idx].note : Math.max(PR_NOTE_MIN, Math.min(PR_NOTE_MAX - 1, init.note + deltaNote)),
          };
        });
        onNotesChange(newNotes);
      } else if (dragState.type === 'resize-right' && dragState.initialDurations) {
        const deltaBeat = snapBeatPR(beat - dragState.originBeat, snapValue);
        const newNotes = [...notes];
        selectedArray.forEach((idx, si) => {
          const newDur = Math.max(snapValue || 0.125, dragState.initialDurations![si] + deltaBeat);
          newNotes[idx] = { ...newNotes[idx], durationBeats: newDur };
        });
        onNotesChange(newNotes);
      } else if (dragState.type === 'resize-left' && dragState.initialDurations && dragState.initialPositions) {
        const deltaBeat = snapBeatPR(beat - dragState.originBeat, snapValue);
        const newNotes = [...notes];
        selectedArray.forEach((idx, si) => {
          const initStart = dragState.initialPositions![si].startBeat;
          const initDur = dragState.initialDurations![si];
          const newStart = Math.max(0, initStart + deltaBeat);
          const newDur = initDur - (newStart - initStart);
          if (newDur >= (snapValue || 0.125)) {
            newNotes[idx] = { ...newNotes[idx], startBeat: newStart, durationBeats: newDur };
          }
        });
        onNotesChange(newNotes);
      } else if (dragState.type === 'draw-extend' && dragState.drawNoteIndex !== undefined) {
        const snappedBeat = snapBeatPR(beat, snapValue);
        const anchorBeat = dragState.originBeat;
        const minDuration = isDrum ? DEFAULT_DRUM_DURATION : snapValue || DEFAULT_NOTE_DURATION;
        const noteStart = Math.max(0, Math.min(anchorBeat, snappedBeat));
        const noteEnd = Math.max(anchorBeat + minDuration, snappedBeat + minDuration);
        const idx = dragState.drawNoteIndex;
        if (idx < notes.length) {
          const newNotes = [...notes];
          newNotes[idx] = {
            ...newNotes[idx],
            startBeat: noteStart,
            durationBeats: noteEnd - noteStart,
          };
          onNotesChange(newNotes);
        }
      } else if (dragState.type === 'rubber-band') {
        setDragState((prev) => prev ? { ...prev, currentX: e.clientX, currentY: e.clientY } : prev);
      }
    },
    [dragState, notes, selectedIndices, getGridPosition, hitTestNote, tool, snapValue, isDrum, onNotesChange],
  );

  const handlePointerUp = useCallback(
    (e: React.PointerEvent) => {
      if (dragState?.type === 'rubber-band' && gridRef.current) {
        // Calculate rubber-band region and select notes within
        const rect = gridRef.current.getBoundingClientRect();
        const x1 = Math.min(dragState.startX, e.clientX) - rect.left;
        const x2 = Math.max(dragState.startX, e.clientX) - rect.left;
        const y1 = Math.min(dragState.startY, e.clientY) - rect.top;
        const y2 = Math.max(dragState.startY, e.clientY) - rect.top;

        const beat1 = gridXToBeat(x1, pxPerBeat);
        const beat2 = gridXToBeat(x2, pxPerBeat);

        const newSelection = new Set(e.shiftKey ? selectedIndices : []);
        notes.forEach((n, i) => {
          const nEnd = n.startBeat + n.durationBeats;
          if (n.startBeat < beat2 && nEnd > beat1) {
            let ny: number;
            if (isDrum) {
              const row = drumMidiToRow(n.note);
              ny = drumRowToGridY(row, rowHeight) + rowHeight / 2;
            } else {
              ny = noteToGridY(n.note, rowHeight) + rowHeight / 2;
            }
            if (ny >= y1 && ny <= y2) {
              newSelection.add(i);
            }
          }
        });
        onSelectionChange(newSelection);
      }
      setDragState(null);
    },
    [dragState, notes, selectedIndices, pxPerBeat, rowHeight, isDrum, onSelectionChange],
  );

  // Keyboard shortcuts within the grid
  useEffect(() => {
    const handleKey = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;

      // Delete selected notes
      if ((e.key === 'Delete' || e.key === 'Backspace') && selectedIndices.size > 0) {
        e.preventDefault();
        const newNotes = notes.filter((_, i) => !selectedIndices.has(i));
        onNotesChange(newNotes);
        onSelectionChange(new Set());
        return;
      }

      // Select all
      if ((e.metaKey || e.ctrlKey) && e.key === 'a') {
        e.preventDefault();
        onSelectionChange(new Set(notes.map((_, i) => i)));
        return;
      }

      // Tool shortcuts
      if (e.key === 'v' || e.key === 'V') { onSelectionChange(selectedIndices); /* trigger tool change handled by parent */ }
    };
    window.addEventListener('keydown', handleKey);
    return () => window.removeEventListener('keydown', handleKey);
  }, [notes, selectedIndices, onNotesChange, onSelectionChange]);

  // --- Rendering ---

  const gridBackground = useMemo(() => {
    const lines: React.JSX.Element[] = [];

    // Horizontal lines (note rows)
    for (let i = 0; i <= totalRows; i++) {
      const y = i * rowHeight;
      let bgColor: string;
      if (isDrum) {
        bgColor = i % 2 === 0 ? 'rgba(255,255,255,0.02)' : 'transparent';
      } else {
        const midi = PR_NOTE_MAX - i - 1;
        bgColor = isBlackKey(midi) ? 'rgba(255,255,255,0.03)' : 'transparent';
      }
      lines.push(
        <div
          key={`row-${i}`}
          className="absolute w-full border-b"
          style={{
            top: y,
            height: rowHeight,
            backgroundColor: bgColor,
            borderColor: 'oklch(0.25 0.005 270)',
          }}
        />,
      );
    }

    // Vertical lines (beat grid)
    for (let beat = 0; beat <= regionLengthBeats; beat += snapValue || 0.25) {
      const x = beatToGridX(beat, pxPerBeat);
      const isBar = beat % 4 === 0;
      const isBeat = beat % 1 === 0;
      lines.push(
        <div
          key={`vline-${beat}`}
          className="absolute top-0 h-full"
          style={{
            left: x,
            width: 1,
            backgroundColor: isBar
              ? 'oklch(0.35 0.01 270)'
              : isBeat
                ? 'oklch(0.28 0.005 270)'
                : 'oklch(0.22 0.003 270)',
          }}
        />,
      );
    }

    return lines;
  }, [totalRows, rowHeight, regionLengthBeats, pxPerBeat, snapValue, isDrum]);

  // Render note rectangles
  const noteRects = useMemo(() => {
    return notes.map((note, i) => {
      const x = beatToGridX(note.startBeat, pxPerBeat);
      const w = beatToGridX(note.durationBeats, pxPerBeat);
      let y: number;
      if (isDrum) {
        const row = drumMidiToRow(note.note);
        y = drumRowToGridY(row >= 0 ? row : 0, rowHeight);
      } else {
        y = noteToGridY(note.note, rowHeight);
      }
      return (
        <NoteRect
          key={i}
          x={x}
          y={y}
          width={w}
          height={rowHeight}
          color={trackColor}
          selected={selectedIndices.has(i)}
          velocity={note.velocity}
        />
      );
    });
  }, [notes, pxPerBeat, rowHeight, isDrum, trackColor, selectedIndices]);

  // Rubber-band rectangle
  const rubberBand = useMemo(() => {
    if (!dragState || dragState.type !== 'rubber-band' || !dragState.currentX || !dragState.currentY || !gridRef.current) return null;
    const rect = gridRef.current.getBoundingClientRect();
    const x1 = Math.min(dragState.startX, dragState.currentX) - rect.left;
    const y1 = Math.min(dragState.startY, dragState.currentY) - rect.top;
    const w = Math.abs(dragState.currentX - dragState.startX);
    const h = Math.abs(dragState.currentY - dragState.startY);
    return (
      <div
        className="absolute pointer-events-none rounded-sm border border-primary/60"
        style={{
          left: x1,
          top: y1,
          width: w,
          height: h,
          backgroundColor: 'oklch(0.60 0.15 265 / 0.15)',
        }}
      />
    );
  }, [dragState]);

  const cursorClass =
    tool === 'draw'
      ? 'cursor-crosshair'
      : tool === 'eraser'
        ? 'cursor-pointer'
        : 'cursor-default';

  return (
    <div
      ref={gridRef}
      className={`relative overflow-hidden ${cursorClass}`}
      style={{ width: gridWidth, height: gridHeight }}
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={handlePointerUp}
    >
      {gridBackground}
      {noteRects}
      {rubberBand}
    </div>
  );
}
