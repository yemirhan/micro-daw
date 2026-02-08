import { useRef, useEffect, useCallback, useState } from 'react';
import { sampleManager } from '@/services/SampleManager';
import type { SampleTrim } from '@/types/samples';

interface WaveformDetailProps {
  sampleId: string;
  duration: number;
  trim?: SampleTrim;
  auditionProgress: number;
  isAuditioning: boolean;
  onTrimStartChange: (seconds: number) => void;
  onTrimEndChange: (seconds: number) => void;
}

const HANDLE_WIDTH = 4;

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

    const buffer = sampleManager.getBuffer(sampleId);
    if (!buffer) return;

    // Draw full waveform
    const samplesPerPeak = buffer.length / w;
    const midY = h / 2;

    // Dimmed region before trim start
    const trimStartX = (trimStart / duration) * w;
    const trimEndX = (trimEnd / duration) * w;

    // Draw waveform
    ctx.fillStyle = 'rgba(255, 255, 255, 0.5)';
    ctx.beginPath();
    ctx.moveTo(0, midY);

    for (let i = 0; i < w; i++) {
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
      ctx.lineTo(i, midY - max * midY);
    }

    for (let i = w - 1; i >= 0; i--) {
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
      ctx.lineTo(i, midY + max * midY);
    }

    ctx.closePath();
    ctx.fill();

    // Dim outside trim region
    ctx.fillStyle = 'rgba(0, 0, 0, 0.5)';
    if (trimStartX > 0) {
      ctx.fillRect(0, 0, trimStartX, h);
    }
    if (trimEndX < w) {
      ctx.fillRect(trimEndX, 0, w - trimEndX, h);
    }

    // Trim handles
    ctx.fillStyle = 'rgba(255, 255, 255, 0.8)';
    ctx.fillRect(trimStartX, 0, HANDLE_WIDTH, h);
    ctx.fillRect(trimEndX - HANDLE_WIDTH, 0, HANDLE_WIDTH, h);

    // Playback cursor
    if (isAuditioning) {
      const trimmedDuration = trimEnd - trimStart;
      const cursorSeconds = trimStart + auditionProgress * trimmedDuration;
      const cursorX = (cursorSeconds / duration) * w;
      ctx.fillStyle = 'rgba(255, 255, 255, 0.9)';
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
    if (Math.abs(x - trimStartX) < HANDLE_WIDTH * 3) {
      setDragging('start');
      (e.target as HTMLElement).setPointerCapture(e.pointerId);
    } else if (Math.abs(x - trimEndX) < HANDLE_WIDTH * 3) {
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

    if (Math.abs(x - trimStartX) < HANDLE_WIDTH * 3 || Math.abs(x - trimEndX) < HANDLE_WIDTH * 3) {
      el.style.cursor = 'col-resize';
    } else {
      el.style.cursor = 'default';
    }
  }, [trimStart, trimEnd, duration]);

  return (
    <div
      ref={containerRef}
      className="relative h-40 w-full rounded-md border border-border/50 bg-background/50"
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
