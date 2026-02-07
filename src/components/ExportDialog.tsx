import { useState, useCallback } from 'react';
import { Download, FileAudio, Music, X, Loader2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { arrangementEngine } from '@/services/ArrangementEngine';
import { exportWav, exportMidi, downloadBlob } from '@/services/ExportService';

interface ExportDialogProps {
  onClose: () => void;
}

export function ExportDialog({ onClose }: ExportDialogProps) {
  const [exporting, setExporting] = useState<'wav' | 'midi' | null>(null);
  const [error, setError] = useState<string | null>(null);

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

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm">
      <div className="relative w-80 rounded-lg border border-border bg-card p-5 shadow-2xl">
        <button
          className="absolute right-3 top-3 text-muted-foreground hover:text-foreground"
          onClick={onClose}
        >
          <X className="h-4 w-4" />
        </button>

        <h2 className="mb-4 text-sm font-bold">Export</h2>

        <div className="flex flex-col gap-2">
          <Button
            variant="outline"
            className="h-12 justify-start gap-3"
            onClick={handleExportWav}
            disabled={exporting !== null}
          >
            {exporting === 'wav' ? (
              <Loader2 className="h-5 w-5 animate-spin" />
            ) : (
              <FileAudio className="h-5 w-5 text-primary" />
            )}
            <div className="text-left">
              <div className="text-xs font-semibold">WAV Audio</div>
              <div className="text-[10px] text-muted-foreground">16-bit PCM, lossless</div>
            </div>
          </Button>

          <Button
            variant="outline"
            className="h-12 justify-start gap-3"
            onClick={handleExportMidi}
            disabled={exporting !== null}
          >
            {exporting === 'midi' ? (
              <Loader2 className="h-5 w-5 animate-spin" />
            ) : (
              <Music className="h-5 w-5 text-primary" />
            )}
            <div className="text-left">
              <div className="text-xs font-semibold">MIDI</div>
              <div className="text-[10px] text-muted-foreground">Standard MIDI file</div>
            </div>
          </Button>
        </div>

        {error && (
          <p className="mt-3 text-xs text-red-500">{error}</p>
        )}
      </div>
    </div>
  );
}
