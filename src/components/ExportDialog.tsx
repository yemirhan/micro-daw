import { useState, useCallback, useEffect } from 'react';
import { FileAudio, Music, X, Loader2, ChevronRight } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { arrangementEngine } from '@/services/ArrangementEngine';
import { exportWav, exportMidi, downloadBlob } from '@/services/ExportService';

interface ExportDialogProps {
  onClose: () => void;
}

export function ExportDialog({ onClose }: ExportDialogProps) {
  const [exporting, setExporting] = useState<'wav' | 'midi' | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Escape closes (unless an export is running).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && !exporting) {
        e.preventDefault();
        onClose();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [exporting, onClose]);

  const handleExportWav = useCallback(async () => {
    setExporting('wav');
    setError(null);
    try {
      const arrangement = arrangementEngine.getArrangement();
      const blob = await exportWav(arrangement);
      const name = arrangement.name || 'untitled';
      downloadBlob(blob, `${name}.wav`);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Export failed');
    } finally {
      setExporting(null);
    }
  }, []);

  const handleExportMidi = useCallback(async () => {
    setExporting('midi');
    setError(null);
    try {
      const arrangement = arrangementEngine.getArrangement();
      const blob = await exportMidi(arrangement);
      const name = arrangement.name || 'untitled';
      downloadBlob(blob, `${name}.mid`);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Export failed');
    } finally {
      setExporting(null);
    }
  }, []);

  const tracks = arrangementEngine.getTracks();
  const audible = tracks.filter((t) => !t.muted).length;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-[oklch(0.1_0.004_65/0.72)] animate-in fade-in-0 duration-150"
      onPointerDown={(e) => {
        if (e.target === e.currentTarget && !exporting) onClose();
      }}
      role="presentation"
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="export-title"
        className="relative w-[360px] rounded-xl border border-border bg-surface-1 p-5 shadow-[0_24px_64px_-12px_oklch(0_0_0/0.7),inset_0_1px_0_oklch(1_0_0/0.05)] animate-in zoom-in-[0.98] fade-in-0 duration-150"
      >
        <Button variant="ghost" size="icon-sm" className="absolute top-3 right-3" onClick={onClose} aria-label="Close" disabled={exporting !== null}>
          <X />
        </Button>

        <h2 id="export-title" className="text-[15px] font-semibold tracking-tight">Export</h2>
        <p className="mt-1 mb-4 text-[12px] text-muted-foreground">
          {tracks.length === 0
            ? 'Nothing to export yet. Add a track first.'
            : `${audible} of ${tracks.length} track${tracks.length === 1 ? '' : 's'} will be included (muted tracks are skipped).`}
        </p>

        <div className="flex flex-col gap-2">
          {([
            { id: 'wav', icon: FileAudio, title: 'WAV audio', sub: '16-bit PCM stereo, rendered offline', run: handleExportWav },
            { id: 'midi', icon: Music, title: 'MIDI file', sub: 'Standard MIDI; drums on channel 10', run: handleExportMidi },
          ] as const).map(({ id, icon: Icon, title, sub, run }) => (
            <button
              key={id}
              type="button"
              onClick={run}
              disabled={exporting !== null || tracks.length === 0}
              className="group flex h-14 items-center gap-3 rounded-lg border border-border bg-surface-2 px-3 text-left transition-colors duration-150 hover:border-input hover:bg-surface-3 focus-visible:outline-2 focus-visible:outline-ring disabled:opacity-45"
            >
              <span className="flex size-8 items-center justify-center rounded-md bg-primary/12 text-primary">
                {exporting === id ? <Loader2 className="size-4 animate-spin" /> : <Icon className="size-4" />}
              </span>
              <span className="flex-1">
                <span className="block text-[13px] font-semibold">{exporting === id ? 'Exporting…' : title}</span>
                <span className="block text-[11px] text-muted-foreground">{sub}</span>
              </span>
              <ChevronRight className="size-4 text-muted-foreground transition-transform duration-150 group-hover:translate-x-0.5" />
            </button>
          ))}
        </div>

        {error && (
          <p className="mt-3 rounded-md border border-signal-rec/40 bg-signal-rec/10 px-2.5 py-1.5 text-[12px] text-signal-rec" role="alert">
            {error}
          </p>
        )}
      </div>
    </div>
  );
}
