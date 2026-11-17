import { useCallback, useLayoutEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react';
import type { PianoRollViewport } from '@/types/pianoRollEditor';
import { computeGridLines, computeRulerLabels, snapBeat, snapFloor } from '@/utils/pianoRollHelpers';
import { crisp, prepareCanvas, type PianoRollColors } from './canvas';

export const RULER_HEIGHT = 30;
const LOOP_STRIP = 10;

interface PianoRollRulerProps {
  viewport: PianoRollViewport;
  colors: PianoRollColors;
  /** Absolute beat of the region start (grid follows song bars). */
  offset: number;
  regionLength: number;
  snap: number;
  /** Region-relative playhead beat, or null when hidden. */
  playhead: number | null;
  /** Region-relative cue (play-from) beat, or null. */
  cue: number | null;
  loop: { start: number; end: number } | null;
  loopEnabled: boolean;
  onSetPosition: (relBeat: number) => void;
  onSetLoop: (relStart: number, relEnd: number) => void;
  onToggleLoop: () => void;
  onWheel: (e: WheelEvent) => void;
}

type Drag =
  | { kind: 'position' }
  | { kind: 'loop'; anchor: number; startX: number; moved: boolean; current: number };

export function PianoRollRuler({
  viewport,
  colors,
  offset,
  regionLength,
  snap,
  playhead,
  cue,
  loop,
  loopEnabled,
  onSetPosition,
  onSetLoop,
  onToggleLoop,
  onWheel,
}: PianoRollRulerProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const wrapRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<Drag | null>(null);
  const [draftLoop, setDraftLoop] = useState<{ start: number; end: number } | null>(null);
  const { pxPerBeat, scrollLeft, width } = viewport;

  useLayoutEffect(() => {
    const ctx = prepareCanvas(canvasRef.current, width, RULER_HEIGHT);
    if (!ctx) return;
    const startBeat = scrollLeft / pxPerBeat;
    const endBeat = (scrollLeft + width) / pxPerBeat;
    const x = (beat: number) => beat * pxPerBeat - scrollLeft;

    ctx.fillStyle = colors.ruler;
    ctx.fillRect(0, 0, width, RULER_HEIGHT);

    // Outside the region
    const endX = x(regionLength);
    if (endX < width) {
      ctx.fillStyle = colors.outside;
      ctx.fillRect(Math.max(0, endX), 0, width - Math.max(0, endX), RULER_HEIGHT);
    }

    // Loop strip
    const shownLoop = draftLoop ?? loop;
    ctx.fillStyle = colors.loopFill;
    ctx.fillRect(0, 0, width, LOOP_STRIP);
    if (shownLoop) {
      const lx0 = x(shownLoop.start);
      const lx1 = x(shownLoop.end);
      ctx.globalAlpha = loopEnabled || draftLoop ? 1 : 0.4;
      ctx.fillStyle = colors.loop;
      ctx.fillRect(lx0, 1, lx1 - lx0, LOOP_STRIP - 2);
      ctx.globalAlpha = 1;
    }

    // Ticks
    const lines = computeGridLines(startBeat - 1, endBeat + 1, pxPerBeat, snap, offset, 8);
    for (const line of lines) {
      const lx = crisp(x(line.beat));
      const h = line.level === 0 ? RULER_HEIGHT - LOOP_STRIP : line.level === 1 ? 7 : 4;
      ctx.strokeStyle = line.level === 0 ? colors.lineBar : colors.lineBeat;
      ctx.beginPath();
      ctx.moveTo(lx, RULER_HEIGHT);
      ctx.lineTo(lx, RULER_HEIGHT - h);
      ctx.stroke();
    }

    // Labels
    ctx.font = '600 10px "JetBrains Mono", ui-monospace, monospace';
    ctx.textBaseline = 'middle';
    for (const label of computeRulerLabels(startBeat - 4, endBeat, pxPerBeat, offset, 40)) {
      ctx.fillStyle = label.major ? colors.foreground : colors.mutedFg;
      ctx.fillText(label.label, x(label.beat) + 4, LOOP_STRIP + (RULER_HEIGHT - LOOP_STRIP) / 2);
    }

    // Region end marker
    if (endX >= 0 && endX <= width) {
      ctx.strokeStyle = colors.mutedFg;
      ctx.beginPath();
      ctx.moveTo(crisp(endX), LOOP_STRIP);
      ctx.lineTo(crisp(endX), RULER_HEIGHT);
      ctx.stroke();
    }

    const triangle = (beat: number, color: string) => {
      const px = x(beat);
      if (px < -8 || px > width + 8) return;
      ctx.fillStyle = color;
      ctx.beginPath();
      ctx.moveTo(px - 5, RULER_HEIGHT - 9);
      ctx.lineTo(px + 5, RULER_HEIGHT - 9);
      ctx.lineTo(px, RULER_HEIGHT - 1);
      ctx.closePath();
      ctx.fill();
      ctx.fillRect(Math.round(px) - 0.5, LOOP_STRIP, 1, RULER_HEIGHT - LOOP_STRIP - 9);
    };
    if (cue !== null) triangle(cue, colors.cue);
    if (playhead !== null) triangle(playhead, colors.playhead);

    // Bottom border
    ctx.fillStyle = colors.border;
    ctx.fillRect(0, RULER_HEIGHT - 1, width, 1);
  }, [colors, width, pxPerBeat, scrollLeft, offset, regionLength, snap, playhead, cue, loop, loopEnabled, draftLoop]);

  // Native (non-passive) wheel listener so Cmd+wheel zoom can preventDefault.
  useLayoutEffect(() => {
    const el = wrapRef.current;
    if (!el) return;
    el.addEventListener('wheel', onWheel, { passive: false });
    return () => el.removeEventListener('wheel', onWheel);
  }, [onWheel]);

  const beatAt = useCallback(
    (clientX: number) => {
      const rect = wrapRef.current?.getBoundingClientRect();
      return ((clientX - (rect?.left ?? 0)) + scrollLeft) / pxPerBeat;
    },
    [scrollLeft, pxPerBeat],
  );

  const handlePointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    e.preventDefault();
    e.currentTarget.setPointerCapture(e.pointerId);
    const rect = e.currentTarget.getBoundingClientRect();
    const y = e.clientY - rect.top;
    const free = e.altKey || e.metaKey || e.ctrlKey;
    const beat = beatAt(e.clientX);
    if (y < LOOP_STRIP + 2 || e.shiftKey) {
      const anchor = free ? beat : snapFloor(beat, snap || 1, offset);
      dragRef.current = { kind: 'loop', anchor, startX: e.clientX, moved: false, current: anchor };
      return;
    }
    dragRef.current = { kind: 'position' };
    onSetPosition(Math.max(-offset, free ? beat : snapBeat(beat, snap, offset)));
  };

  const handlePointerMove = (e: ReactPointerEvent<HTMLDivElement>) => {
    const drag = dragRef.current;
    if (!drag) return;
    const free = e.altKey || e.metaKey || e.ctrlKey;
    const beat = beatAt(e.clientX);
    if (drag.kind === 'position') {
      onSetPosition(Math.max(-offset, free ? beat : snapBeat(beat, snap, offset)));
      return;
    }
    if (Math.abs(e.clientX - drag.startX) > 3) drag.moved = true;
    if (!drag.moved) return;
    drag.current = Math.max(-offset, free ? beat : snapBeat(beat, snap || 1, offset));
    const a = Math.min(drag.anchor, drag.current);
    const b = Math.max(drag.anchor, drag.current);
    setDraftLoop(b > a ? { start: a, end: b } : null);
  };

  const handlePointerUp = () => {
    const drag = dragRef.current;
    dragRef.current = null;
    if (!drag || drag.kind !== 'loop') return;
    setDraftLoop(null);
    if (!drag.moved) {
      onToggleLoop();
      return;
    }
    const a = Math.min(drag.anchor, drag.current);
    const b = Math.max(drag.anchor, drag.current);
    if (b - a > 1e-3) onSetLoop(a, b);
  };

  return (
    <div
      ref={wrapRef}
      className="relative cursor-pointer select-none"
      style={{ height: RULER_HEIGHT }}
      title="Click/drag: set playhead · Drag the top strip (or Shift+drag): set loop · Click the strip: toggle loop"
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={handlePointerUp}
      onPointerCancel={handlePointerUp}
    >
      <canvas ref={canvasRef} className="absolute inset-0 block" />
    </div>
  );
}
