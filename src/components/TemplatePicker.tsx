import { useEffect, useState } from 'react';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';
import { ALL_TEMPLATES } from '@/data/templates';
import type { ProjectTemplate, TemplateCategory } from '@/types/templates';
import { LayoutTemplate, X } from 'lucide-react';

interface TemplatePickerProps {
  onSelect: (template: ProjectTemplate) => void;
  onClose: () => void;
}

const CATEGORY_LABELS: Record<TemplateCategory, string> = {
  starter: 'Starter',
  rock: 'Rock',
  'hip-hop': 'Hip-Hop',
  jazz: 'Jazz',
  edm: 'EDM',
  ambient: 'Ambient',
};

const CATEGORY_COLORS: Record<TemplateCategory, string> = {
  starter: 'oklch(0.65 0.16 195)',
  rock: 'oklch(0.62 0.22 25)',
  'hip-hop': 'oklch(0.62 0.22 310)',
  jazz: 'oklch(0.65 0.20 250)',
  edm: 'oklch(0.75 0.17 90)',
  ambient: 'oklch(0.65 0.20 155)',
};

const ALL_CATEGORIES: (TemplateCategory | 'all')[] = ['all', 'starter', 'rock', 'hip-hop', 'jazz', 'edm', 'ambient'];

export function TemplatePicker({ onSelect, onClose }: TemplatePickerProps) {
  const [filter, setFilter] = useState<TemplateCategory | 'all'>('all');

  const filtered = filter === 'all' ? ALL_TEMPLATES : ALL_TEMPLATES.filter((t) => t.category === filter);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        onClose();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-[oklch(0.1_0.004_65/0.72)] animate-in fade-in-0 duration-150"
      onPointerDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
      role="presentation"
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="template-title"
        className="relative flex max-h-[80vh] w-full max-w-[720px] flex-col overflow-hidden rounded-xl border border-border bg-surface-1 shadow-[0_24px_64px_-12px_oklch(0_0_0/0.7),inset_0_1px_0_oklch(1_0_0/0.05)] animate-in zoom-in-[0.98] fade-in-0 duration-150"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between border-b border-border px-5 py-3.5">
          <div>
            <h2 id="template-title" className="flex items-center gap-2 text-[15px] font-semibold tracking-tight">
              <LayoutTemplate className="size-4 text-primary" />
              New from template
            </h2>
            <p className="mt-0.5 text-[12px] text-muted-foreground">Start with tracks, sounds and a groove already set up.</p>
          </div>
          <Button variant="ghost" size="icon-sm" onClick={onClose} aria-label="Close">
            <X />
          </Button>
        </div>

        {/* Category tabs */}
        <div className="flex gap-1 border-b border-border px-5 py-2" role="tablist">
          {ALL_CATEGORIES.map((cat) => (
            <button
              key={cat}
              type="button"
              role="tab"
              aria-selected={filter === cat}
              className={cn(
                'h-7 rounded-md px-2.5 text-[12px] font-semibold transition-colors duration-150 focus-visible:outline-2 focus-visible:outline-ring',
                filter === cat ? 'bg-surface-3 text-foreground' : 'text-muted-foreground hover:bg-surface-2 hover:text-foreground',
              )}
              onClick={() => setFilter(cat)}
            >
              {cat === 'all' ? 'All' : CATEGORY_LABELS[cat]}
            </button>
          ))}
        </div>

        {/* Template grid */}
        <div className="grid grid-cols-2 gap-3 overflow-y-auto p-5">
          {filtered.map((template) => {
            const trackColors = (template.arrangement?.tracks ?? []).map((t) => t.color).slice(0, 6);
            return (
              <button
                key={template.id}
                className="group flex flex-col rounded-lg border border-border bg-surface-2 p-4 text-left transition-colors duration-150 hover:border-primary/50 hover:bg-surface-3 focus-visible:outline-2 focus-visible:outline-ring"
                onClick={() => onSelect(template)}
              >
                <div className="flex items-start justify-between gap-2">
                  <h3 className="text-[14px] font-semibold text-foreground">{template.name}</h3>
                  <span
                    className="shrink-0 rounded-sm px-1.5 py-0.5 text-[10px] font-bold tracking-[0.04em]"
                    style={{
                      backgroundColor: `color-mix(in oklab, ${CATEGORY_COLORS[template.category]} 18%, transparent)`,
                      color: `color-mix(in oklab, ${CATEGORY_COLORS[template.category]} 85%, white)`,
                    }}
                  >
                    {CATEGORY_LABELS[template.category]}
                  </span>
                </div>
                <p className="mt-1 text-[12px] leading-relaxed text-muted-foreground">{template.description}</p>
                {trackColors.length > 0 && (
                  <div className="mt-3 flex flex-col gap-[3px]" aria-hidden>
                    {trackColors.map((c, i) => (
                      <span
                        key={i}
                        className="h-[5px] rounded-full"
                        style={{ backgroundColor: c, width: `${55 + ((i * 37) % 45)}%`, opacity: 0.85 }}
                      />
                    ))}
                  </div>
                )}
                <div className="mt-3 flex items-center gap-3 font-mono text-[11px] text-muted-foreground">
                  <span>{template.bpm} BPM</span>
                  <span className="size-1 rounded-full bg-border" />
                  <span>{template.trackCount} tracks</span>
                </div>
              </button>
            );
          })}
        </div>
      </div>
    </div>
  );
}
