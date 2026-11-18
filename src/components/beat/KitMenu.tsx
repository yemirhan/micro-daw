import { useState } from 'react';
import { ChevronDown, Drum, Package, Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { cn } from '@/lib/utils';
import type { KitChoice } from '@/components/packs/useKitSources';
import type { KitState } from '@/types/sampler';

export interface KitMenuProps {
  kitName: string;
  userKits: KitState[];
  onLoadFactoryKit: () => void;
  onLoadKit: (kit: KitState) => void;
  onSaveKit: (name: string) => void;
  onDeleteKit?: (id: string) => void;
  /** Built-in classic-machine kits (listed after Factory). */
  builtInKits?: KitChoice[];
  /** Kits of installed packs. */
  packKits?: KitChoice[];
  /** Opens the Packs page. */
  onBrowsePacks?: () => void;
  className?: string;
}

function GroupLabel({ children }: { children: React.ReactNode }) {
  return <div className="px-2 pt-2 pb-1 text-[11px] text-muted-foreground first:pt-1.5">{children}</div>;
}

function ChoiceRow({ choice, onPick }: { choice: KitChoice; onPick: (c: KitChoice) => void }) {
  return (
    <button
      type="button"
      className="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-xs hover:bg-surface-3 focus-visible:bg-surface-3 focus-visible:outline-none"
      onClick={() => onPick(choice)}
    >
      <span aria-hidden className="size-2 shrink-0 rounded-[2px]" style={{ backgroundColor: choice.color ?? 'var(--surface-4)' }} />
      <span className="min-w-0 flex-1 truncate">{choice.name}</span>
      {choice.detail && <span className="max-w-[45%] truncate text-[10.5px] text-muted-foreground">{choice.detail}</span>}
    </button>
  );
}

/** Kit picker: the kit's name as a button; load Factory / a saved kit, save the current one. */
export function KitMenu({
  kitName,
  userKits,
  onLoadFactoryKit,
  onLoadKit,
  onSaveKit,
  onDeleteKit,
  builtInKits = [],
  packKits = [],
  onBrowsePacks,
  className,
}: KitMenuProps) {
  const [open, setOpen] = useState(false);
  const [name, setName] = useState('');
  const pick = (choice: KitChoice) => {
    const kit = choice.create();
    if (kit) onLoadKit(kit);
    setOpen(false);
  };
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
      <PopoverContent align="start" className="w-72 p-1">
        <div className="max-h-[min(70vh,540px)] overflow-y-auto">
        <GroupLabel>Built-in</GroupLabel>
        <button
          type="button"
          className="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-xs hover:bg-surface-3 focus-visible:bg-surface-3 focus-visible:outline-none"
          onClick={() => {
            onLoadFactoryKit();
            setOpen(false);
          }}
        >
          <span aria-hidden className="size-2 shrink-0 rounded-[2px] bg-primary" />
          Factory
        </button>
        {builtInKits.map((c) => (
          <ChoiceRow key={c.id} choice={c} onPick={pick} />
        ))}
        {packKits.length > 0 && <GroupLabel>Installed packs</GroupLabel>}
        {packKits.map((c) => (
          <ChoiceRow key={c.id} choice={c} onPick={pick} />
        ))}
        <GroupLabel>Your kits</GroupLabel>
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
        {userKits.length === 0 && <div className="px-2 py-1 text-[11px] text-muted-foreground">Saved kits appear here.</div>}
        </div>
        {onBrowsePacks && (
          <>
            <div className="my-1 h-px bg-border" />
            <button
              type="button"
              className="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-xs hover:bg-surface-3 focus-visible:bg-surface-3 focus-visible:outline-none"
              onClick={() => {
                setOpen(false);
                onBrowsePacks();
              }}
            >
              <Package className="size-3.5 text-muted-foreground" aria-hidden />
              Browse packs…
            </button>
          </>
        )}
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
