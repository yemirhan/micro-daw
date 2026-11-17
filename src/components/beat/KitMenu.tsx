import { useState } from 'react';
import { ChevronDown, Drum, Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { cn } from '@/lib/utils';
import type { KitState } from '@/types/sampler';

export interface KitMenuProps {
  kitName: string;
  userKits: KitState[];
  onLoadFactoryKit: () => void;
  onLoadKit: (kit: KitState) => void;
  onSaveKit: (name: string) => void;
  onDeleteKit?: (id: string) => void;
  className?: string;
}

/** Kit picker: the kit's name as a button; load Factory / a saved kit, save the current one. */
export function KitMenu({
  kitName,
  userKits,
  onLoadFactoryKit,
  onLoadKit,
  onSaveKit,
  onDeleteKit,
  className,
}: KitMenuProps) {
  const [open, setOpen] = useState(false);
  const [name, setName] = useState('');
  return (
    <Popover
      open={open}
      onOpenChange={(o) => {
        setOpen(o);
        if (o) setName(kitName === 'Factory' ? '' : kitName);
      }}
    >
      <PopoverTrigger asChild>
        <Button variant="ghost" size="xs" className={cn('max-w-[150px] gap-1 px-1.5 text-[12px] font-semibold', className)} aria-label={`Kit: ${kitName}`}>
          <Drum className="text-muted-foreground" />
          <span className="truncate">{kitName}</span>
          <ChevronDown className="size-3 text-muted-foreground" />
        </Button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-64 p-1">
        <div className="px-2 pt-1.5 pb-1 text-[10.5px] text-muted-foreground">Load kit</div>
        <button
          type="button"
          className="flex w-full items-center rounded-sm px-2 py-1.5 text-left text-xs hover:bg-surface-3"
          onClick={() => {
            onLoadFactoryKit();
            setOpen(false);
          }}
        >
          Factory
        </button>
        {userKits.map((k) => (
          <div key={k.id} className="group/kit flex items-center rounded-sm hover:bg-surface-3">
            <button
              type="button"
              className="min-w-0 flex-1 truncate px-2 py-1.5 text-left text-xs"
              onClick={() => {
                onLoadKit(k);
                setOpen(false);
              }}
            >
              {k.name}
            </button>
            {onDeleteKit && (
              <button
                type="button"
                aria-label={`Delete kit ${k.name}`}
                className="mr-1 rounded-sm p-1 text-muted-foreground opacity-0 group-hover/kit:opacity-100 hover:text-signal-rec focus-visible:opacity-100"
                onClick={() => onDeleteKit(k.id)}
              >
                <Trash2 className="size-3.5" />
              </button>
            )}
          </div>
        ))}
        {userKits.length === 0 && <div className="px-2 py-1 text-[11px] text-muted-foreground/70">Saved kits appear here.</div>}
        <div className="my-1 h-px bg-border" />
        <form
          className="flex gap-1 p-1"
          onSubmit={(e) => {
            e.preventDefault();
            onSaveKit(name);
            setOpen(false);
          }}
        >
          <input
            aria-label="Kit name"
            placeholder="Kit name"
            value={name}
            onChange={(e) => setName(e.target.value)}
            className="h-7 min-w-0 flex-1 rounded-sm border border-input bg-surface-0 px-2 text-xs outline-none focus:border-ring"
          />
          <Button type="submit" size="sm">Save kit</Button>
        </form>
      </PopoverContent>
    </Popover>
  );
}
