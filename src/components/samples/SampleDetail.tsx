import { useState, useEffect } from 'react';
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group';
import { InfoTab } from './InfoTab';
import { ProcessTab } from './ProcessTab';
import { GenerateTab } from './GenerateTab';
import type { SampleLibraryEntry, SampleProcessingParams, SignalGeneratorParams } from '@/types/samples';

type TabId = 'info' | 'process' | 'generate';

interface SampleDetailProps {
  entry: SampleLibraryEntry | null;
  isAuditioning: boolean;
  auditionProgress: number;
  isProcessing: boolean;
  onToggleAudition: () => void;
  onTrimStartChange: (id: string, seconds: number) => void;
  onTrimEndChange: (id: string, seconds: number) => void;
  onResetTrim: (id: string) => void;
  onRename: (id: string, name: string) => void;
  onSendToArrangement: () => void;
  onRemove: (id: string) => void;
  onProcess: (params: SampleProcessingParams) => void;
  onGenerate: (params: SignalGeneratorParams) => void;
}

export function SampleDetail({
  entry,
  isAuditioning,
  auditionProgress,
  isProcessing,
  onToggleAudition,
  onTrimStartChange,
  onTrimEndChange,
  onResetTrim,
  onRename,
  onSendToArrangement,
  onRemove,
  onProcess,
  onGenerate,
}: SampleDetailProps) {
  const [activeTab, setActiveTab] = useState<TabId>(entry ? 'info' : 'generate');

  // Force to generate tab when no entry selected
  useEffect(() => {
    if (!entry && activeTab !== 'generate') {
      setActiveTab('generate');
    }
  }, [entry, activeTab]);

  return (
    <div className="flex flex-col gap-4">
      {/* Tab selector */}
      <ToggleGroup
        type="single"
        value={activeTab}
        onValueChange={(v) => { if (v) setActiveTab(v as TabId); }}
        className="justify-start"
      >
        <ToggleGroupItem value="info" disabled={!entry} className="h-7 px-3 text-xs">
          Info
        </ToggleGroupItem>
        <ToggleGroupItem value="process" disabled={!entry} className="h-7 px-3 text-xs">
          Process
        </ToggleGroupItem>
        <ToggleGroupItem value="generate" className="h-7 px-3 text-xs">
          Generate
        </ToggleGroupItem>
      </ToggleGroup>

      {/* Tab content */}
      {activeTab === 'info' && entry && (
        <InfoTab
          entry={entry}
          isAuditioning={isAuditioning}
          auditionProgress={auditionProgress}
          onToggleAudition={onToggleAudition}
          onTrimStartChange={onTrimStartChange}
          onTrimEndChange={onTrimEndChange}
          onResetTrim={onResetTrim}
          onRename={onRename}
          onSendToArrangement={onSendToArrangement}
          onRemove={onRemove}
        />
      )}

      {activeTab === 'process' && entry && (
        <ProcessTab isProcessing={isProcessing} onProcess={onProcess} />
      )}

      {activeTab === 'generate' && (
        <GenerateTab isProcessing={isProcessing} onGenerate={onGenerate} />
      )}
    </div>
  );
}
