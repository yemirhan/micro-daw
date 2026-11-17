import { useState } from 'react';
import { Info, SlidersHorizontal, Wand2, type LucideIcon } from 'lucide-react';
import { cn } from '@/lib/utils';
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

const TABS: { id: TabId; label: string; icon: LucideIcon; needsSample: boolean }[] = [
  { id: 'info', label: 'Edit', icon: Info, needsSample: true },
  { id: 'process', label: 'Process', icon: SlidersHorizontal, needsSample: true },
  { id: 'generate', label: 'Generate', icon: Wand2, needsSample: false },
];

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
  const [selectedTab, setActiveTab] = useState<TabId>(entry ? 'info' : 'generate');
  // Without a sample only Generate is available.
  const activeTab: TabId = entry ? selectedTab : 'generate';

  return (
    <div className="flex flex-col">
      {/* Tab bar */}
      <div role="tablist" aria-label="Sample tools" className="flex h-9 shrink-0 items-end gap-1 border-b border-border px-4">
        {TABS.map((tab) => {
          const disabled = tab.needsSample && !entry;
          const active = activeTab === tab.id;
          return (
            <button
              key={tab.id}
              type="button"
              role="tab"
              aria-selected={active}
              disabled={disabled}
              onClick={() => setActiveTab(tab.id)}
              className={cn(
                'relative -mb-px inline-flex h-9 items-center gap-1.5 border-b-2 px-3 text-xs font-semibold transition-colors duration-150',
                'outline-none focus-visible:ring-2 focus-visible:ring-ring/60 disabled:cursor-not-allowed disabled:opacity-40',
                active ? 'border-primary text-foreground' : 'border-transparent text-muted-foreground hover:text-foreground',
              )}
            >
              <tab.icon className="size-3.5" aria-hidden />
              {tab.label}
            </button>
          );
        })}
      </div>

      <div className="w-full max-w-4xl px-6 py-5">
        {activeTab === 'info' && entry && (
          <InfoTab
            key={entry.sample.id}
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

        {activeTab === 'process' && entry && <ProcessTab isProcessing={isProcessing} onProcess={onProcess} />}

        {activeTab === 'generate' && <GenerateTab isProcessing={isProcessing} onGenerate={onGenerate} />}
      </div>
    </div>
  );
}
