import { memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { Repeat, Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Tip } from '@/components/ui/tip';
import { cn } from '@/lib/utils';
import type { Region } from '@/types/arrangement';
import { PAD_COUNT, type PadIndex, type PadSettings } from '@/types/sampler';
import { describeStep, nudgeVelocity, stepKeyAction, type StepCell } from './stepKeys';
import {
  STEP_GRIDS,
  adjustVelocity,
  buildStepMatrix,
  cellAt,
  needsToggle,
  padColor,
  padLabel,
  padName,
  paintModeFor,
  playheadStep,
  rowForPad,
  stepBeatsFor,
  stepCount,
  stepsPerBeat,
  type PaintMode,
  type StepGrid,
} from './beatHelpers';
import { prepareCanvas } from './waveform';

export interface StepSequencerProps {
  pads: PadSettings[];
  selectedPad: PadIndex;
  /** The pattern region (null = empty pattern). */
  region: Pick<Region, 'startBeat' | 'notes'> | null;
  /** Bump when the region's notes change in place (see useBeat's patternVersion). */
  patternVersion?: number;
  patternStartBeat: number;
  bars: number;
  grid: StepGrid;
  onGridChange: (grid: StepGrid) => void;
  onToggleStep: (pad: PadIndex, step: number, stepBeats: number) => void;
  onSetVelocity: (pad: PadIndex, step: number, stepBeats: number, velocity: number) => void;
  /**
   * Wheel / keyboard velocity nudges (merged into one undo step per burst by the
   * caller, e.g. useBeat's nudgeStepVelocity). Defaults to onSetVelocity.
   */
  onNudgeVelocity?: (pad: PadIndex, step: number, stepBeats: number, velocity: number) => void;
  /** A pointer gesture (paint / velocity drag) is about to change steps: open one undo step. */
  onGestureStart?: (label: string) => void;
  /** The gesture ended (pointer up / cancel): close the undo step. */
  onGestureEnd?: () => void;
  onSelectPad: (pad: PadIndex) => void;
  /** Clears the whole pattern (the button is disabled when omitted). */
  onClear?: () => void;
  /**
   * Why playback would not loop exactly this pattern (beatHelpers'
   * describeLoopMismatch); shown as a subtle header hint. null/omitted = it does.
   */
  loopHint?: string | null;
  /** Hint button: set the transport loop to the pattern range. */
  onLoopPattern?: () => void;
  /** Transport position feed for the playhead (updated without re-rendering). */
  subscribePosition?: (cb: (beat: number) => void) => () => void;
  playing: boolean;
  className?: string;
}

const LABEL_W = 132;
const MIN_CELL_W = 18;
const MIN_ROW_H = 14;

type Drag = { grouped: boolean } & (
  | { kind: 'paint'; mode: PaintMode; visited: Set<number> }
  | { kind: 'velocity'; pad: PadIndex; step: number; startY: number; startVel: number; last: number }
);

function cssVar(name: string, fallback: string): string {
  if (typeof window === 'undefined') return fallback;
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fallback;
}

/**
 * 16-row step grid on one canvas. Click toggles, drag paints (draw or erase,
 * decided by the first cell), Alt/Option-drag or the wheel sets velocity
 * (shown as cell brightness). The playhead is a DOM column moved directly
 * from the transport feed, so playback never re-renders the grid.
 */
export function StepSequencer({
  pads,
  selectedPad,
  region,
  patternVersion,
  patternStartBeat,
  bars,
  grid,
  onGridChange,
  onToggleStep,
  onSetVelocity,
  onNudgeVelocity,
  onGestureStart,
  onGestureEnd,
  onSelectPad,
  onClear,
  loopHint,
  onLoopPattern,
  subscribePosition,
  playing,
  className,
}: StepSequencerProps) {
  const stepBeats = stepBeatsFor(grid);
  const steps = stepCount(bars, stepBeats);
  const patternBeats = bars * 4;

  const matrix = useMemo(
    () => buildStepMatrix(region, patternStartBeat, stepBeats, steps),
    // patternVersion catches in-place note edits.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [region, patternVersion, patternStartBeat, stepBeats, steps],
  );
  const matrixRef = useRef(matrix);
  useEffect(() => {
    matrixRef.current = matrix;
  }, [matrix]);

  // --- Size --------------------------------------------------------------------
  const scrollRef = useRef<HTMLDivElement>(null);
  const [box, setBox] = useState({ w: 0, h: 0 });
  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const measure = () => setBox((b) => (b.w === el.clientWidth && b.h === el.clientHeight ? b : { w: el.clientWidth, h: el.clientHeight }));
    measure();
    if (typeof ResizeObserver === 'undefined') return;
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  const rowH = Math.max(MIN_ROW_H, Math.floor(box.h / PAD_COUNT) || MIN_ROW_H);
  const cellW = Math.max(MIN_CELL_W, box.w > 0 ? Math.floor(box.w / steps) : MIN_CELL_W);
  const gridW = cellW * steps;
  const gridH = rowH * PAD_COUNT;

  // --- Draw ----------------------------------------------------------------------
  const canvasRef = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = prepareCanvas(canvas, gridW, gridH);
    if (!ctx) return;
    const surface0 = cssVar('--surface-0', '#1b1a19');
    const surface1 = cssVar('--surface-1', '#222120');
    const surface2 = cssVar('--surface-2', '#2a2928');
    const surface3 = cssVar('--surface-3', '#333231');
    const perBeat = stepsPerBeat(stepBeats);
    const perBar = perBeat * 4;

    // Beat-group shading (alternate beats) + selected row tint.
    for (let s = 0; s < steps; s += perBeat) {
      ctx.fillStyle = Math.floor(s / perBeat) % 2 === 0 ? surface0 : surface1;
      ctx.fillRect(s * cellW, 0, perBeat * cellW, gridH);
    }
    const selRow = rowForPad(selectedPad);
    ctx.fillStyle = 'oklch(1 0 0 / 0.045)';
    ctx.fillRect(0, selRow * rowH, gridW, rowH);

    const inset = cellW >= 24 ? 2 : 1.5;
    const radius = Math.min(3, cellW / 6);
    for (let pad = 0; pad < PAD_COUNT; pad++) {
      const y = rowForPad(pad) * rowH;
      const color = padColor(pads[pad], pad);
      for (let s = 0; s < steps; s++) {
        const v = matrix[pad * steps + s];
        const x = s * cellW;
        if (v > 0) {
          ctx.globalAlpha = 0.3 + 0.7 * v;
          ctx.fillStyle = color;
        } else {
          ctx.globalAlpha = 1;
          ctx.fillStyle = s % perBeat === 0 ? surface3 : surface2;
        }
        ctx.beginPath();
        ctx.roundRect?.(x + inset, y + inset, cellW - inset * 2, rowH - inset * 2, radius);
        if (!ctx.roundRect) ctx.rect(x + inset, y + inset, cellW - inset * 2, rowH - inset * 2);
        ctx.fill();
      }
    }
    ctx.globalAlpha = 1;
    // Bar lines
    ctx.fillStyle = 'oklch(1 0 0 / 0.16)';
    for (let s = perBar; s < steps; s += perBar) ctx.fillRect(s * cellW - 0.5, 0, 1, gridH);
  }, [matrix, pads, selectedPad, steps, stepBeats, cellW, rowH, gridW, gridH]);

  // --- Playhead (imperative) --------------------------------------------------------
  const playheadRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = playheadRef.current;
    if (!el) return;
    if (!playing || !subscribePosition) {
      el.style.opacity = '0';
      return;
    }
    let lastStep = -1;
    const unsub = subscribePosition((beat) => {
      const s = playheadStep(beat, patternStartBeat, patternBeats, stepBeats);
      if (s === null) {
        el.style.opacity = '0';
        lastStep = -1;
        return;
      }
      const step = Math.floor(s);
      if (step === lastStep) return;
      lastStep = step;
      el.style.opacity = '1';
      el.style.transform = `translateX(${step * cellW}px)`;
    });
    return () => {
      unsub();
      el.style.opacity = '0';
    };
  }, [playing, subscribePosition, patternStartBeat, patternBeats, stepBeats, cellW]);

  // --- Pointer -----------------------------------------------------------------------
  const drag = useRef<Drag | null>(null);
  // Ends the drag and closes its undo step (pointer up / cancel / lost capture / unmount).
  const gestureEndRef = useRef(onGestureEnd);
  useEffect(() => {
    gestureEndRef.current = onGestureEnd;
  });
  const endDrag = useCallback(() => {
    const d = drag.current;
    drag.current = null;
    if (d?.grouped) gestureEndRef.current?.();
  }, []);
  useEffect(() => endDrag, [endDrag]);

  const hitCell = useCallback(
    (e: { clientX: number; clientY: number }) => {
      const r = canvasRef.current?.getBoundingClientRect();
      if (!r) return null;
      return cellAt(e.clientX - r.left, e.clientY - r.top, cellW, rowH, steps);
    },
    [cellW, rowH, steps],
  );

  /** Open the gesture's undo step right before its first change. */
  const groupDrag = (d: Drag, label: string) => {
    if (d.grouped) return;
    d.grouped = true;
    onGestureStart?.(label);
  };

  const paint = (pad: PadIndex, step: number, d: Extract<Drag, { kind: 'paint' }>) => {
    const key = pad * steps + step;
    if (d.visited.has(key)) return;
    d.visited.add(key);
    if (!needsToggle(d.mode, matrixRef.current[key] ?? 0)) return;
    groupDrag(d, d.mode === 'draw' ? 'Paint steps' : 'Erase steps');
    onToggleStep(pad, step, stepBeats);
  };

  const onPointerDown = (e: React.PointerEvent<HTMLCanvasElement>) => {
    if (e.button !== 0) return;
    const cell = hitCell(e);
    if (!cell) return;
    e.preventDefault();
    endDrag();
    e.currentTarget.setPointerCapture?.(e.pointerId);
    // Keyboard focus follows the pointer (focus ring only shows for keyboard use).
    e.currentTarget.focus({ preventScroll: true });
    setKbFocus(false);
    setFocusCell(cell);
    const v = matrixRef.current[cell.pad * steps + cell.step] ?? 0;
    if (cell.pad !== selectedPad) onSelectPad(cell.pad);
    if (e.altKey) {
      if (v > 0) drag.current = { kind: 'velocity', grouped: false, pad: cell.pad, step: cell.step, startY: e.clientY, startVel: v, last: v };
      return;
    }
    const d: Drag = { kind: 'paint', grouped: false, mode: paintModeFor(v), visited: new Set() };
    drag.current = d;
    paint(cell.pad, cell.step, d);
  };

  const onPointerMove = (e: React.PointerEvent<HTMLCanvasElement>) => {
    const d = drag.current;
    if (!d) return;
    if (d.kind === 'velocity') {
      const next = adjustVelocity(d.startVel, d.startY - e.clientY);
      if (next !== d.last) {
        d.last = next;
        groupDrag(d, 'Step velocity');
        onSetVelocity(d.pad, d.step, stepBeats, next);
      }
      return;
    }
    const cell = hitCell(e);
    if (cell) paint(cell.pad, cell.step, d);
  };

  // Wheel over a filled cell sets its velocity (non-passive so it doesn't scroll).
  const nudge = onNudgeVelocity ?? onSetVelocity;
  const wheelRef = useRef({ hitCell, steps, stepBeats, nudge });
  useEffect(() => {
    wheelRef.current = { hitCell, steps, stepBeats, nudge };
  });
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const onWheel = (e: WheelEvent) => {
      const { hitCell: hit, steps: n, stepBeats: sb, nudge: setVel } = wheelRef.current;
      const cell = hit(e);
      if (!cell) return;
      const v = matrixRef.current[cell.pad * n + cell.step] ?? 0;
      if (v <= 0 || Math.abs(e.deltaY) < Math.abs(e.deltaX)) return;
      e.preventDefault();
      const next = adjustVelocity(v, e.deltaY < 0 ? 6 : -6);
      if (next !== v) setVel(cell.pad, cell.step, sb, next);
    };
    canvas.addEventListener('wheel', onWheel, { passive: false });
    return () => canvas.removeEventListener('wheel', onWheel);
  }, []);

  // --- Keyboard (roving focus cell) ---------------------------------------------------
  const [focusCell, setFocusCell] = useState<StepCell>(() => ({ pad: selectedPad, step: 0 }));
  const cell: StepCell = { pad: focusCell.pad, step: Math.min(focusCell.step, steps - 1) };
  /** Focus ring shows while the grid has keyboard focus (not after a click). */
  const [kbFocus, setKbFocus] = useState(false);
  const [announceTick, setAnnounceTick] = useState(0);
  const cellName = (pad: PadIndex) => padName(pads[pad]) || `Pad ${padLabel(pad)}`;

  // Live-region text for the focused cell, derived from the current matrix (so a
  // toggle is announced with its result). Empty until the grid is used by keyboard.
  const announcement = announceTick > 0 ? describeStep(cellName(cell.pad), cell.step, matrix[cell.pad * steps + cell.step] ?? 0) : '';

  // Keep the focused cell scrolled into view.
  useEffect(() => {
    const el = scrollRef.current;
    if (!el || announceTick === 0) return;
    const x = cell.step * cellW;
    if (x < el.scrollLeft) el.scrollLeft = x;
    else if (x + cellW > el.scrollLeft + el.clientWidth) el.scrollLeft = x + cellW - el.clientWidth;
  }, [announceTick, cell.step, cellW]);

  const onKeyDown = (e: React.KeyboardEvent<HTMLCanvasElement>) => {
    const action = stepKeyAction(e, cell, steps, stepsPerBeat(stepBeats));
    if (!action) return;
    // Space / Enter / brackets would otherwise reach the app's global shortcuts.
    e.preventDefault();
    e.stopPropagation();
    setKbFocus(true);
    if (action.type === 'move') {
      setFocusCell(action.cell);
      if (action.cell.pad !== selectedPad) onSelectPad(action.cell.pad);
    } else if (action.type === 'toggle') {
      if (e.repeat) return;
      onToggleStep(cell.pad, cell.step, stepBeats);
    } else {
      const v = matrixRef.current[cell.pad * steps + cell.step] ?? 0;
      if (v > 0) {
        const next = nudgeVelocity(v, action.delta);
        if (next !== v) nudge(cell.pad, cell.step, stepBeats, next);
      }
    }
    setAnnounceTick((t) => t + 1);
  };

  return (
    <section aria-label="Step sequencer" className={cn('@container/steps flex min-h-0 min-w-0 flex-col', className)}>
      <header className="flex h-8 shrink-0 items-center gap-3 px-3">
        <span className="text-[12px] font-semibold text-foreground">Steps</span>
        <div role="radiogroup" aria-label="Step size" className="flex h-6 items-center rounded-md border border-border bg-surface-0 p-0.5">
          {STEP_GRIDS.map((g) => (
            <button
              key={g}
              type="button"
              role="radio"
              aria-checked={grid === g}
              onClick={() => onGridChange(g)}
              className={cn(
                'h-full rounded-[4px] px-1.5 font-mono text-[11px] outline-none focus-visible:ring-2 focus-visible:ring-ring/70',
                grid === g ? 'bg-surface-3 text-foreground' : 'text-muted-foreground hover:text-foreground',
              )}
            >
              {g}
            </button>
          ))}
        </div>
        <span className="font-mono text-[11px] text-muted-foreground tabular-nums">
          {steps} steps · {bars} bar{bars === 1 ? '' : 's'}
        </span>
        {loopHint ? (
          <div data-loop-hint className="ml-auto flex min-w-0 items-center gap-1.5 text-[11px] text-signal-warn">
            <span className="min-w-0 truncate" title="The transport loops a different range than this grid shows">{loopHint}</span>
            {onLoopPattern && (
              <Tip label="Loop exactly this pattern">
                <Button variant="outline" size="xs" className="shrink-0" onClick={onLoopPattern}>
                  <Repeat />
                  Loop pattern
                </Button>
              </Tip>
            )}
          </div>
        ) : (
          <span className="ml-auto hidden min-w-0 truncate text-[11px] text-muted-foreground @[760px]/steps:block">
            Click to toggle, drag to paint, {navigator.platform?.includes('Mac') ? 'Option' : 'Alt'}-drag or scroll for velocity
          </span>
        )}
        <Tip label="Clear the whole pattern" shortcut="⇧⌫">
          <Button variant="ghost" size="xs" className={cn('shrink-0', loopHint ? 'ml-0' : 'ml-auto @[760px]/steps:ml-0')} disabled={!onClear} onClick={onClear}>
            <Trash2 />
            Clear
          </Button>
        </Tip>
      </header>

      <div className="flex min-h-0 flex-1 border-t border-border">
        {/* Row labels */}
        <div className="flex shrink-0 flex-col border-r border-border bg-surface-1" style={{ width: LABEL_W }}>
          {Array.from({ length: PAD_COUNT }, (_, row) => {
            const pad = PAD_COUNT - 1 - row;
            return (
              <RowLabel
                key={pad}
                pad={pad}
                settings={pads[pad]}
                height={rowH}
                selected={pad === selectedPad}
                onSelect={onSelectPad}
              />
            );
          })}
        </div>

        {/* Grid */}
        <div ref={scrollRef} className="relative min-h-0 min-w-0 flex-1 overflow-x-auto overflow-y-hidden">
          <div className="relative" style={{ width: gridW, height: gridH }}>
            <canvas
              ref={canvasRef}
              role="grid"
              tabIndex={0}
              aria-label={`Pattern grid, ${PAD_COUNT} pads by ${steps} steps. Arrow keys move, Space toggles, plus and minus set velocity.`}
              aria-rowcount={PAD_COUNT}
              aria-colcount={steps}
              data-steps={steps}
              data-focus-pad={cell.pad}
              data-focus-step={cell.step}
              className="absolute inset-0 block cursor-pointer touch-none outline-none"
              style={{ width: gridW, height: gridH }}
              onPointerDown={onPointerDown}
              onPointerMove={onPointerMove}
              onPointerUp={endDrag}
              onPointerCancel={endDrag}
              onLostPointerCapture={endDrag}
              onKeyDown={onKeyDown}
              onFocus={(e) => {
                let visible = false;
                try {
                  visible = e.currentTarget.matches(':focus-visible');
                } catch {
                  // selector unsupported (tests): treat as pointer focus
                }
                if (visible) setKbFocus(true);
              }}
              onBlur={() => setKbFocus(false)}
            />
            {/* Roving focus cell: a DOM ring over the canvas (keyboard focus only), so moving it never redraws the grid. */}
            <div
              aria-hidden
              data-step-focus
              className={cn('pointer-events-none absolute top-0 left-0 rounded-[4px] ring-2 ring-ring ring-offset-1 ring-offset-surface-0', kbFocus ? 'opacity-100' : 'opacity-0')}
              style={{ width: cellW, height: rowH, transform: `translate(${cell.step * cellW}px, ${rowForPad(cell.pad) * rowH}px)` }}
            />
            <div
              ref={playheadRef}
              aria-hidden
              className="pointer-events-none absolute top-0 left-0 opacity-0"
              style={{ width: cellW, height: gridH, willChange: 'transform' }}
            >
              <div className="absolute inset-0 bg-foreground/[0.07]" />
              <div className="absolute inset-y-0 left-0 w-[2px] bg-signal-play" />
            </div>
          </div>
        </div>
      </div>
      <div role="status" aria-live="polite" aria-atomic className="sr-only">
        {announcement}
      </div>
    </section>
  );
}

const RowLabel = memo(function RowLabel({
  pad,
  settings,
  height,
  selected,
  onSelect,
}: {
  pad: PadIndex;
  settings: PadSettings | undefined;
  height: number;
  selected: boolean;
  onSelect: (pad: PadIndex) => void;
}) {
  const color = padColor(settings, pad);
  const name = padName(settings);
  return (
    <button
      type="button"
      onClick={() => onSelect(pad)}
      aria-pressed={selected}
      aria-label={`Select pad ${padLabel(pad)}`}
      className={cn(
        'flex shrink-0 items-center gap-1.5 pr-2 pl-2 text-left outline-none focus-visible:bg-surface-3',
        selected ? 'bg-surface-3' : 'hover:bg-surface-2',
      )}
      style={{ height }}
    >
      <span className="h-[60%] max-h-3 w-[3px] shrink-0 rounded-full" style={{ background: color, opacity: settings?.sample ? 1 : 0.35 }} />
      <span className="w-[18px] shrink-0 font-mono text-[11px] text-muted-foreground">{padLabel(pad)}</span>
      <span className={cn('min-w-0 truncate text-[11px] leading-none', selected ? 'font-semibold text-foreground' : name ? 'text-foreground/80' : 'text-muted-foreground')}>
        {name || '—'}
      </span>
    </button>
  );
});
