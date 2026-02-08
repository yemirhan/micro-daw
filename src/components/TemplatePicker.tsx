import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { ALL_TEMPLATES } from '@/data/templates';
import type { ProjectTemplate, TemplateCategory } from '@/types/templates';
import { Music, X } from 'lucide-react';

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

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm">
      <div
        className="relative w-full max-w-2xl rounded-lg border border-border bg-card shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between border-b border-border px-5 py-3">
          <h2 className="flex items-center gap-2 text-sm font-semibold">
            <Music className="h-4 w-4" />
            New from Template
          </h2>
          <Button variant="ghost" size="sm" className="h-7 w-7 p-0" onClick={onClose}>
            <X className="h-4 w-4" />
          </Button>
        </div>

        {/* Category tabs */}
        <div className="flex gap-1 border-b border-border px-5 py-2">
          {ALL_CATEGORIES.map((cat) => (
            <Button
              key={cat}
              variant={filter === cat ? 'default' : 'ghost'}
              size="sm"
              className="h-6 px-2 text-[11px]"
              onClick={() => setFilter(cat)}
            >
              {cat === 'all' ? 'All' : CATEGORY_LABELS[cat]}
            </Button>
          ))}
        </div>

        {/* Template grid */}
        <div className="grid grid-cols-2 gap-3 p-5 max-h-[400px] overflow-y-auto">
          {filtered.map((template) => (
            <button
              key={template.id}
              className="group flex flex-col rounded-lg border border-border/50 bg-background/50 p-4 text-left transition-all hover:border-primary/40 hover:bg-primary/5 hover:shadow-md"
              onClick={() => onSelect(template)}
            >
              <div className="flex items-start justify-between">
                <h3 className="text-sm font-semibold text-foreground">{template.name}</h3>
                <Badge
                  className="text-[9px] px-1.5 py-0"
                  style={{
                    backgroundColor: `color-mix(in oklch, ${CATEGORY_COLORS[template.category]} 20%, transparent)`,
                    color: CATEGORY_COLORS[template.category],
                    borderColor: CATEGORY_COLORS[template.category],
                  }}
                >
                  {CATEGORY_LABELS[template.category]}
                </Badge>
              </div>
              <p className="mt-1 text-[11px] text-muted-foreground leading-relaxed">
                {template.description}
              </p>
              <div className="mt-2 flex items-center gap-3 text-[10px] text-muted-foreground">
                <span>{template.bpm} BPM</span>
                <span>{template.trackCount} tracks</span>
              </div>
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
