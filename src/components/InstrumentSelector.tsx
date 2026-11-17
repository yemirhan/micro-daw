import { useState, useMemo, type ReactNode } from 'react';
import { SlidersHorizontal, ChevronDown } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Tip } from '@/components/ui/tip';
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '@/components/ui/popover';
import {
  Command,
  CommandInput,
  CommandList,
  CommandEmpty,
  CommandGroup,
  CommandItem,
} from '@/components/ui/command';
import { SYNTH_PRESETS, PRESET_VIBES, PRESET_CATEGORIES } from '@/utils/constants';
import type { PresetCategory, PresetVibe } from '@/types/audio';

interface InstrumentSelectorProps {
  presetIndex: number;
  onChange: (index: number) => void;
  onToggleEditor?: () => void;
  editorOpen?: boolean;
}

const VIBE_LABEL = new Map(PRESET_VIBES.map((v) => [v.id, v.label]));

interface Entry {
  index: number;
  name: string;
  vibe: PresetVibe;
  description?: string;
}

export function InstrumentSelector({ presetIndex, onChange, onToggleEditor, editorOpen }: InstrumentSelectorProps) {
  const [open, setOpen] = useState(false);
  const [vibeFilter, setVibeFilter] = useState<PresetVibe | null>(null);

  const currentPreset = SYNTH_PRESETS[presetIndex] ?? SYNTH_PRESETS[0];

  const { byCategory, vibeCounts } = useMemo(() => {
    const groups = new Map<PresetCategory, Entry[]>();
    const counts = new Map<PresetVibe, number>();
    SYNTH_PRESETS.forEach((preset, index) => {
      if (!groups.has(preset.category)) groups.set(preset.category, []);
      groups.get(preset.category)!.push({ index, name: preset.name, vibe: preset.vibe, description: preset.description });
      counts.set(preset.vibe, (counts.get(preset.vibe) ?? 0) + 1);
    });
    return { byCategory: groups, vibeCounts: counts };
  }, []);

  const handleSelect = (index: number) => {
    onChange(index);
    setOpen(false);
  };

  const categoryLabel = PRESET_CATEGORIES.find((c) => c.id === currentPreset.category)?.label;

  return (
    <div className="flex items-center gap-1" data-testid="instrument-selector">
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <Button
            variant="outline"
            size="sm"
            role="combobox"
            aria-expanded={open}
            aria-label="Choose instrument sound"
            className="w-[150px] justify-between gap-2 px-2 text-xs min-[1360px]:w-[184px] data-[state=open]:border-input data-[state=open]:bg-surface-3"
          >
            <span className="flex min-w-0 items-center gap-2">
              <span className="hidden shrink-0 rounded-[3px] bg-surface-4 px-1 py-px text-[9.5px] font-bold uppercase tracking-[0.06em] text-muted-foreground min-[1360px]:inline">
                {categoryLabel ?? 'Sound'}
              </span>
              <span className="truncate font-semibold text-foreground">{currentPreset.name}</span>
            </span>
            <ChevronDown className="size-3.5 shrink-0 text-muted-foreground" />
          </Button>
        </PopoverTrigger>
        <PopoverContent className="w-[340px] p-0" align="end">
          <Command>
            <CommandInput placeholder="Search sounds…" className="h-10 text-[13px]" />

            {/* Vibe filter chips */}
            <div
              className="flex gap-1 overflow-x-auto border-b border-border px-2 py-2"
              style={{ scrollbarWidth: 'none' }}
            >
              <VibeChip active={vibeFilter === null} onClick={() => setVibeFilter(null)}>
                All
              </VibeChip>
              {PRESET_VIBES.map(({ id, label }) => {
                if (!vibeCounts.get(id)) return null;
                return (
                  <VibeChip key={id} active={vibeFilter === id} onClick={() => setVibeFilter(vibeFilter === id ? null : id)}>
                    {label}
                  </VibeChip>
                );
              })}
            </div>

            <CommandList className="max-h-[360px]">
              <CommandEmpty>No sounds found.</CommandEmpty>
              {PRESET_CATEGORIES.map(({ id, label }) => {
                const presets = (byCategory.get(id) ?? []).filter((p) => vibeFilter === null || p.vibe === vibeFilter);
                if (presets.length === 0) return null;
                return (
                  <CommandGroup key={id} heading={label} className="[&:not(:first-child)]:border-t [&:not(:first-child)]:border-border/60">
                    {presets.map(({ index, name, vibe, description }) => {
                      const selected = index === presetIndex;
                      return (
                        <CommandItem
                          key={index}
                          value={`${name} ${label} ${vibe} ${description ?? ''}`}
                          onSelect={() => handleSelect(index)}
                          className="flex items-center justify-between gap-2"
                          title={description}
                          data-current={selected || undefined}
                        >
                          <span className="flex min-w-0 items-center gap-2">
                            <span
                              className={`size-1.5 shrink-0 rounded-full ${selected ? 'bg-primary' : 'bg-transparent'}`}
                              aria-hidden
                            />
                            <span className={`truncate ${selected ? 'font-semibold text-primary' : ''}`}>{name}</span>
                          </span>
                          <span className="shrink-0 text-[10.5px] capitalize text-muted-foreground" aria-hidden>
                            {VIBE_LABEL.get(vibe) ?? vibe}
                          </span>
                        </CommandItem>
                      );
                    })}
                  </CommandGroup>
                );
              })}
            </CommandList>
          </Command>
        </PopoverContent>
      </Popover>
      {onToggleEditor && (
        <Tip label={editorOpen ? 'Hide synth editor' : 'Edit sound'}>
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={onToggleEditor}
            aria-pressed={!!editorOpen}
            aria-label="Toggle synth editor"
            data-testid="synth-editor-toggle"
            className={editorOpen ? 'bg-primary/15 text-primary hover:bg-primary/20 hover:text-primary' : undefined}
          >
            <SlidersHorizontal />
          </Button>
        </Tip>
      )}
    </div>
  );
}

function VibeChip({ active, onClick, children }: { active: boolean; onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      className={`h-6 shrink-0 rounded-sm px-2 text-[11px] font-semibold transition-colors duration-150 focus-visible:outline-2 focus-visible:outline-ring ${
        active
          ? 'bg-primary text-primary-foreground'
          : 'bg-surface-3 text-muted-foreground hover:bg-surface-4 hover:text-foreground'
      }`}
      aria-pressed={active}
      onClick={onClick}
    >
      {children}
    </button>
  );
}
