import { memo, useEffect, useRef } from 'react';
import { PackageX, TriangleAlert } from 'lucide-react';
import { cn } from '@/lib/utils';
import type { PadSettings } from '@/types/sampler';
import { usePadBuffer } from './usePadBuffer';
import { missingPacks, usePackAvailability } from './usePackAvailability';
import { drawPeaks, getPeaks, prepareCanvas } from './waveform';

interface PadThumbProps {
  settings: PadSettings;
  color: string;
  className?: string;
}

/** Tiny waveform of a pad's sample; the trimmed part is drawn in the pad color. */
export const PadThumb = memo(function PadThumb({ settings, color, className }: PadThumbProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const { buffer, status } = usePadBuffer(settings.sample?.source);
  const { startSec, endSec } = settings;
  const packs = usePackAvailability();
  const missingPack = status === 'missing' ? (missingPacks([settings], packs)[0] ?? null) : null;

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const draw = () => {
      const w = canvas.clientWidth;
      const h = canvas.clientHeight;
      if (!w || !h) return;
      const ctx = prepareCanvas(canvas, w, h);
      if (!ctx || !buffer) return;
      const peaks = getPeaks(buffer, Math.min(160, Math.max(16, Math.round(w / 2))));
      drawPeaks(ctx, peaks, { x: 0, y: 0, w, h }, 'oklch(1 0 0 / 0.16)');
      // Played region on top, clipped to the trim range.
      const dur = buffer.duration || 1;
      const a = Math.min(1, Math.max(0, startSec / dur));
      const b = Math.min(1, Math.max(a, (endSec ?? dur) / dur));
      ctx.save();
      ctx.beginPath();
      ctx.rect(a * w, 0, Math.max(1, (b - a) * w), h);
      ctx.clip();
      drawPeaks(ctx, peaks, { x: 0, y: 0, w, h }, color);
      ctx.restore();
    };
    draw();
    if (typeof ResizeObserver === 'undefined') return;
    const ro = new ResizeObserver(draw);
    ro.observe(canvas);
    return () => ro.disconnect();
  }, [buffer, color, startSec, endSec]);

  if (missingPack) {
    return (
      <span data-status="pack-missing" title={`Pack not installed: ${missingPack.name}`} className={cn('flex items-center gap-1 font-mono text-[11px] leading-none text-signal-warn', className)}>
        <PackageX aria-hidden className="size-3 shrink-0" />
        <span className="truncate">no pack</span>
      </span>
    );
  }

  if (status === 'missing') {
    return (
      <span data-status={status} title="Sample file not found" className={cn('flex items-center gap-1 font-mono text-[11px] leading-none text-signal-warn', className)}>
        <TriangleAlert aria-hidden className="size-3 shrink-0" />
        <span className="truncate">missing</span>
      </span>
    );
  }

  return (
    <canvas
      ref={canvasRef}
      aria-hidden
      data-status={status}
      className={cn(status === 'loading' && 'animate-pulse', className)}
    />
  );
});
