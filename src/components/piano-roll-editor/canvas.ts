/**
 * Canvas plumbing for the piano roll: DPR-aware sizing and theme colors
 * resolved from the app's CSS variables (so a theme change carries over to
 * the canvas-drawn panes without touching this code).
 */
import { useEffect, useState } from 'react';

/** Size a canvas for the device pixel ratio and return a context scaled to CSS px. */
export function prepareCanvas(canvas: HTMLCanvasElement | null, width: number, height: number): CanvasRenderingContext2D | null {
  if (!canvas || width <= 0 || height <= 0) return null;
  const dpr = typeof window !== 'undefined' ? window.devicePixelRatio || 1 : 1;
  const w = Math.round(width * dpr);
  const h = Math.round(height * dpr);
  if (canvas.width !== w) canvas.width = w;
  if (canvas.height !== h) canvas.height = h;
  canvas.style.width = `${width}px`;
  canvas.style.height = `${height}px`;
  let ctx: CanvasRenderingContext2D | null;
  try {
    ctx = canvas.getContext('2d');
  } catch {
    ctx = null; // jsdom
  }
  if (!ctx) return null;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, width, height);
  return ctx;
}

/** Snap a coordinate to the pixel grid so 1px lines stay crisp. */
export function crisp(v: number): number {
  return Math.round(v) + 0.5;
}

// ---------------------------------------------------------------------------
// Theme colors
// ---------------------------------------------------------------------------

let probe: HTMLSpanElement | null = null;
const cache = new Map<string, string>();

/** Resolve any CSS color expression (var(), color-mix(), oklch()) to a canvas-usable string. */
export function resolveColor(expr: string, fallback = '#888'): string {
  if (typeof document === 'undefined') return fallback;
  const hit = cache.get(expr);
  if (hit) return hit;
  if (!probe) {
    probe = document.createElement('span');
    probe.style.display = 'none';
    probe.setAttribute('aria-hidden', 'true');
    document.body.appendChild(probe);
  }
  probe.style.color = '';
  probe.style.color = expr;
  const resolved = probe.style.color ? getComputedStyle(probe).color : '';
  const out = resolved || fallback;
  cache.set(expr, out);
  return out;
}

/** `pct`% of color a over b, resolved. */
export function mixColor(a: string, b: string, pct: number): string {
  return resolveColor(`color-mix(in oklab, ${a} ${Math.round(pct)}%, ${b})`);
}

export interface PianoRollColors {
  background: string;
  card: string;
  foreground: string;
  muted: string;
  mutedFg: string;
  border: string;
  primary: string;
  primaryFg: string;
  destructive: string;
  /** Lane backgrounds */
  rowWhite: string;
  rowBlack: string;
  rowC: string;
  outside: string;
  /** Grid lines by level: bar, beat, subdivision */
  lineBar: string;
  lineBeat: string;
  lineSub: string;
  lineOctave: string;
  /** Piano keys */
  keyWhite: string;
  keyBlack: string;
  keyLabel: string;
  keyHover: string;
  selection: string;
  selectionBorder: string;
  playhead: string;
  cue: string;
  loop: string;
  loopFill: string;
  ghost: string;
  ruler: string;
}

function buildColors(): PianoRollColors {
  const v = (name: string) => `var(--${name})`;
  return {
    background: resolveColor(v('background')),
    card: resolveColor(v('card')),
    foreground: resolveColor(v('foreground')),
    muted: resolveColor(v('muted')),
    mutedFg: resolveColor(v('muted-foreground')),
    border: resolveColor(v('border')),
    primary: resolveColor(v('primary')),
    primaryFg: resolveColor(v('primary-foreground')),
    destructive: resolveColor(v('destructive')),
    rowWhite: mixColor(v('card'), v('background'), 70),
    rowBlack: mixColor(v('background'), 'black', 86),
    rowC: mixColor(v('card'), v('foreground'), 94),
    outside: mixColor(v('background'), 'black', 84),
    lineBar: mixColor(v('foreground'), 'transparent', 24),
    lineBeat: mixColor(v('foreground'), 'transparent', 11),
    lineSub: mixColor(v('foreground'), 'transparent', 5),
    lineOctave: mixColor(v('foreground'), 'transparent', 12),
    keyWhite: mixColor(v('foreground'), v('card'), 82),
    keyBlack: mixColor(v('background'), 'black', 60),
    keyLabel: mixColor(v('background'), v('foreground'), 70),
    keyHover: mixColor(v('primary'), v('foreground'), 55),
    selection: mixColor(v('primary'), 'transparent', 14),
    selectionBorder: mixColor(v('primary'), 'transparent', 70),
    playhead: resolveColor(v('primary')),
    cue: mixColor(v('foreground'), 'transparent', 45),
    loop: mixColor(v('primary'), 'transparent', 80),
    loopFill: mixColor(v('primary'), 'transparent', 9),
    ghost: mixColor(v('muted-foreground'), 'transparent', 30),
    ruler: mixColor(v('card'), v('background'), 80),
  };
}

/** Theme palette for canvases; recomputed when the root element's class/style/theme attributes change. */
export function usePianoRollColors(): PianoRollColors {
  const [colors, setColors] = useState<PianoRollColors>(buildColors);
  useEffect(() => {
    if (typeof MutationObserver === 'undefined') return;
    const obs = new MutationObserver(() => {
      cache.clear();
      setColors(buildColors());
    });
    obs.observe(document.documentElement, { attributes: true, attributeFilter: ['class', 'style', 'data-theme'] });
    return () => obs.disconnect();
  }, []);
  return colors;
}

/** Velocity color ramp for a base (track) color: index 0 = softest. */
export function velocityRamp(base: string, buckets = 12): { fill: string[]; selected: string; border: string; text: string } {
  const fill: string[] = [];
  for (let i = 0; i < buckets; i++) {
    const t = i / (buckets - 1);
    // Soft notes sink towards the background, loud ones get brighter.
    const pct = 30 + t * 70;
    const c = mixColor(base, 'var(--background)', pct);
    fill.push(t > 0.75 ? mixColor('white', c, (t - 0.75) * 60) : c);
  }
  return {
    fill,
    selected: mixColor('white', base, 55),
    border: mixColor(base, 'black', 55),
    text: mixColor('black', base, 78),
  };
}
