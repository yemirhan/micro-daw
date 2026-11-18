import { useState } from 'react';
import { Check, ChevronDown, Plus } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { cn } from '@/lib/utils';
import type { DrumTrackInfo } from '@/hooks/useBeat';

export interface DrumTrackMenuProps {
  tracks: DrumTrackInfo[];
  activeTrackId: string | null;
  onSelect: (trackId: string) => void;
  onNew: () => void;
  className?: string;
}

/**
 * Which drum track the beat view edits (pads, steps, live hits, loop recording).
 * The trigger shows the active track's colour + name; the list offers every
 * drum track and "New drum track".
 */
export function DrumTrackMenu({ tracks, activeTrackId, onSelect, onNew, className }: DrumTrackMenuProps) {
  const [open, setOpen] = useState(false);
  const active = tracks.find((t) => t.id === activeTrackId) ?? null;
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          variant="ghost"
          size="xs"
          className={cn('max-w-[112px] gap-1 px-1.5 text-[12px] font-semibold', className)}
          aria-label={`Drum track: ${active?.label ?? 'none'}`}
          title="Drum track the pads and steps edit"
        >
          <span aria-hidden className="size-2 shrink-0 rounded-full" style={{ background: active?.color ?? 'var(--muted-foreground)' }} />
          <span className="truncate">{active?.label ?? 'No track'}</span>
          <ChevronDown className="size-3 text-muted-foreground" />
        </Button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-56 p-1">
        <div className="px-2 pt-1.5 pb-1 text-[11px] text-muted-foreground">Drum track</div>
        <div role="listbox" aria-label="Drum tracks">
          {tracks.map((t) => {
            const selected = t.id === activeTrackId;
            return (
              <button
                key={t.id}
                type="button"
                role="option"
                aria-selected={selected}
                className={cn(
                  'flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-xs outline-none hover:bg-surface-3 focus-visible:bg-surface-3',
                  selected && 'font-semibold',
                )}
                onClick={() => {
                  onSelect(t.id);
                  setOpen(false);
                }}
              >
                <span aria-hidden className="size-2 shrink-0 rounded-full" style={{ background: t.color }} />
                <span className="min-w-0 flex-1 truncate">{t.label}</span>
                {selected && <Check className="size-3.5 text-primary" />}
              </button>
            );
          })}
          {tracks.length === 0 && <div className="px-2 py-1 text-[11px] text-muted-foreground">No drum tracks yet.</div>}
        </div>
        <div className="my-1 h-px bg-border" />
        <button
          type="button"
          className="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-xs outline-none hover:bg-surface-3 focus-visible:bg-surface-3"
          onClick={() => {
            onNew();
            setOpen(false);
          }}
        >
          <Plus className="size-3.5 text-muted-foreground" />
          New drum track
        </button>
      </PopoverContent>
    </Popover>
  );
}
