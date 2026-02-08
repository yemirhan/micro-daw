import { useState } from 'react';
import { Play, Square, SendHorizontal, Trash2, RotateCcw, Check, Pencil } from 'lucide-react';
import { Button } from '@/components/ui/button';
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

  return (
    <div className="flex flex-col gap-4">
      {/* Name */}
      <div className="flex items-center gap-2">
        {isEditing ? (
          <form
            className="flex items-center gap-1"
            onSubmit={(e) => { e.preventDefault(); handleCommitName(); }}
          >
            <input
              autoFocus
              className="rounded border border-border bg-background px-2 py-0.5 text-sm"
              value={editName}
              onChange={(e) => setEditName(e.target.value)}
              onBlur={handleCommitName}
            />
            <Button size="icon" variant="ghost" className="h-6 w-6" type="submit">
              <Check className="h-3 w-3" />
            </Button>
          </form>
        ) : (
          <>
            <h3 className="text-sm font-semibold">{sample.name}</h3>
            <Button
              size="icon"
              variant="ghost"
              className="h-6 w-6"
              onClick={() => { setEditName(sample.name); setIsEditing(true); }}
            >
              <Pencil className="h-3 w-3" />
            </Button>
          </>
        )}
      </div>

      {/* Waveform */}
      <WaveformDetail
        sampleId={sample.id}
        duration={sample.durationSeconds}
        trim={trim}
        auditionProgress={auditionProgress}
        isAuditioning={isAuditioning}
        onTrimStartChange={(sec) => onTrimStartChange(sample.id, sec)}
        onTrimEndChange={(sec) => onTrimEndChange(sample.id, sec)}
      />

      {/* Trim controls */}
      <div className="flex items-center gap-3 text-xs">
        <label className="flex items-center gap-1">
          <span className="text-muted-foreground">Start:</span>
          <input
            type="number"
            step={0.01}
            min={0}
            max={trimEnd - 0.01}
            value={trimStart.toFixed(2)}
            onChange={(e) => onTrimStartChange(sample.id, parseFloat(e.target.value) || 0)}
            className="w-16 rounded border border-border bg-background px-1 py-0.5 text-xs"
          />
        </label>
        <span className="text-muted-foreground">&mdash;</span>
        <label className="flex items-center gap-1">
          <span className="text-muted-foreground">End:</span>
          <input
            type="number"
            step={0.01}
            min={trimStart + 0.01}
            max={sample.durationSeconds}
            value={trimEnd.toFixed(2)}
            onChange={(e) => onTrimEndChange(sample.id, parseFloat(e.target.value) || sample.durationSeconds)}
            className="w-16 rounded border border-border bg-background px-1 py-0.5 text-xs"
          />
        </label>
        <Button variant="ghost" size="sm" className="h-6 px-2 text-xs" onClick={() => onResetTrim(sample.id)}>
          <RotateCcw className="mr-1 h-3 w-3" />
          Reset
        </Button>
      </div>

      {/* Metadata */}
      <div className="grid grid-cols-2 gap-x-4 gap-y-1 text-xs text-muted-foreground">
        <div>Duration: <span className="text-foreground">{formatDuration(trimmedDuration)}</span></div>
        <div>Sample rate: <span className="text-foreground">{sample.sampleRate} Hz</span></div>
        <div>Channels: <span className="text-foreground">{sample.channels === 1 ? 'Mono' : 'Stereo'}</span></div>
        {sample.path && (
          <div className="col-span-2 truncate" title={sample.path}>
            Path: <span className="text-foreground">{sample.path}</span>
          </div>
        )}
      </div>

      {/* Actions */}
      <div className="flex items-center gap-2 pt-1">
        <Button
          variant="outline"
          size="sm"
          onClick={onToggleAudition}
          className="gap-1.5"
        >
          {isAuditioning ? <Square className="h-3.5 w-3.5" /> : <Play className="h-3.5 w-3.5" />}
          {isAuditioning ? 'Stop' : 'Audition'}
        </Button>

        <Button
          size="sm"
          onClick={onSendToArrangement}
          className="gap-1.5"
        >
          <SendHorizontal className="h-3.5 w-3.5" />
          Send to Arrangement
        </Button>

        <div className="flex-1" />

        <Button
          variant="ghost"
          size="sm"
          onClick={() => onRemove(sample.id)}
          className="gap-1.5 text-destructive hover:text-destructive"
        >
          <Trash2 className="h-3.5 w-3.5" />
          Remove
        </Button>
      </div>
    </div>
  );
}
