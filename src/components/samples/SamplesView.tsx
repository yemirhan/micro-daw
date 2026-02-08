import { AudioLines, Plus } from 'lucide-react';
import { Button } from '@/components/ui/button';
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
    <div className="flex h-full flex-col overflow-hidden">
      {/* Header */}
      <div className="flex items-center justify-between border-b border-border/50 px-4 py-3">
        <div>
          <h2 className="flex items-center gap-2 text-sm font-semibold">
            <AudioLines className="h-4 w-4" />
            Samples
          </h2>
          <p className="text-xs text-muted-foreground">
            Import and manage audio samples for your projects
          </p>
        </div>
        <Button size="sm" variant="outline" onClick={lib.importSamples} className="gap-1.5">
          <Plus className="h-3.5 w-3.5" />
          Import
        </Button>
      </div>

      {/* Content */}
      {lib.entries.length === 0 ? (
        /* Empty state */
        <div className="flex flex-1 flex-col items-center justify-center gap-3 p-8 text-center">
          <AudioLines className="h-12 w-12 text-muted-foreground/40" />
          <div>
            <p className="text-sm font-medium">No samples yet</p>
            <p className="text-xs text-muted-foreground">
              Import WAV or MP3 files to build your sample library
            </p>
          </div>
          <Button size="sm" onClick={lib.importSamples} className="gap-1.5">
            <Plus className="h-3.5 w-3.5" />
            Import Samples
          </Button>
        </div>
      ) : (
        /* Two-panel layout */
        <div className="flex min-h-0 flex-1">
          {/* Sample list */}
          <div className="flex w-56 shrink-0 flex-col border-r border-border/50">
            <div className="overflow-y-auto p-2">
              {lib.entries.map((entry) => (
                <SampleListItem
                  key={entry.sample.id}
                  entry={entry}
                  isSelected={lib.selectedId === entry.sample.id}
                  onSelect={() => lib.selectSample(entry.sample.id)}
                />
              ))}
            </div>
          </div>

          {/* Detail panel */}
          <div className="flex min-w-0 flex-1 flex-col overflow-y-auto p-4">
            {lib.selectedEntry ? (
              <SampleDetail
                entry={lib.selectedEntry}
                isAuditioning={lib.isAuditioning}
                auditionProgress={lib.auditionProgress}
                onToggleAudition={lib.toggleAudition}
                onTrimStartChange={lib.setTrimStart}
                onTrimEndChange={lib.setTrimEnd}
                onResetTrim={lib.resetTrim}
                onRename={lib.renameSample}
                onSendToArrangement={handleSendToArrangement}
                onRemove={lib.removeSample}
              />
            ) : (
              <div className="flex flex-1 items-center justify-center text-xs text-muted-foreground">
                Select a sample to view details
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
