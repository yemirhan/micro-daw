import { useRef, useEffect } from 'react';
import { sampleManager } from '@/services/SampleManager';
import type { SampleLibraryEntry } from '@/types/samples';
import { cn } from '@/lib/utils';
import { themeColor } from './themeColor';

interface SampleListItemProps {
  entry: SampleLibraryEntry;
  isSelected: boolean;
  onSelect: () => void;
}

function formatDuration(seconds: number): string {
  if (seconds < 1) return `${(seconds * 1000).toFixed(0)}ms`;
  if (seconds < 60) return `${seconds.toFixed(1)}s`;
  const mins = Math.floor(seconds / 60);
  const secs = seconds % 60;
  return `${mins}:${secs.toFixed(0).padStart(2, '0')}`;
}

const THUMB_W = 72;
const THUMB_H = 28;

export function SampleListItem({ entry, isSelected, onSelect }: SampleListItemProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    canvas.width = THUMB_W * dpr;
    canvas.height = THUMB_H * dpr;
    ctx.scale(dpr, dpr);
    ctx.clearRect(0, 0, THUMB_W, THUMB_H);

    const peaks = sampleManager.computePeaks(entry.sample.id, THUMB_W);
    if (!peaks) return;

    const midY = THUMB_H / 2;
    ctx.fillStyle = isSelected ? themeColor('--primary', 'oklch(0.74 0.17 55)') : themeColor('--muted-foreground', 'oklch(0.68 0 0)');
    ctx.globalAlpha = isSelected ? 0.95 : 0.6;
    ctx.beginPath();
    ctx.moveTo(0, midY);
    for (let i = 0; i < peaks.length; i++) ctx.lineTo(i, midY - peaks[i] * midY);
    for (let i = peaks.length - 1; i >= 0; i--) ctx.lineTo(i, midY + peaks[i] * midY);
    ctx.closePath();
    ctx.fill();
  }, [entry.sample.id, isSelected]);

  return (
    <button
      type="button"
      aria-pressed={isSelected}
      className={cn(
        'relative flex w-full items-center gap-3 rounded-md px-2 py-1.5 text-left transition-colors duration-150',
        'outline-none focus-visible:ring-2 focus-visible:ring-ring/60',
        isSelected ? 'bg-surface-3 text-foreground' : 'text-foreground/85 hover:bg-surface-2',
      )}
      onClick={onSelect}
    >
      {isSelected && <span className="absolute inset-y-1.5 left-0 w-0.5 rounded-full bg-primary" aria-hidden />}
      <canvas
        ref={canvasRef}
        className="shrink-0 rounded-sm border border-border bg-surface-0"
        style={{ width: THUMB_W, height: THUMB_H }}
        aria-hidden
      />
      <div className="min-w-0 flex-1">
        <div className="truncate text-[13px] font-medium">{entry.sample.name}</div>
        <div className="font-mono text-[10.5px] tabular-nums text-muted-foreground">
          {formatDuration(entry.sample.durationSeconds)} · {entry.sample.channels === 1 ? 'mono' : 'stereo'}
        </div>
      </div>
    </button>
  );
}
