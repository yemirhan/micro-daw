import { AudioLines, Plus } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { EmptyState } from '@/components/ui/view-kit';
import { useSampleLibrary } from '@/hooks/useSampleLibrary';
import { SampleListItem } from './SampleListItem';
import { SampleDetail } from './SampleDetail';

interface SamplesViewProps {
  onSwitchToDAW: () => void;
}

export function SamplesView({ onSwitchToDAW }: SamplesViewProps) {
  const lib = useSampleLibrary();

  const handleSendToArrangement = () => {
    lib.sendToArrangement();
    onSwitchToDAW();
  };

  return (
    <div className="flex h-full flex-col overflow-hidden bg-background">
      {/* Toolbar */}
      <header className="flex h-12 shrink-0 items-center gap-3 border-b border-border bg-surface-1 px-4">
        <AudioLines className="size-4 text-muted-foreground" aria-hidden />
        <h2 className="text-sm font-semibold">Samples</h2>
        <span className="text-xs text-muted-foreground">Import, trim, process or generate audio for your arrangement</span>
        <Button size="sm" variant="outline" onClick={lib.importSamples} className="ml-auto h-7 gap-1.5 px-2.5 text-xs">
          <Plus className="size-3.5" />
          Import audio
        </Button>
      </header>

      <div className="flex min-h-0 flex-1">
        {/* Library */}
        <aside className="flex w-72 shrink-0 flex-col border-r border-border bg-surface-1" aria-label="Sample library">
          <div className="flex h-9 shrink-0 items-center justify-between border-b border-border px-3">
            <span className="text-[10.5px] font-bold uppercase tracking-[0.08em] text-muted-foreground">Library</span>
            <span className="font-mono text-[10.5px] tabular-nums text-muted-foreground">{lib.entries.length}</span>
          </div>
          <div className="flex-1 overflow-y-auto p-1.5">
            {lib.entries.length === 0 ? (
              <EmptyState icon={AudioLines} title="No samples yet">
                <p>Import WAV, MP3 or other audio files, or create a tone in the Generate tab.</p>
                <Button size="sm" variant="outline" onClick={lib.importSamples} className="mt-3 h-7 gap-1.5 text-xs">
                  <Plus className="size-3.5" />
                  Import audio
                </Button>
              </EmptyState>
            ) : (
              <div className="flex flex-col gap-0.5">
                {lib.entries.map((entry) => (
                  <SampleListItem
                    key={entry.sample.id}
                    entry={entry}
                    isSelected={lib.selectedId === entry.sample.id}
                    onSelect={() => lib.selectSample(entry.sample.id)}
                  />
                ))}
              </div>
            )}
          </div>
        </aside>

        {/* Detail */}
        <div className="flex min-w-0 flex-1 flex-col overflow-y-auto">
          <SampleDetail
            entry={lib.selectedEntry}
            isAuditioning={lib.isAuditioning}
            auditionProgress={lib.auditionProgress}
            isProcessing={lib.isProcessing}
            onToggleAudition={lib.toggleAudition}
            onTrimStartChange={lib.setTrimStart}
            onTrimEndChange={lib.setTrimEnd}
            onResetTrim={lib.resetTrim}
            onRename={lib.renameSample}
            onSendToArrangement={handleSendToArrangement}
            onRemove={lib.removeSample}
            onProcess={lib.processSample}
            onGenerate={lib.generateSample}
          />
        </div>
      </div>
    </div>
  );
}
