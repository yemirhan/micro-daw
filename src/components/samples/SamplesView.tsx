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

      {/* Content — always show two-panel layout */}
      <div className="flex min-h-0 flex-1">
        {/* Sample list */}
        <div className="flex w-56 shrink-0 flex-col border-r border-border/50">
          <div className="overflow-y-auto p-2">
            {lib.entries.length === 0 ? (
              <div className="flex flex-col items-center gap-2 py-8 text-center">
                <AudioLines className="h-8 w-8 text-muted-foreground/40" />
                <p className="text-xs text-muted-foreground">No samples yet</p>
                <Button size="sm" variant="outline" onClick={lib.importSamples} className="gap-1.5 text-xs">
                  <Plus className="h-3 w-3" />
                  Import
                </Button>
              </div>
            ) : (
              lib.entries.map((entry) => (
                <SampleListItem
                  key={entry.sample.id}
                  entry={entry}
                  isSelected={lib.selectedId === entry.sample.id}
                  onSelect={() => lib.selectSample(entry.sample.id)}
                />
              ))
            )}
          </div>
        </div>

        {/* Detail panel — always rendered */}
        <div className="flex min-w-0 flex-1 flex-col overflow-y-auto p-4">
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
