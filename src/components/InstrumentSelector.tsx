import { useState, useMemo } from 'react';
import { SlidersHorizontal, ChevronDown } from 'lucide-react';
import { Button } from '@/components/ui/button';
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
import { SYNTH_PRESETS, PRESET_VIBES } from '@/utils/constants';
import type { PresetVibe } from '@/types/audio';

interface InstrumentSelectorProps {
  presetIndex: number;
  onChange: (index: number) => void;
  onToggleEditor?: () => void;
  editorOpen?: boolean;
}

export function InstrumentSelector({ presetIndex, onChange, onToggleEditor, editorOpen }: InstrumentSelectorProps) {
  const [open, setOpen] = useState(false);
  const [vibeFilter, setVibeFilter] = useState<PresetVibe | null>(null);

  const currentPreset = SYNTH_PRESETS[presetIndex] ?? SYNTH_PRESETS[0];

  const vibeGroups = useMemo(() => {
    const groups = new Map<PresetVibe, { index: number; name: string; category: string }[]>();
    SYNTH_PRESETS.forEach((preset, i) => {
      if (!groups.has(preset.vibe)) groups.set(preset.vibe, []);
      groups.get(preset.vibe)!.push({ index: i, name: preset.name, category: preset.category });
    });
    return groups;
  }, []);

  const handleSelect = (index: number) => {
    onChange(index);
    setOpen(false);
  };

  return (
    <div className="flex items-center gap-1.5">
      <span className="text-muted-foreground text-[11px] font-semibold uppercase tracking-wider">
        Synth
      </span>
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <Button
            variant="outline"
            size="sm"
            role="combobox"
            aria-expanded={open}
            className="w-[140px] justify-between h-7 text-xs font-medium"
          >
            <span className="truncate">{currentPreset.name}</span>
            <ChevronDown className="ml-1 h-3 w-3 shrink-0 opacity-50" />
          </Button>
        </PopoverTrigger>
        <PopoverContent className="w-[300px] p-0" align="start">
          <Command>
            <CommandInput placeholder="Search sounds..." className="h-8 text-xs" />

            {/* Vibe filter pills */}
            <div
              className="flex gap-1.5 px-2 py-2 border-b border-border/50 overflow-x-auto"
              style={{ scrollbarWidth: 'none', WebkitOverflowScrolling: 'touch' }}
            >
              <button
                className={`shrink-0 rounded-full px-2.5 py-1 text-[11px] font-medium transition-colors ${
                  vibeFilter === null
                    ? 'bg-primary text-primary-foreground shadow-sm'
                    : 'bg-muted/60 text-muted-foreground hover:bg-muted hover:text-foreground'
                }`}
                onClick={() => setVibeFilter(null)}
              >
                All
              </button>
              {PRESET_VIBES.map(({ id, label, emoji }) => {
                const count = vibeGroups.get(id)?.length ?? 0;
                if (count === 0) return null;
                return (
                  <button
                    key={id}
                    className={`shrink-0 rounded-full px-2.5 py-1 text-[11px] font-medium transition-colors ${
                      vibeFilter === id
                        ? 'bg-primary text-primary-foreground shadow-sm'
                        : 'bg-muted/60 text-muted-foreground hover:bg-muted hover:text-foreground'
                    }`}
                    onClick={() => setVibeFilter(vibeFilter === id ? null : id)}
                  >
                    {emoji} {label}
                  </button>
                );
              })}
            </div>

            <CommandList className="max-h-[280px]">
              <CommandEmpty>No sounds found.</CommandEmpty>
              {PRESET_VIBES.map(({ id, label, emoji }) => {
                const presets = vibeGroups.get(id);
                if (!presets || presets.length === 0) return null;
                if (vibeFilter !== null && vibeFilter !== id) return null;
                return (
                  <CommandGroup
                    key={id}
                    heading={`${emoji} ${label}`}
                    className="[&_[cmdk-group-heading]]:text-[11px] [&_[cmdk-group-heading]]:font-semibold [&_[cmdk-group-heading]]:uppercase [&_[cmdk-group-heading]]:tracking-wider [&_[cmdk-group-heading]]:text-muted-foreground/80 [&_[cmdk-group-heading]]:pt-2.5 [&_[cmdk-group-heading]]:pb-1 [&:not(:first-child)]:border-t [&:not(:first-child)]:border-border/30"
                  >
                    {presets.map(({ index, name, category }) => (
                      <CommandItem
                        key={index}
                        value={`${name} ${category} ${id}`}
                        onSelect={() => handleSelect(index)}
                        className="flex items-center justify-between"
                      >
                        <span className={index === presetIndex ? 'font-semibold' : ''}>{name}</span>
                        <span className="text-[10px] text-muted-foreground/60 uppercase">{category}</span>
                      </CommandItem>
                    ))}
                  </CommandGroup>
                );
              })}
            </CommandList>
          </Command>
        </PopoverContent>
      </Popover>
      {onToggleEditor && (
        <Button
          variant={editorOpen ? 'secondary' : 'ghost'}
          size="sm"
          className="h-6 w-6 p-0"
          onClick={onToggleEditor}
          title="Synth Editor"
        >
          <SlidersHorizontal className="h-3.5 w-3.5" />
        </Button>
      )}
    </div>
  );
}
