import { useCallback, useEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from 'react';
import { cn } from '@/lib/utils';
import { SampleBrowser } from './SampleBrowser';

export const BROWSER_MIN_WIDTH = 200;
export const BROWSER_MAX_WIDTH = 520;
export const BROWSER_DEFAULT_WIDTH = 248;
const WIDTH_KEY = 'micro-daw:browser-width';

export const clampBrowserWidth = (w: number): number =>
  Number.isFinite(w) ? Math.round(Math.min(BROWSER_MAX_WIDTH, Math.max(BROWSER_MIN_WIDTH, w))) : BROWSER_DEFAULT_WIDTH;

function readWidth(): number {
  try {
    const v = Number(localStorage.getItem(WIDTH_KEY));
    return v ? clampBrowserWidth(v) : BROWSER_DEFAULT_WIDTH;
  } catch {
    return BROWSER_DEFAULT_WIDTH;
  }
}

function writeWidth(w: number) {
  try {
    localStorage.setItem(WIDTH_KEY, String(w));
  } catch {
    // ignore (private mode / blocked storage)
  }
}

export interface BrowserPanelProps {
  /** Assign a file to the selected pad of the active drum track. */
  onAssign: (path: string, name: string) => void;
  className?: string;
}

/**
 * Left-hand sample browser panel with a drag handle on its right edge
 * (arrow keys on the focused handle resize too). The width is remembered.
 */
export function BrowserPanel({ onAssign, className }: BrowserPanelProps) {
  const [width, setWidth] = useState(readWidth);
  const drag = useRef<{ x: number; start: number } | null>(null);
  const widthRef = useRef(width);
  useEffect(() => {
    widthRef.current = width;
  });

  const commit = useCallback(() => writeWidth(widthRef.current), []);

  const onPointerDown = (e: PointerEvent<HTMLDivElement>) => {
    e.preventDefault();
    drag.current = { x: e.clientX, start: width };
    e.currentTarget.setPointerCapture(e.pointerId);
  };
  const onPointerMove = (e: PointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    if (d) setWidth(clampBrowserWidth(d.start + e.clientX - d.x));
  };
  const onPointerUp = () => {
    if (!drag.current) return;
    drag.current = null;
    commit();
  };
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
    e.preventDefault();
    e.stopPropagation();
    const next = clampBrowserWidth(width + (e.key === 'ArrowRight' ? 1 : -1) * (e.shiftKey ? 48 : 16));
    setWidth(next);
    writeWidth(next);
  };

  return (
    <aside
      aria-label="Sample browser"
      data-keys-scope="browser"
      style={{ width }}
      className={cn('relative flex shrink-0 flex-col border-r border-border bg-surface-1', className)}
    >
      <SampleBrowser onAssign={onAssign} className="min-h-0 flex-1" />
      <div
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize sample browser"
        aria-valuemin={BROWSER_MIN_WIDTH}
        aria-valuemax={BROWSER_MAX_WIDTH}
        aria-valuenow={width}
        tabIndex={0}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={onPointerUp}
        onDoubleClick={() => {
          setWidth(BROWSER_DEFAULT_WIDTH);
          writeWidth(BROWSER_DEFAULT_WIDTH);
        }}
        onKeyDown={onKeyDown}
        className="absolute inset-y-0 -right-1 z-10 w-2 cursor-col-resize outline-none after:absolute after:inset-y-0 after:left-1/2 after:w-px after:-translate-x-1/2 after:bg-transparent after:transition-colors hover:after:bg-primary/60 focus-visible:after:bg-ring"
      />
    </aside>
  );
}
