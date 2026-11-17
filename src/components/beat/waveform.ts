/**
 * Waveform peaks + canvas drawing shared by the pad thumbnails and the pad
 * editor. Peaks are cached per buffer and resolution.
 */

export interface Peaks {
  /** Per column: min and max sample in -1..1. */
  min: Float32Array;
  max: Float32Array;
}

interface BufferLike {
  numberOfChannels: number;
  length: number;
  getChannelData(channel: number): Float32Array;
}

const peakCache = new WeakMap<object, Map<number, Peaks>>();

/** Min/max per column over all channels (pure). */
export function computePeaks(buffer: BufferLike, columns: number): Peaks {
  const cols = Math.max(1, Math.floor(columns));
  const min = new Float32Array(cols);
  const max = new Float32Array(cols);
  const len = buffer.length;
  if (len === 0) return { min, max };
  const channels = Array.from({ length: Math.max(1, buffer.numberOfChannels) }, (_, c) => buffer.getChannelData(c));
  for (let col = 0; col < cols; col++) {
    const from = Math.floor((col * len) / cols);
    const to = Math.max(from + 1, Math.floor(((col + 1) * len) / cols));
    let lo = 1;
    let hi = -1;
    for (const data of channels) {
      // Stride through very long columns; peaks stay visually exact enough.
      const stride = Math.max(1, Math.floor((to - from) / 512));
      for (let i = from; i < to && i < data.length; i += stride) {
        const v = data[i];
        if (v < lo) lo = v;
        if (v > hi) hi = v;
      }
    }
    min[col] = lo > hi ? 0 : lo;
    max[col] = lo > hi ? 0 : hi;
  }
  return { min, max };
}

export function getPeaks(buffer: BufferLike, columns: number): Peaks {
  const cols = Math.max(1, Math.floor(columns));
  let byCols = peakCache.get(buffer);
  if (!byCols) {
    byCols = new Map();
    peakCache.set(buffer, byCols);
  }
  let peaks = byCols.get(cols);
  if (!peaks) {
    peaks = computePeaks(buffer, cols);
    byCols.set(cols, peaks);
  }
  return peaks;
}

/** Size a canvas's backing store to its CSS box × devicePixelRatio; returns the 2D context (or null in jsdom). */
export function prepareCanvas(canvas: HTMLCanvasElement, cssW: number, cssH: number): CanvasRenderingContext2D | null {
  const dpr = typeof window !== 'undefined' ? Math.max(1, window.devicePixelRatio || 1) : 1;
  const w = Math.max(1, Math.round(cssW * dpr));
  const h = Math.max(1, Math.round(cssH * dpr));
  if (canvas.width !== w) canvas.width = w;
  if (canvas.height !== h) canvas.height = h;
  let ctx: CanvasRenderingContext2D | null;
  try {
    ctx = canvas.getContext('2d');
  } catch {
    ctx = null;
  }
  if (!ctx) return null;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, cssW, cssH);
  return ctx;
}

/** Mirrored min/max waveform filling the given box. */
export function drawPeaks(
  ctx: CanvasRenderingContext2D,
  peaks: Peaks,
  box: { x: number; y: number; w: number; h: number },
  color: string,
): void {
  const cols = peaks.min.length;
  const mid = box.y + box.h / 2;
  const half = box.h / 2;
  ctx.fillStyle = color;
  const colW = box.w / cols;
  for (let i = 0; i < cols; i++) {
    const top = mid - Math.max(0.5 / half, peaks.max[i]) * half;
    const bottom = mid - Math.min(-0.5 / half, peaks.min[i]) * half;
    ctx.fillRect(box.x + i * colW, top, Math.max(1, colW - 0.25), Math.max(1, bottom - top));
  }
}
