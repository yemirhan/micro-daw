import { useState } from 'react';
import { Play, Square, SendHorizontal, Trash2, RotateCcw, Check, Pencil } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Readout } from '@/components/ui/view-kit';
import { WaveformDetail } from './WaveformDetail';
import type { SampleLibraryEntry } from '@/types/samples';

interface InfoTabProps {
  entry: SampleLibraryEntry;
  isAuditioning: boolean;
  auditionProgress: number;
  onToggleAudition: () => void;
  onTrimStartChange: (id: string, seconds: number) => void;
  onTrimEndChange: (id: string, seconds: number) => void;
  onResetTrim: (id: string) => void;
  onRename: (id: string, name: string) => void;
  onSendToArrangement: () => void;
  onRemove: (id: string) => void;
}

function formatDuration(seconds: number): string {
  if (seconds < 1) return `${(seconds * 1000).toFixed(0)}ms`;
  if (seconds < 60) return `${seconds.toFixed(2)}s`;
  const mins = Math.floor(seconds / 60);
  const secs = seconds % 60;
  return `${mins}:${secs.toFixed(1).padStart(4, '0')}`;
}

export function InfoTab({
  entry,
  isAuditioning,
  auditionProgress,
  onToggleAudition,
  onTrimStartChange,
  onTrimEndChange,
  onResetTrim,
  onRename,
  onSendToArrangement,
  onRemove,
}: InfoTabProps) {
  const [isEditing, setIsEditing] = useState(false);
  const [editName, setEditName] = useState(entry.sample.name);

  const { sample, trim } = entry;
  const trimStart = trim?.startSeconds ?? 0;
  const trimEnd = trim?.endSeconds ?? sample.durationSeconds;
  const trimmedDuration = trimEnd - trimStart;

  const handleCommitName = () => {
    const trimmed = editName.trim();
    if (trimmed && trimmed !== sample.name) {
      onRename(sample.id, trimmed);
    } else {
      setEditName(sample.name);
    }
    setIsEditing(false);
  };

  const isTrimmed = trimStart > 0 || trimEnd < sample.durationSeconds;
  const timeInput =
    'daw-lcd h-7 w-20 px-2 text-right text-xs text-foreground outline-none focus-visible:border-primary/60 focus-visible:ring-2 focus-visible:ring-ring/40';

  return (
    <div className="flex flex-col gap-4">
      {/* Name + actions */}
      <div className="flex items-center gap-3">
        <div className="flex min-w-0 flex-1 items-center gap-1.5">
          {isEditing ? (
            <form className="flex items-center gap-1" onSubmit={(e) => { e.preventDefault(); handleCommitName(); }}>
              <input
                autoFocus
                aria-label="Sample name"
                className="h-8 w-72 rounded-md border border-input bg-surface-0 px-2 text-sm outline-none focus-visible:border-primary/60 focus-visible:ring-2 focus-visible:ring-ring/40"
                value={editName}
                onChange={(e) => setEditName(e.target.value)}
                onBlur={handleCommitName}
                onKeyDown={(e) => {
                  if (e.key === 'Escape') {
                    setEditName(sample.name);
                    setIsEditing(false);
                  }
                }}
              />
              <Button size="icon-sm" variant="ghost" type="submit" aria-label="Save name">
                <Check className="size-4" />
              </Button>
            </form>
          ) : (
            <>
              <h3 className="truncate text-base font-semibold">{sample.name}</h3>
              <Button
                size="icon-xs"
                variant="ghost"
                className="text-muted-foreground"
                onClick={() => { setEditName(sample.name); setIsEditing(true); }}
                title="Rename"
                aria-label="Rename sample"
              >
                <Pencil />
              </Button>
            </>
          )}
        </div>
        <Button variant="outline" onClick={onToggleAudition} className="gap-1.5" aria-pressed={isAuditioning}>
          {isAuditioning ? <Square className="size-3.5 fill-current" /> : <Play className="size-3.5 fill-current" />}
          {isAuditioning ? 'Stop' : 'Audition'}
        </Button>
        <Button onClick={onSendToArrangement} className="gap-1.5">
          <SendHorizontal className="size-4" />
          Send to arrangement
        </Button>
      </div>

      {/* Waveform */}
      <div>
        <WaveformDetail
          sampleId={sample.id}
          duration={sample.durationSeconds}
          trim={trim}
          auditionProgress={auditionProgress}
          isAuditioning={isAuditioning}
          onTrimStartChange={(sec) => onTrimStartChange(sample.id, sec)}
          onTrimEndChange={(sec) => onTrimEndChange(sample.id, sec)}
        />
        <div className="mt-1 flex justify-between font-mono text-[10px] tabular-nums text-muted-foreground">
          <span>0.00s</span>
          <span>Drag the orange handles to trim</span>
          <span>{sample.durationSeconds.toFixed(2)}s</span>
        </div>
      </div>

      {/* Trim + metadata */}
      <div className="flex flex-wrap items-end gap-3">
        <label className="flex flex-col gap-1">
          <span className="text-[9.5px] font-bold uppercase tracking-[0.1em] text-muted-foreground">Start (s)</span>
          <input
            type="number"
            step={0.01}
            min={0}
            max={trimEnd - 0.01}
            value={trimStart.toFixed(2)}
            onChange={(e) => onTrimStartChange(sample.id, parseFloat(e.target.value) || 0)}
            className={timeInput}
          />
        </label>
        <label className="flex flex-col gap-1">
          <span className="text-[9.5px] font-bold uppercase tracking-[0.1em] text-muted-foreground">End (s)</span>
          <input
            type="number"
            step={0.01}
            min={trimStart + 0.01}
            max={sample.durationSeconds}
            value={trimEnd.toFixed(2)}
            onChange={(e) => onTrimEndChange(sample.id, parseFloat(e.target.value) || sample.durationSeconds)}
            className={timeInput}
          />
        </label>
        <Button
          variant="ghost"
          size="sm"
          className="h-7 px-2 text-xs text-muted-foreground"
          onClick={() => onResetTrim(sample.id)}
          disabled={!isTrimmed}
        >
          <RotateCcw className="size-3.5" />
          Reset trim
        </Button>

        <div className="ml-auto flex items-end gap-2">
          <Readout label="Length">{formatDuration(trimmedDuration)}</Readout>
          <Readout label="Rate">
            {(sample.sampleRate / 1000).toFixed(1)}
            <span className="ml-0.5 text-[10px] text-muted-foreground">kHz</span>
          </Readout>
          <Readout label="Channels">{sample.channels === 1 ? 'Mono' : 'Stereo'}</Readout>
        </div>
      </div>

      {sample.path && (
        <p className="truncate font-mono text-[11px] text-muted-foreground" title={sample.path}>
          {sample.path}
        </p>
      )}

      <div className="flex items-center justify-end border-t border-border pt-3">
        <Button
          variant="ghost"
          size="sm"
          onClick={() => onRemove(sample.id)}
          className="gap-1.5 text-destructive hover:bg-destructive/10 hover:text-destructive"
        >
          <Trash2 className="size-3.5" />
          Remove from library
        </Button>
      </div>
    </div>
  );
}
