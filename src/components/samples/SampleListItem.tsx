import { useRef, useEffect } from 'react';
import { sampleManager } from '@/services/SampleManager';
import type { SampleLibraryEntry } from '@/types/samples';
import { cn } from '@/lib/utils';

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

export function SampleListItem({ entry, isSelected, onSelect }: SampleListItemProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    const w = 100;
    const h = 32;
    canvas.width = w * dpr;
    canvas.height = h * dpr;
    ctx.scale(dpr, dpr);
    ctx.clearRect(0, 0, w, h);

    const peaks = sampleManager.computePeaks(entry.sample.id, w);
    if (!peaks) return;

    const midY = h / 2;
    ctx.fillStyle = isSelected ? 'rgba(255, 255, 255, 0.6)' : 'rgba(255, 255, 255, 0.35)';
    ctx.beginPath();
    ctx.moveTo(0, midY);

    for (let i = 0; i < peaks.length; i++) {
      const y = midY - peaks[i] * midY;
      ctx.lineTo(i, y);
    }
    for (let i = peaks.length - 1; i >= 0; i--) {
      const y = midY + peaks[i] * midY;
      ctx.lineTo(i, y);
    }

    ctx.closePath();
    ctx.fill();
  }, [entry.sample.id, isSelected]);

  return (
    <button
      className={cn(
        'flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm transition-colors',
        isSelected ? 'bg-accent text-accent-foreground' : 'hover:bg-accent/50'
      )}
      onClick={onSelect}
    >
      <canvas
        ref={canvasRef}
        className="shrink-0 rounded-sm"
        style={{ width: 100, height: 32 }}
      />
      <div className="min-w-0 flex-1">
        <div className="truncate font-medium">{entry.sample.name}</div>
        <div className="text-xs text-muted-foreground">
          {formatDuration(entry.sample.durationSeconds)}
        </div>
      </div>
    </button>
  );
}
