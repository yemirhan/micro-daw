import { useRef, useEffect, useCallback, useState } from 'react';
import { sampleManager } from '@/services/SampleManager';
import type { SampleTrim } from '@/types/samples';
import { themeColor } from './themeColor';

interface WaveformDetailProps {
  sampleId: string;
  duration: number;
  trim?: SampleTrim;
  auditionProgress: number;
  isAuditioning: boolean;
  onTrimStartChange: (seconds: number) => void;
  onTrimEndChange: (seconds: number) => void;
}

const HANDLE_WIDTH = 2;
const HANDLE_HIT = 10;

export function WaveformDetail({
  sampleId,
  duration,
  trim,
  auditionProgress,
  isAuditioning,
  onTrimStartChange,
  onTrimEndChange,
}: WaveformDetailProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const [dragging, setDragging] = useState<'start' | 'end' | null>(null);
  /** Peaks per pixel column, cached per sample + width (redraws run every frame while auditioning). */
  const peaksCache = useRef<{ buffer: AudioBuffer; width: number; peaks: Float32Array } | null>(null);

  const trimStart = trim?.startSeconds ?? 0;
  const trimEnd = trim?.endSeconds ?? duration;

  const draw = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    const rect = canvas.getBoundingClientRect();
    const w = rect.width;
    const h = rect.height;

    canvas.width = w * dpr;
    canvas.height = h * dpr;
    ctx.scale(dpr, dpr);
    ctx.clearRect(0, 0, w, h);

    const accent = themeColor('--primary', 'oklch(0.74 0.17 55)');
    const ink = themeColor('--foreground', 'oklch(0.93 0 0)');
    const grid = themeColor('--border', 'oklch(0.29 0 0)');

    // Center line + quarter grid
    ctx.fillStyle = grid;
    ctx.fillRect(0, Math.round(h / 2), w, 1);
    for (let q = 1; q < 4; q++) ctx.fillRect(Math.round((w * q) / 4), 0, 1, h);

    const buffer = sampleManager.getBuffer(sampleId);
    if (!buffer) return;

    // Draw full waveform
    const samplesPerPeak = buffer.length / w;
    const midY = h / 2;

    const trimStartX = (trimStart / duration) * w;
    const trimEndX = (trimEnd / duration) * w;

    // Peak per pixel column (all channels)
    const width = Math.ceil(w);
    const cached = peaksCache.current;
    let peaks = cached && cached.buffer === buffer && cached.width === width ? cached.peaks : null;
    if (!peaks) {
      peaks = new Float32Array(Math.max(0, width));
      for (let i = 0; i < peaks.length; i++) {
        const bucketStart = Math.floor(i * samplesPerPeak);
        const bucketEnd = Math.min(Math.floor((i + 1) * samplesPerPeak), buffer.length);
        let max = 0;
        for (let ch = 0; ch < buffer.numberOfChannels; ch++) {
          const data = buffer.getChannelData(ch);
          for (let j = bucketStart; j < bucketEnd; j++) {
            const abs = Math.abs(data[j]);
            if (abs > max) max = abs;
          }
        }
        peaks[i] = max;
      }
      peaksCache.current = { buffer, width, peaks };
    }
    const wave = peaks;
    const tracePath = () => {
      ctx.beginPath();
      ctx.moveTo(0, midY);
      for (let i = 0; i < wave.length; i++) ctx.lineTo(i, midY - wave[i] * midY * 0.92);
      for (let i = wave.length - 1; i >= 0; i--) ctx.lineTo(i, midY + wave[i] * midY * 0.92);
      ctx.closePath();
    };

    // Whole waveform dimmed, then the trimmed region in the accent on top
    ctx.fillStyle = ink;
    ctx.globalAlpha = 0.2;
    tracePath();
    ctx.fill();

    ctx.save();
    ctx.beginPath();
    ctx.rect(trimStartX, 0, Math.max(0, trimEndX - trimStartX), h);
    ctx.clip();
    ctx.fillStyle = accent;
    ctx.globalAlpha = 0.9;
    tracePath();
    ctx.fill();
    ctx.restore();

    ctx.globalAlpha = 1;

    // Shade outside the trim region
    ctx.fillStyle = 'rgba(0, 0, 0, 0.35)';
    if (trimStartX > 0) ctx.fillRect(0, 0, trimStartX, h);
    if (trimEndX < w) ctx.fillRect(trimEndX, 0, w - trimEndX, h);

    // Trim handles: a line plus a grab tab at the top
    ctx.fillStyle = accent;
    ctx.fillRect(trimStartX, 0, HANDLE_WIDTH, h);
    ctx.fillRect(trimEndX - HANDLE_WIDTH, 0, HANDLE_WIDTH, h);
    ctx.fillRect(trimStartX, 0, 8, 12);
    ctx.fillRect(trimEndX - 8, 0, 8, 12);

    // Playback cursor
    if (isAuditioning) {
      const trimmedDuration = trimEnd - trimStart;
      const cursorSeconds = trimStart + auditionProgress * trimmedDuration;
      const cursorX = (cursorSeconds / duration) * w;
      ctx.fillStyle = ink;
      ctx.fillRect(cursorX, 0, 1, h);
    }
  }, [sampleId, duration, trimStart, trimEnd, auditionProgress, isAuditioning]);

  useEffect(() => {
    draw();
  }, [draw]);

  // Redraw on audition progress with rAF
  useEffect(() => {
    if (!isAuditioning) return;
    let raf = 0;
    const tick = () => {
      draw();
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [isAuditioning, draw]);

  const handlePointerDown = useCallback((e: React.PointerEvent) => {
    const rect = containerRef.current?.getBoundingClientRect();
    if (!rect) return;

    const x = e.clientX - rect.left;
    const w = rect.width;
    const trimStartX = (trimStart / duration) * w;
    const trimEndX = (trimEnd / duration) * w;

    // Check if near a handle
    if (Math.abs(x - trimStartX) < HANDLE_HIT) {
      setDragging('start');
      (e.target as HTMLElement).setPointerCapture(e.pointerId);
    } else if (Math.abs(x - trimEndX) < HANDLE_HIT) {
      setDragging('end');
      (e.target as HTMLElement).setPointerCapture(e.pointerId);
    }
  }, [trimStart, trimEnd, duration]);

  const handlePointerMove = useCallback((e: React.PointerEvent) => {
    if (!dragging) return;
    const rect = containerRef.current?.getBoundingClientRect();
    if (!rect) return;

    const x = e.clientX - rect.left;
    const seconds = Math.max(0, Math.min(duration, (x / rect.width) * duration));

    if (dragging === 'start') {
      onTrimStartChange(seconds);
    } else {
      onTrimEndChange(seconds);
    }
  }, [dragging, duration, onTrimStartChange, onTrimEndChange]);

  const handlePointerUp = useCallback(() => {
    setDragging(null);
  }, []);

  // Cursor style
  const getCursor = useCallback((e: React.PointerEvent) => {
    const rect = containerRef.current?.getBoundingClientRect();
    if (!rect) return;

    const x = e.clientX - rect.left;
    const w = rect.width;
    const trimStartX = (trimStart / duration) * w;
    const trimEndX = (trimEnd / duration) * w;

    const el = containerRef.current;
    if (!el) return;

    if (Math.abs(x - trimStartX) < HANDLE_HIT || Math.abs(x - trimEndX) < HANDLE_HIT) {
      el.style.cursor = 'col-resize';
    } else {
      el.style.cursor = 'default';
    }
  }, [trimStart, trimEnd, duration]);

  return (
    <div
      ref={containerRef}
      className="relative h-48 w-full overflow-hidden rounded-lg border border-border bg-surface-0 shadow-[inset_0_1px_3px_oklch(0_0_0/0.35)]"
      onPointerDown={handlePointerDown}
      onPointerMove={(e) => { handlePointerMove(e); getCursor(e); }}
      onPointerUp={handlePointerUp}
    >
      <canvas
        ref={canvasRef}
        className="absolute inset-0"
        style={{ width: '100%', height: '100%' }}
      />
    </div>
  );
}
