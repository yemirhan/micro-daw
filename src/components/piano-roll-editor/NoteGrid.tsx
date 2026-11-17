import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type MutableRefObject, type PointerEvent as ReactPointerEvent } from 'react';
import type { RegionNote } from '@/types/arrangement';
import type { PianoRollTool, PianoRollViewport, RowLayout } from '@/types/pianoRollEditor';
import { midiToNoteName } from '@/utils/noteHelpers';
import {
  appendCopies,
  applyClickSelection,
  computeGridLines,
  deleteNotes,
  drawSpan,
  hitTestNote,
  isShadedRow,
  moveNotes,
  notesInRect,
  noteVisualWidth,
  resizeNotes,
  roundBeat,
  snapFloor,
  snappedMoveDelta,
  velocityBucket,
  BEATS_PER_BAR,
  DEFAULT_DRUM_DURATION,
} from '@/utils/pianoRollHelpers';
import { crisp, prepareCanvas, velocityRamp, type PianoRollColors } from './canvas';

/** Minimum drawn width of a note (px); drum hits get a chunkier minimum. */
export const NOTE_MIN_PX = 4;
export const DRUM_MIN_PX = 9;
const DRAG_THRESHOLD = 3;
const RAMP_BUCKETS = 12;

export interface NoteGridProps {
  scrollerRef: MutableRefObject<HTMLDivElement | null>;
  viewport: PianoRollViewport;
  colors: PianoRollColors;
  layout: RowLayout;
  notes: RegionNote[];
  ghostNotes: RegionNote[];
  selection: ReadonlySet<number>;
  regionLength: number;
  /** Absolute beat of the region start (so the grid follows song bars). */
  offset: number;
  /** Total scrollable width in beats. */
  contentBeats: number;
  snap: number;
  tool: PianoRollTool;
  isDrum: boolean;
  trackColor: string;
  drawLength: number;
  drawVelocity: number;
  playhead: number | null;
  cue: number | null;
  onScroll: (left: number, top: number) => void;
  onResize: (width: number, height: number) => void;
  onWheel: (e: WheelEvent) => void;
  onDraft: (notes: RegionNote[] | null) => void;
  onCommit: (notes: RegionNote[], label: string, selection: number[]) => void;
  onSelectionChange: (selection: Set<number>) => void;
  onPreview: (pitch: number, velocity?: number) => void;
  onHoverRow: (row: number | null) => void;
  onDrawLengthChange: (beats: number) => void;
  onCursor: (beat: number | null, pitch: number | null) => void;
}

type Gesture =
  | {
      kind: 'move';
      base: RegionNote[];
      indices: number[];
      anchor: number;
      startBeat: number;
      startRow: number;
      startX: number;
      startY: number;
      lastPitch: number;
      moved: boolean;
      duplicate: boolean;
      /** Clicked an already-selected note without shift: collapse selection on a plain click. */
      collapseTo: number | null;
      current: RegionNote[];
    }
  | {
      kind: 'resize';
      base: RegionNote[];
      indices: number[];
      anchor: number;
      edge: 'start' | 'end';
      startBeat: number;
      startX: number;
      moved: boolean;
      current: RegionNote[];
    }
  | { kind: 'draw'; base: RegionNote[]; index: number; anchorStart: number; startX: number; moved: boolean; current: RegionNote[] }
  | { kind: 'paint'; base: RegionNote[]; pitch: number; row: number; current: RegionNote[]; added: number }
  | { kind: 'band'; startBeat: number; startRow: number; startX: number; startY: number; moved: boolean; additive: Set<number> | null }
  | { kind: 'erase'; base: RegionNote[]; removed: Set<number> };

export function NoteGrid(props: NoteGridProps) {
  const {
    scrollerRef,
    viewport,
    colors,
    layout,
    notes,
    ghostNotes,
    selection,
    regionLength,
    offset,
    contentBeats,
    snap,
    isDrum,
    trackColor,
    playhead,
    cue,
  } = props;
  const { pxPerBeat, rowHeight, scrollLeft, scrollTop, width, height } = viewport;
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const gestureRef = useRef<Gesture | null>(null);
  const [band, setBand] = useState<{ x0: number; y0: number; x1: number; y1: number } | null>(null);
  const [hoverRow, setHoverRow] = useState<number | null>(null);
  const hoverKey = useRef('');

  // Latest props for the pointer handlers (avoids re-binding on every render).
  const live = useRef(props);
  useLayoutEffect(() => {
    live.current = props;
  });

  const minW = isDrum ? DRUM_MIN_PX : NOTE_MIN_PX;
  const ramp = useMemo(() => velocityRamp(trackColor, RAMP_BUCKETS), [trackColor, colors]); // eslint-disable-line react-hooks/exhaustive-deps
  const rowRamps = useMemo(() => {
    if (!layout.rowColor) return null;
    const map = new Map<number, ReturnType<typeof velocityRamp>>();
    for (let r = 0; r < layout.rowCount; r++) {
      const c = layout.rowColor(r);
      if (c) map.set(r, velocityRamp(c, RAMP_BUCKETS));
    }
    return map;
  }, [layout, colors]); // eslint-disable-line react-hooks/exhaustive-deps

  const contentWidth = Math.max(width, Math.ceil(contentBeats * pxPerBeat));
  const contentHeight = Math.max(height, layout.rowCount * rowHeight);

  // -------------------------------------------------------------------------
  // Drawing
  // -------------------------------------------------------------------------
  useLayoutEffect(() => {
    const ctx = prepareCanvas(canvasRef.current, width, height);
    if (!ctx) return;
    const x = (beat: number) => beat * pxPerBeat - scrollLeft;
    const startBeat = scrollLeft / pxPerBeat;
    const endBeat = (scrollLeft + width) / pxPerBeat;
    const firstRow = Math.max(0, Math.floor(scrollTop / rowHeight));
    const lastRow = Math.min(layout.rowCount - 1, Math.ceil((scrollTop + height) / rowHeight));

    // Rows
    ctx.fillStyle = colors.outside;
    ctx.fillRect(0, 0, width, height);
    for (let row = firstRow; row <= lastRow; row++) {
      const y = row * rowHeight - scrollTop;
      ctx.fillStyle = isShadedRow(layout, row) ? colors.rowBlack : colors.rowWhite;
      ctx.fillRect(0, y, width, rowHeight);
      if (hoverRow === row) {
        ctx.fillStyle = colors.selection;
        ctx.fillRect(0, y, width, rowHeight);
      }
    }

    // Alternate bar shading (very subtle) for orientation.
    const firstBar = Math.floor((startBeat + offset) / BEATS_PER_BAR);
    const lastBar = Math.ceil((endBeat + offset) / BEATS_PER_BAR);
    ctx.fillStyle = colors.lineSub;
    const rowsBottom = layout.rowCount * rowHeight - scrollTop;
    for (let bar = firstBar; bar <= lastBar; bar++) {
      if (bar % 2 === 0) continue;
      const bx = x(bar * BEATS_PER_BAR - offset);
      ctx.fillRect(bx, 0, BEATS_PER_BAR * pxPerBeat, Math.min(height, rowsBottom));
    }

    // Horizontal row lines + octave lines
    if (rowHeight >= 9 || layout.kind === 'drum') {
      ctx.fillStyle = colors.lineSub;
      for (let row = firstRow; row <= lastRow; row++) {
        ctx.fillRect(0, Math.round((row + 1) * rowHeight - scrollTop) - 1, width, 1);
      }
    }
    if (layout.kind === 'piano') {
      ctx.fillStyle = colors.lineOctave;
      for (let row = firstRow; row <= lastRow; row++) {
        if (layout.rowToPitch(row) % 12 !== 0) continue;
        ctx.fillRect(0, Math.round((row + 1) * rowHeight - scrollTop) - 1, width, 1);
      }
    }

    // Vertical grid lines
    const lines = computeGridLines(startBeat - 1, endBeat + 1, pxPerBeat, snap, offset);
    const vh = Math.min(height, rowsBottom);
    for (const level of [2, 1, 0] as const) {
      ctx.fillStyle = level === 0 ? colors.lineBar : level === 1 ? colors.lineBeat : colors.lineSub;
      for (const line of lines) {
        if (line.level !== level) continue;
        ctx.fillRect(Math.round(x(line.beat)), 0, 1, vh);
      }
    }

    // Outside the region (dimmed but still editable: drawing there grows the region)
    const endX = x(regionLength);
    if (endX < width) {
      ctx.fillStyle = colors.outside;
      ctx.globalAlpha = 0.55;
      ctx.fillRect(Math.max(0, endX), 0, width, vh);
      ctx.globalAlpha = 1;
      ctx.fillStyle = colors.mutedFg;
      ctx.fillRect(Math.round(endX), 0, 1, vh);
    }

    // Ghost notes (other regions on the same track)
    if (ghostNotes.length > 0) {
      ctx.fillStyle = colors.ghost;
      for (const n of ghostNotes) {
        const row = layout.pitchToRow(n.note);
        if (row < firstRow || row > lastRow) continue;
        const nx = x(n.startBeat);
        const w = noteVisualWidth(n, pxPerBeat, minW);
        if (nx > width || nx + w < 0) continue;
        ctx.fillRect(nx, row * rowHeight - scrollTop + 2, w, rowHeight - 4);
      }
    }

    // Notes: unselected first, selected on top.
    const radius = Math.min(3, rowHeight / 4);
    const showNames = rowHeight >= 11;
    ctx.font = `600 ${Math.min(10, Math.max(8, rowHeight - 5))}px "JetBrains Mono", ui-monospace, monospace`;
    ctx.textBaseline = 'middle';
    ctx.textAlign = 'left';
    const drawNote = (n: RegionNote, sel: boolean) => {
      const row = layout.pitchToRow(n.note);
      if (row < firstRow || row > lastRow) return;
      const nx = x(n.startBeat);
      const w = noteVisualWidth(n, pxPerBeat, minW);
      if (nx > width || nx + w < 0) return;
      const ny = row * rowHeight - scrollTop + 1;
      const nh = Math.max(2, rowHeight - 2);
      const r = rowRamps?.get(row) ?? ramp;
      ctx.beginPath();
      ctx.roundRect(nx + 0.5, ny + 0.5, Math.max(1, w - 1), nh - 1, radius);
      ctx.fillStyle = sel ? r.selected : r.fill[velocityBucket(n.velocity, RAMP_BUCKETS)];
      ctx.fill();
      ctx.lineWidth = sel ? 1.5 : 1;
      ctx.strokeStyle = sel ? colors.foreground : r.border;
      ctx.stroke();
      ctx.lineWidth = 1;
      // Velocity tick along the bottom edge
      if (!isDrum && w > 10 && nh >= 8) {
        ctx.fillStyle = r.border;
        ctx.globalAlpha = 0.5;
        ctx.fillRect(nx + 2, ny + nh - 3, Math.max(1, (w - 4) * n.velocity), 1.5);
        ctx.globalAlpha = 1;
      }
      if (showNames && !isDrum && w > 28) {
        ctx.save();
        ctx.beginPath();
        ctx.rect(nx + 3, ny, w - 6, nh);
        ctx.clip();
        ctx.fillStyle = sel ? colors.background : r.text;
        ctx.fillText(midiToNoteName(n.note), nx + 4, ny + nh / 2 - (nh >= 12 ? 0.5 : 0));
        ctx.restore();
      }
    };
    for (let i = 0; i < notes.length; i++) if (!selection.has(i)) drawNote(notes[i], false);
    for (const i of selection) if (notes[i]) drawNote(notes[i], true);

    // Cue + playhead
    if (cue !== null) {
      const cx = crisp(x(cue));
      if (cx >= 0 && cx <= width) {
        ctx.strokeStyle = colors.cue;
        ctx.setLineDash([3, 3]);
        ctx.beginPath();
        ctx.moveTo(cx, 0);
        ctx.lineTo(cx, height);
        ctx.stroke();
        ctx.setLineDash([]);
      }
    }
    if (playhead !== null) {
      const px = x(playhead);
      if (px >= -2 && px <= width + 2) {
        ctx.fillStyle = colors.playhead;
        ctx.fillRect(Math.round(px), 0, 1.5, height);
      }
    }

    // Rubber band
    if (band) {
      const bx = Math.min(band.x0, band.x1) - scrollLeft;
      const by = Math.min(band.y0, band.y1) - scrollTop;
      const bw = Math.abs(band.x1 - band.x0);
      const bh = Math.abs(band.y1 - band.y0);
      ctx.fillStyle = colors.selection;
      ctx.fillRect(bx, by, bw, bh);
      ctx.strokeStyle = colors.selectionBorder;
      ctx.strokeRect(crisp(bx), crisp(by), Math.round(bw), Math.round(bh));
    }
  }, [colors, width, height, pxPerBeat, rowHeight, scrollLeft, scrollTop, layout, notes, ghostNotes, selection, regionLength, offset, snap, isDrum, ramp, rowRamps, minW, playhead, cue, band, hoverRow]);

  // -------------------------------------------------------------------------
  // Size / scroll / wheel plumbing
  // -------------------------------------------------------------------------
  useLayoutEffect(() => {
    const el = scrollerRef.current;
    if (!el) return;
    const report = () => live.current.onResize(el.clientWidth, el.clientHeight);
    report();
    if (typeof ResizeObserver === 'undefined') return;
    const obs = new ResizeObserver(report);
    obs.observe(el);
    return () => obs.disconnect();
  }, [scrollerRef]);

  useLayoutEffect(() => {
    const el = scrollerRef.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => live.current.onWheel(e);
    el.addEventListener('wheel', onWheel, { passive: false });
    return () => el.removeEventListener('wheel', onWheel);
  }, [scrollerRef]);

  // Escape cancels an in-progress gesture (before the editor/App see it).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape' || !gestureRef.current) return;
      e.preventDefault();
      e.stopPropagation();
      gestureRef.current = null;
      setBand(null);
      live.current.onDraft(null);
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, []);

  // -------------------------------------------------------------------------
  // Pointer interaction
  // -------------------------------------------------------------------------
  const pointFromEvent = useCallback((e: { clientX: number; clientY: number }) => {
    const el = scrollerRef.current;
    const rect = el?.getBoundingClientRect();
    const cx = e.clientX - (rect?.left ?? 0) + (el?.scrollLeft ?? 0);
    const cy = e.clientY - (rect?.top ?? 0) + (el?.scrollTop ?? 0);
    const p = live.current;
    const row = Math.floor(cy / p.viewport.rowHeight);
    return { x: cx, y: cy, beat: cx / p.viewport.pxPerBeat, row };
  }, [scrollerRef]);

  const hitAt = (x: number, y: number) => {
    const p = live.current;
    return hitTestNote(p.notes, x, y, p.layout, p.viewport.pxPerBeat, p.viewport.rowHeight, {
      resizable: !p.isDrum,
      selected: p.selection as Set<number>,
      minWidthPx: p.isDrum ? DRUM_MIN_PX : NOTE_MIN_PX,
    });
  };

  const newNoteAt = (beat: number, row: number, free: boolean): RegionNote => {
    const p = live.current;
    const start = Math.max(0, free || p.snap <= 0 ? roundBeat(beat) : snapFloor(beat, p.snap, p.offset));
    return {
      note: p.layout.rowToPitch(row),
      startBeat: start,
      durationBeats: p.isDrum ? Math.min(DEFAULT_DRUM_DURATION, p.snap > 0 ? p.snap : DEFAULT_DRUM_DURATION) : p.drawLength,
      velocity: p.drawVelocity,
      isDrum: p.isDrum,
    };
  };

  const handlePointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (e.button !== 0 && e.button !== 2) return;
    const p = live.current;
    const pt = pointFromEvent(e);
    if (pt.row < 0 || pt.row >= p.layout.rowCount) return;
    e.preventDefault();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    // Keep keyboard shortcuts working after clicking the grid.
    (e.currentTarget.closest('[data-piano-roll]') as HTMLElement | null)?.focus({ preventScroll: true });
    const free = e.metaKey || e.ctrlKey;
    const hit = hitAt(pt.x, pt.y);

    // Right button or eraser: erase (drag to keep erasing)
    if (e.button === 2 || p.tool === 'eraser') {
      const removed = new Set<number>();
      if (hit) removed.add(hit.index);
      gestureRef.current = { kind: 'erase', base: p.notes, removed };
      if (removed.size) p.onDraft(deleteNotes(p.notes, removed));
      return;
    }

    if (hit) {
      const idx = hit.index;
      // Double-click a note (pointer tool) deletes it.
      if (e.detail === 2 && p.tool === 'pointer' && hit.edge === 'body') {
        p.onCommit(deleteNotes(p.notes, [idx]), 'Delete Note', []);
        return;
      }
      let sel: Set<number>;
      let collapseTo: number | null = null;
      if (e.shiftKey) {
        sel = applyClickSelection(p.selection, idx, true);
        p.onSelectionChange(sel);
        if (!sel.has(idx)) return; // toggled off: no drag
      } else {
        if (p.selection.has(idx) && p.selection.size > 1) collapseTo = idx;
        sel = applyClickSelection(p.selection, idx, false);
        p.onSelectionChange(sel);
      }
      const note = p.notes[idx];
      p.onPreview(note.note, note.velocity);
      const indices = [...sel].sort((a, b) => a - b);

      if (hit.edge !== 'body') {
        gestureRef.current = {
          kind: 'resize',
          base: p.notes,
          indices,
          anchor: idx,
          edge: hit.edge,
          startBeat: pt.beat,
          startX: e.clientX,
          moved: false,
          current: p.notes,
        };
        return;
      }

      let base = p.notes;
      let moveIdx = indices;
      let anchor = idx;
      const duplicate = e.altKey;
      if (duplicate) {
        const res = appendCopies(p.notes, indices);
        base = res.notes;
        moveIdx = res.selection;
        anchor = res.selection[indices.indexOf(idx)];
      }
      gestureRef.current = {
        kind: 'move',
        base,
        indices: moveIdx,
        anchor,
        startBeat: pt.beat,
        startRow: pt.row,
        startX: e.clientX,
        startY: e.clientY,
        lastPitch: note.note,
        moved: false,
        duplicate,
        collapseTo,
        current: base,
      };
      return;
    }

    // Empty space
    const drawHere = p.tool === 'draw' || (p.tool === 'pointer' && e.detail === 2);
    if (drawHere) {
      const note = newNoteAt(pt.beat, pt.row, free);
      const next = [...p.notes, note];
      p.onPreview(note.note, note.velocity);
      p.onSelectionChange(new Set([next.length - 1]));
      if (p.isDrum) {
        gestureRef.current = { kind: 'paint', base: p.notes, pitch: note.note, row: pt.row, current: next, added: 1 };
      } else {
        gestureRef.current = { kind: 'draw', base: p.notes, index: next.length - 1, anchorStart: note.startBeat, startX: e.clientX, moved: false, current: next };
      }
      p.onDraft(next);
      return;
    }

    // Pointer on empty space: rubber band
    gestureRef.current = {
      kind: 'band',
      startBeat: pt.beat,
      startRow: pt.row,
      startX: pt.x,
      startY: pt.y,
      moved: false,
      additive: e.shiftKey ? new Set(p.selection) : null,
    };
  };

  const autoScroll = (e: ReactPointerEvent<HTMLDivElement>) => {
    const el = scrollerRef.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    const edge = 24;
    if (rect.width < edge * 4 || rect.height < edge * 4) return;
    if (e.clientX > rect.right - edge) el.scrollLeft += 12;
    else if (e.clientX < rect.left + edge) el.scrollLeft -= 12;
    if (e.clientY > rect.bottom - edge) el.scrollTop += 8;
    else if (e.clientY < rect.top + edge) el.scrollTop -= 8;
  };

  const updateCursor = (target: HTMLElement, x: number, y: number) => {
    const p = live.current;
    let cursor = p.tool === 'draw' ? 'crosshair' : p.tool === 'eraser' ? 'cell' : 'default';
    if (p.tool !== 'eraser') {
      const hit = hitAt(x, y);
      if (hit) cursor = hit.edge === 'body' ? (p.tool === 'draw' ? 'pointer' : 'grab') : 'ew-resize';
    }
    if (target.style.cursor !== cursor) target.style.cursor = cursor;
  };

  const handlePointerMove = (e: ReactPointerEvent<HTMLDivElement>) => {
    const p = live.current;
    const pt = pointFromEvent(e);
    const g = gestureRef.current;

    // Hover reporting (row highlight + status), only when it changes.
    const inRows = pt.row >= 0 && pt.row < p.layout.rowCount;
    const snapped = p.snap > 0 ? snapFloor(pt.beat, p.snap, p.offset) : roundBeat(pt.beat);
    const key = `${inRows ? pt.row : -1}:${snapped}`;
    if (key !== hoverKey.current) {
      hoverKey.current = key;
      const row = inRows ? pt.row : null;
      setHoverRow(row);
      p.onHoverRow(row);
      p.onCursor(Math.max(0, snapped), inRows ? p.layout.rowToPitch(pt.row) : null);
    }

    if (!g) {
      updateCursor(e.currentTarget, pt.x, pt.y);
      return;
    }
    autoScroll(e);
    const free = e.metaKey || e.ctrlKey || (g.kind !== 'move' && e.altKey);
    const snap = free ? 0 : p.snap;

    switch (g.kind) {
      case 'move': {
        if (!g.moved && Math.hypot(e.clientX - g.startX, e.clientY - g.startY) < DRAG_THRESHOLD) return;
        g.moved = true;
        e.currentTarget.style.cursor = 'grabbing';
        const anchorNote = g.base[g.anchor];
        const dBeat = snappedMoveDelta(anchorNote.startBeat, pt.beat - g.startBeat, snap, p.offset);
        const dRow = pt.row - g.startRow;
        g.current = moveNotes(g.base, g.indices, dBeat, dRow, p.layout);
        if (g.duplicate) p.onSelectionChange(new Set(g.indices));
        p.onDraft(g.current);
        const pitch = g.current[g.anchor].note;
        if (pitch !== g.lastPitch) {
          g.lastPitch = pitch;
          p.onPreview(pitch, g.current[g.anchor].velocity);
        }
        return;
      }
      case 'resize': {
        if (!g.moved && Math.abs(e.clientX - g.startX) < DRAG_THRESHOLD) return;
        g.moved = true;
        g.current = resizeNotes(g.base, g.indices, g.edge, g.anchor, pt.beat - g.startBeat, snap, p.offset);
        p.onDraft(g.current);
        return;
      }
      case 'draw': {
        if (!g.moved && Math.abs(e.clientX - g.startX) < DRAG_THRESHOLD) return;
        g.moved = true;
        const span = drawSpan(g.anchorStart, pt.beat, snap, p.offset);
        const next = g.current.slice();
        next[g.index] = { ...next[g.index], ...span };
        g.current = next;
        p.onDraft(next);
        return;
      }
      case 'paint': {
        const step = p.snap > 0 ? p.snap : DEFAULT_DRUM_DURATION;
        const beat = Math.max(0, free ? roundBeat(pt.beat) : snapFloor(pt.beat, step, p.offset));
        const exists = g.current.some((n) => n.note === g.pitch && Math.abs(n.startBeat - beat) < step / 2);
        if (exists) return;
        const note: RegionNote = { note: g.pitch, startBeat: beat, durationBeats: Math.min(DEFAULT_DRUM_DURATION, step), velocity: p.drawVelocity, isDrum: true };
        g.current = [...g.current, note];
        g.added++;
        p.onDraft(g.current);
        p.onPreview(g.pitch, p.drawVelocity);
        return;
      }
      case 'band': {
        if (!g.moved && Math.hypot(pt.x - g.startX, pt.y - g.startY) < DRAG_THRESHOLD) return;
        g.moved = true;
        setBand({ x0: g.startX, y0: g.startY, x1: pt.x, y1: pt.y });
        const inside = notesInRect(p.notes, g.startBeat, pt.beat, g.startRow, pt.row, p.layout);
        const next = new Set(g.additive ?? []);
        for (const i of inside) next.add(i);
        p.onSelectionChange(next);
        return;
      }
      case 'erase': {
        const hit = hitTestNote(g.base, pt.x, pt.y, p.layout, p.viewport.pxPerBeat, p.viewport.rowHeight, {
          resizable: false,
          minWidthPx: p.isDrum ? DRUM_MIN_PX : NOTE_MIN_PX,
        });
        if (hit && !g.removed.has(hit.index)) {
          g.removed.add(hit.index);
          p.onDraft(deleteNotes(g.base, g.removed));
        }
        return;
      }
    }
  };

  const finish = (e: ReactPointerEvent<HTMLDivElement>, cancelled = false) => {
    const g = gestureRef.current;
    gestureRef.current = null;
    if (!g) return;
    const p = live.current;
    setBand(null);
    if (cancelled) {
      p.onDraft(null);
      return;
    }
    switch (g.kind) {
      case 'move':
        if (g.moved) {
          p.onCommit(g.current, g.duplicate ? 'Duplicate Notes' : g.indices.length > 1 ? 'Move Notes' : 'Move Note', g.indices);
        } else {
          p.onDraft(null);
          if (g.collapseTo !== null) p.onSelectionChange(new Set([g.collapseTo]));
        }
        break;
      case 'resize':
        if (g.moved) {
          p.onCommit(g.current, g.indices.length > 1 ? 'Resize Notes' : 'Resize Note', g.indices);
          if (g.edge === 'end') p.onDrawLengthChange(g.current[g.anchor].durationBeats);
        } else {
          p.onDraft(null);
        }
        break;
      case 'draw':
        p.onCommit(g.current, 'Draw Note', [g.index]);
        if (g.moved) p.onDrawLengthChange(g.current[g.index].durationBeats);
        break;
      case 'paint': {
        const first = g.base.length;
        p.onCommit(g.current, g.added > 1 ? 'Draw Notes' : 'Draw Note', Array.from({ length: g.added }, (_, k) => first + k));
        break;
      }
      case 'band':
        if (!g.moved && !g.additive) p.onSelectionChange(new Set());
        break;
      case 'erase':
        if (g.removed.size > 0) p.onCommit(deleteNotes(g.base, g.removed), g.removed.size > 1 ? 'Delete Notes' : 'Delete Note', []);
        else p.onDraft(null);
        break;
    }
    const pt = pointFromEvent(e);
    updateCursor(e.currentTarget, pt.x, pt.y);
  };

  return (
    <div
      ref={scrollerRef}
      data-testid="piano-roll-grid"
      data-px-per-beat={pxPerBeat}
      data-row-height={rowHeight}
      data-row-count={layout.rowCount}
      className="relative min-h-0 min-w-0 flex-1 overflow-auto overscroll-none"
      onScroll={(e) => props.onScroll(e.currentTarget.scrollLeft, e.currentTarget.scrollTop)}
    >
      <div
        className="relative touch-none select-none"
        style={{ width: contentWidth, height: contentHeight }}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={(e) => finish(e)}
        onPointerCancel={(e) => finish(e, true)}
        onPointerLeave={() => {
          if (gestureRef.current) return;
          hoverKey.current = '';
          setHoverRow(null);
          live.current.onHoverRow(null);
          live.current.onCursor(null, null);
        }}
        onContextMenu={(e) => e.preventDefault()}
      >
        <canvas ref={canvasRef} className="pointer-events-none sticky top-0 left-0 block" />
      </div>
    </div>
  );
}
