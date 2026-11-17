import { useLayoutEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react';
import type { PianoRollViewport, RowLayout } from '@/types/pianoRollEditor';
import { isBlackKey, midiToNoteName } from '@/utils/noteHelpers';
import { prepareCanvas, type PianoRollColors } from './canvas';

export const KEY_COLUMN_WIDTH = 76;
const BLACK_KEY_RATIO = 0.6;

interface PianoKeyColumnProps {
  viewport: PianoRollViewport;
  colors: PianoRollColors;
  layout: RowLayout;
  /** Row under the grid cursor (highlighted). */
  hoverRow: number | null;
  /** Pitches to light up (selected notes / auditioned key). */
  litPitches: ReadonlySet<number>;
  onPreview: (pitch: number) => void;
  onWheel: (e: WheelEvent) => void;
}

/** Canvas-drawn keyboard. Click / drag across keys to audition. */
export function PianoKeyColumn({ viewport, colors, layout, hoverRow, litPitches, onPreview, onWheel }: PianoKeyColumnProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const wrapRef = useRef<HTMLDivElement>(null);
  const dragRow = useRef<number | null>(null);
  const [pressed, setPressed] = useState<number | null>(null);
  const { rowHeight, scrollTop, height } = viewport;
  const width = KEY_COLUMN_WIDTH;

  useLayoutEffect(() => {
    const ctx = prepareCanvas(canvasRef.current, width, height);
    if (!ctx) return;
    const firstRow = Math.max(0, Math.floor(scrollTop / rowHeight) - 1);
    const lastRow = Math.min(layout.rowCount - 1, Math.ceil((scrollTop + height) / rowHeight) + 1);
    const blackW = Math.round(width * BLACK_KEY_RATIO);

    ctx.fillStyle = colors.keyWhite;
    ctx.fillRect(0, 0, width, height);
    ctx.font = '600 9px "JetBrains Mono", ui-monospace, monospace';
    ctx.textBaseline = 'middle';
    ctx.textAlign = 'right';

    for (let row = firstRow; row <= lastRow; row++) {
      const pitch = layout.rowToPitch(row);
      const y = row * rowHeight - scrollTop;
      const black = isBlackKey(pitch);
      const lit = litPitches.has(pitch) || pressed === pitch;
      const hover = hoverRow === row;

      if (!black) {
        if (lit || hover) {
          ctx.fillStyle = lit ? colors.primary : colors.keyHover;
          ctx.globalAlpha = lit ? 0.85 : 0.35;
          ctx.fillRect(0, y, width, rowHeight);
          ctx.globalAlpha = 1;
        }
        // Key separators: full width below C and F (B|C and E|F have no black key between)
        const pc = pitch % 12;
        if (pc === 0 || pc === 5) {
          ctx.fillStyle = colors.border;
          ctx.fillRect(0, Math.round(y + rowHeight) - 1, width, 1);
        }
      }
    }

    // Black keys on top (+ the white-key split line through their middle)
    for (let row = firstRow; row <= lastRow; row++) {
      const pitch = layout.rowToPitch(row);
      if (!isBlackKey(pitch)) continue;
      const y = row * rowHeight - scrollTop;
      ctx.fillStyle = colors.border;
      ctx.fillRect(blackW, Math.round(y + rowHeight / 2), width - blackW, 1);
      const lit = litPitches.has(pitch) || pressed === pitch;
      ctx.fillStyle = lit ? colors.primary : colors.keyBlack;
      ctx.beginPath();
      ctx.roundRect(0, y + 0.5, blackW, rowHeight - 1, [0, 3, 3, 0]);
      ctx.fill();
      if (hoverRow === row && !lit) {
        ctx.fillStyle = colors.keyHover;
        ctx.globalAlpha = 0.35;
        ctx.fill();
        ctx.globalAlpha = 1;
      }
    }

    // Labels: every C, plus the hovered key
    for (let row = firstRow; row <= lastRow; row++) {
      const pitch = layout.rowToPitch(row);
      const isC = pitch % 12 === 0;
      if (!isC && hoverRow !== row) continue;
      if (rowHeight < 7) continue;
      const y = row * rowHeight - scrollTop + rowHeight / 2;
      const lit = litPitches.has(pitch) || pressed === pitch;
      ctx.fillStyle = lit ? colors.primaryFg : isBlackKey(pitch) ? colors.foreground : colors.keyLabel;
      if (isBlackKey(pitch)) {
        ctx.textAlign = 'left';
        ctx.fillText(midiToNoteName(pitch), blackW + 4, y);
        ctx.textAlign = 'right';
      } else {
        ctx.fillText(midiToNoteName(pitch), width - 5, y);
      }
    }

    // Right edge
    ctx.fillStyle = colors.border;
    ctx.fillRect(width - 1, 0, 1, height);
  }, [colors, layout, rowHeight, scrollTop, height, width, hoverRow, litPitches, pressed]);

  useLayoutEffect(() => {
    const el = wrapRef.current;
    if (!el) return;
    el.addEventListener('wheel', onWheel, { passive: false });
    return () => el.removeEventListener('wheel', onWheel);
  }, [onWheel]);

  const pitchAt = (e: ReactPointerEvent<HTMLDivElement>) => {
    const rect = e.currentTarget.getBoundingClientRect();
    const row = Math.floor((e.clientY - rect.top + scrollTop) / rowHeight);
    if (row < 0 || row >= layout.rowCount) return null;
    return { row, pitch: layout.rowToPitch(row) };
  };

  return (
    <div
      ref={wrapRef}
      className="relative shrink-0 cursor-pointer select-none overflow-hidden"
      style={{ width, height }}
      title="Click or drag across the keys to audition"
      onPointerDown={(e) => {
        if (e.button !== 0) return;
        const hit = pitchAt(e);
        if (!hit) return;
        e.currentTarget.setPointerCapture(e.pointerId);
        dragRow.current = hit.row;
        setPressed(hit.pitch);
        onPreview(hit.pitch);
      }}
      onPointerMove={(e) => {
        if (dragRow.current === null) return;
        const hit = pitchAt(e);
        if (!hit || hit.row === dragRow.current) return;
        dragRow.current = hit.row;
        setPressed(hit.pitch);
        onPreview(hit.pitch);
      }}
      onPointerUp={() => {
        dragRow.current = null;
        setPressed(null);
      }}
      onPointerCancel={() => {
        dragRow.current = null;
        setPressed(null);
      }}
    >
      <canvas ref={canvasRef} className="absolute inset-0 block" />
    </div>
  );
}
