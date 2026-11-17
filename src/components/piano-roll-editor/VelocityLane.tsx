import { useLayoutEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react';
import type { RegionNote } from '@/types/arrangement';
import type { PianoRollViewport } from '@/types/pianoRollEditor';
import {
  clampVelocity,
  notesAtX,
  offsetVelocities,
  rampVelocities,
  setVelocities,
  velocityBucket,
  velocityToMidi,
} from '@/utils/pianoRollHelpers';
import { crisp, prepareCanvas, velocityRamp, type PianoRollColors } from './canvas';

export const VELOCITY_LANE_HEIGHT = 92;
export const VELOCITY_PAD_TOP = 10;
export const VELOCITY_PAD_BOTTOM = 6;
const PAD_TOP = VELOCITY_PAD_TOP;
const PAD_BOTTOM = VELOCITY_PAD_BOTTOM;
const BUCKETS = 12;

interface VelocityLaneProps {
  viewport: PianoRollViewport;
  colors: PianoRollColors;
  notes: RegionNote[];
  selection: ReadonlySet<number>;
  trackColor: string;
  regionLength: number;
  onDraft: (notes: RegionNote[] | null) => void;
  onCommit: (notes: RegionNote[], label: string) => void;
  onWheel: (e: WheelEvent) => void;
}

type Gesture =
  | { kind: 'set'; base: RegionNote[]; indices: number[]; current: RegionNote[] }
  | { kind: 'offset'; base: RegionNote[]; indices: number[]; startY: number; current: RegionNote[] }
  | { kind: 'ramp'; base: RegionNote[]; candidates: number[] | null; beatA: number; velA: number; beatB: number; velB: number; current: RegionNote[] };

/**
 * Velocity stems under the grid. Drag a stem to set it (drags all selected
 * notes relatively when the stem is part of a multi-selection); drag on empty
 * space (or Alt+drag anywhere) to draw a ramp across the notes in range
 * (only the selected ones if there is a selection).
 */
export function VelocityLane({ viewport, colors, notes, selection, trackColor, regionLength, onDraft, onCommit, onWheel }: VelocityLaneProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const wrapRef = useRef<HTMLDivElement>(null);
  const gestureRef = useRef<Gesture | null>(null);
  const [readout, setReadout] = useState<{ x: number; y: number; text: string } | null>(null);
  const [rampLine, setRampLine] = useState<{ x0: number; v0: number; x1: number; v1: number } | null>(null);
  const { pxPerBeat, scrollLeft, width } = viewport;
  const height = VELOCITY_LANE_HEIGHT;
  const usable = height - PAD_TOP - PAD_BOTTOM;
  const ramp = useMemo(() => velocityRamp(trackColor, BUCKETS), [trackColor, colors]); // eslint-disable-line react-hooks/exhaustive-deps

  const velToY = (v: number) => PAD_TOP + (1 - v) * usable;
  const yToVel = (y: number) => clampVelocity(1 - (y - PAD_TOP) / usable);

  useLayoutEffect(() => {
    const ctx = prepareCanvas(canvasRef.current, width, height);
    if (!ctx) return;
    const x = (beat: number) => beat * pxPerBeat - scrollLeft;
    ctx.fillStyle = colors.rowBlack;
    ctx.fillRect(0, 0, width, height);

    // Guides at 25 / 50 / 75 / 100 %
    ctx.fillStyle = colors.lineSub;
    for (const v of [0.25, 0.5, 0.75, 1]) ctx.fillRect(0, Math.round(velToY(v)), width, 1);

    const endX = x(regionLength);
    if (endX < width) {
      ctx.fillStyle = colors.outside;
      ctx.globalAlpha = 0.55;
      ctx.fillRect(Math.max(0, endX), 0, width, height);
      ctx.globalAlpha = 1;
    }

    const bottom = height - PAD_BOTTOM;
    const drawStem = (n: RegionNote, sel: boolean) => {
      const sx = x(n.startBeat);
      if (sx < -6 || sx > width + 6) return;
      const top = velToY(n.velocity);
      const color = sel ? ramp.selected : ramp.fill[velocityBucket(n.velocity, BUCKETS)];
      ctx.fillStyle = color;
      ctx.globalAlpha = sel ? 0.35 : 0.22;
      const barW = Math.max(2, Math.min(n.durationBeats * pxPerBeat - 1, 14));
      ctx.fillRect(sx, top, barW, bottom - top);
      ctx.globalAlpha = 1;
      ctx.fillRect(Math.round(sx), top, sel ? 2 : 1.5, bottom - top);
      ctx.beginPath();
      ctx.arc(sx + 1, top, sel ? 3.5 : 3, 0, Math.PI * 2);
      ctx.fill();
      if (sel) {
        ctx.strokeStyle = colors.foreground;
        ctx.stroke();
      }
    };
    for (let i = 0; i < notes.length; i++) if (!selection.has(i)) drawStem(notes[i], false);
    for (const i of selection) if (notes[i]) drawStem(notes[i], true);

    if (rampLine) {
      ctx.strokeStyle = colors.foreground;
      ctx.setLineDash([4, 3]);
      ctx.beginPath();
      ctx.moveTo(rampLine.x0 - scrollLeft, velToY(rampLine.v0));
      ctx.lineTo(rampLine.x1 - scrollLeft, velToY(rampLine.v1));
      ctx.stroke();
      ctx.setLineDash([]);
    }

    if (readout) {
      ctx.font = '600 10px "JetBrains Mono", ui-monospace, monospace';
      ctx.textBaseline = 'top';
      const tw = ctx.measureText(readout.text).width + 8;
      const rx = Math.min(width - tw - 2, Math.max(2, readout.x - scrollLeft + 8));
      const ry = Math.max(2, Math.min(height - 18, readout.y - 8));
      ctx.fillStyle = colors.background;
      ctx.globalAlpha = 0.9;
      ctx.fillRect(rx, ry, tw, 15);
      ctx.globalAlpha = 1;
      ctx.fillStyle = colors.foreground;
      ctx.fillText(readout.text, rx + 4, ry + 2.5);
    }

    ctx.fillStyle = colors.border;
    ctx.fillRect(0, 0, width, 1);
    ctx.strokeStyle = colors.border;
    ctx.beginPath();
    ctx.moveTo(0, crisp(height - 1));
    ctx.lineTo(width, crisp(height - 1));
    ctx.stroke();
  }, [colors, width, height, pxPerBeat, scrollLeft, notes, selection, ramp, regionLength, rampLine, readout]); // eslint-disable-line react-hooks/exhaustive-deps

  useLayoutEffect(() => {
    const el = wrapRef.current;
    if (!el) return;
    el.addEventListener('wheel', onWheel, { passive: false });
    return () => el.removeEventListener('wheel', onWheel);
  }, [onWheel]);

  const pointOf = (e: ReactPointerEvent<HTMLDivElement>) => {
    const rect = e.currentTarget.getBoundingClientRect();
    const cx = e.clientX - rect.left + scrollLeft;
    const y = e.clientY - rect.top;
    return { cx, y, beat: cx / pxPerBeat, vel: yToVel(y) };
  };

  const handlePointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    e.preventDefault();
    e.currentTarget.setPointerCapture(e.pointerId);
    const pt = pointOf(e);
    const selected = selection.size > 0 ? new Set(selection) : null;
    // Prefer stems of selected notes, then any stem.
    let hits = selected ? notesAtX(notes, pt.cx, pxPerBeat, 6, selected) : [];
    if (hits.length === 0) hits = notesAtX(notes, pt.cx, pxPerBeat, 6);

    if (hits.length > 0 && !e.altKey) {
      const multi = selected !== null && hits.some((i) => selected.has(i)) && selected.size > 1;
      if (multi) {
        gestureRef.current = { kind: 'offset', base: notes, indices: [...selected], startY: pt.y, current: notes };
        setReadout({ x: pt.cx, y: pt.y, text: '±0' });
        return;
      }
      const current = setVelocities(notes, hits, pt.vel);
      gestureRef.current = { kind: 'set', base: notes, indices: hits, current };
      onDraft(current);
      setReadout({ x: pt.cx, y: pt.y, text: String(velocityToMidi(pt.vel)) });
      return;
    }

    gestureRef.current = {
      kind: 'ramp',
      base: notes,
      candidates: selected ? [...selected] : null,
      beatA: pt.beat,
      velA: pt.vel,
      beatB: pt.beat,
      velB: pt.vel,
      current: notes,
    };
    setRampLine({ x0: pt.cx, v0: pt.vel, x1: pt.cx, v1: pt.vel });
  };

  const handlePointerMove = (e: ReactPointerEvent<HTMLDivElement>) => {
    const g = gestureRef.current;
    if (!g) return;
    const pt = pointOf(e);
    if (g.kind === 'set') {
      g.current = setVelocities(g.base, g.indices, pt.vel);
      onDraft(g.current);
      setReadout({ x: pt.cx, y: pt.y, text: String(velocityToMidi(pt.vel)) });
    } else if (g.kind === 'offset') {
      const delta = (g.startY - pt.y) / usable;
      g.current = offsetVelocities(g.base, g.indices, delta);
      onDraft(g.current);
      const d = Math.round(delta * 127);
      setReadout({ x: pt.cx, y: pt.y, text: `${d >= 0 ? '+' : ''}${d}` });
    } else {
      g.beatB = pt.beat;
      g.velB = pt.vel;
      g.current = rampVelocities(g.base, g.candidates, g.beatA, g.velA, g.beatB, g.velB);
      onDraft(g.current);
      setRampLine({ x0: g.beatA * pxPerBeat, v0: g.velA, x1: pt.cx, v1: pt.vel });
      setReadout({ x: pt.cx, y: pt.y, text: `${velocityToMidi(g.velA)} → ${velocityToMidi(pt.vel)}` });
    }
  };

  const handlePointerUp = (cancel: boolean) => {
    const g = gestureRef.current;
    gestureRef.current = null;
    setReadout(null);
    setRampLine(null);
    if (!g) return;
    if (cancel || g.current === g.base) {
      onDraft(null);
      return;
    }
    onCommit(g.current, 'Edit Velocity');
  };

  return (
    <div
      ref={wrapRef}
      className="relative min-w-0 flex-1 cursor-ns-resize touch-none select-none"
      style={{ height }}
      title="Drag a stem to set velocity · Drag empty space (or Alt+drag) to draw a ramp"
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={() => handlePointerUp(false)}
      onPointerCancel={() => handlePointerUp(true)}
    >
      <canvas ref={canvasRef} className="absolute inset-0 block" />
    </div>
  );
}
