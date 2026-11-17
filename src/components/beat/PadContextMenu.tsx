import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { PAD_COLORS } from '@/audio/sampler/factoryKit';
import { cn } from '@/lib/utils';
import type { PadIndex, PadSettings } from '@/types/sampler';
import { padLabel, padName } from './beatHelpers';

export interface PadMenuActions {
  onClear?: (pad: PadIndex) => void;
  onRename?: (pad: PadIndex, name: string) => void;
  onColor?: (pad: PadIndex, color: string | undefined) => void;
  onCopy?: (pad: PadIndex) => void;
  onPaste?: (pad: PadIndex) => void;
  onChop?: (pad: PadIndex) => void;
  onChokeGroup?: (pad: PadIndex, group: number) => void;
}

interface PadContextMenuProps extends PadMenuActions {
  pad: PadIndex;
  settings: PadSettings;
  x: number;
  y: number;
  canPaste: boolean;
  onClose: () => void;
}

const CHOKE_GROUPS = [0, 1, 2, 3, 4];

function MenuItem({ children, onClick, disabled, destructive }: { children: React.ReactNode; onClick: () => void; disabled?: boolean; destructive?: boolean }) {
  return (
    <button
      type="button"
      role="menuitem"
      disabled={disabled}
      onClick={onClick}
      className={cn(
        'flex w-full items-center rounded-sm px-2 py-1.5 text-left text-xs outline-none hover:bg-surface-3 focus-visible:bg-surface-3 disabled:opacity-40 disabled:hover:bg-transparent',
        destructive ? 'text-signal-rec' : 'text-popover-foreground',
      )}
    >
      {children}
    </button>
  );
}

/** Right-click menu of a pad: rename, color, choke group, copy/paste, chop, clear. */
export function PadContextMenu({
  pad,
  settings,
  x,
  y,
  canPaste,
  onClose,
  onClear,
  onRename,
  onColor,
  onCopy,
  onPaste,
  onChop,
  onChokeGroup,
}: PadContextMenuProps) {
  const ref = useRef<HTMLDivElement>(null);
  const [renaming, setRenaming] = useState(false);
  const [draft, setDraft] = useState(padName(settings));
  const [pos, setPos] = useState({ x, y });
  const hasSample = !!settings.sample;

  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('mousedown', onDown);
    window.addEventListener('keydown', onKey);
    return () => {
      window.removeEventListener('mousedown', onDown);
      window.removeEventListener('keydown', onKey);
    };
  }, [onClose]);

  // Keep the menu on screen.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    const nx = Math.max(4, Math.min(x, window.innerWidth - r.width - 4));
    const ny = Math.max(4, Math.min(y, window.innerHeight - r.height - 4));
    if (nx !== pos.x || ny !== pos.y) setPos({ x: nx, y: ny });
  }, [x, y, pos.x, pos.y]);

  const run = (fn: (() => void) | undefined) => () => {
    fn?.();
    onClose();
  };

  return (
    <div
      ref={ref}
      role="menu"
      aria-label={`Pad ${padLabel(pad)}`}
      className="fixed z-50 w-[208px] rounded-md border border-border bg-popover p-1 shadow-[0_16px_40px_-10px_oklch(0_0_0/0.65)]"
      style={{ left: pos.x, top: pos.y }}
      onContextMenu={(e) => e.preventDefault()}
    >
      <div className="flex items-baseline justify-between px-2 pt-1 pb-1.5">
        <span className="truncate text-xs font-semibold text-foreground">{padName(settings) || 'Empty pad'}</span>
        <span className="font-mono text-[10px] text-muted-foreground">{padLabel(pad)}</span>
      </div>

      {renaming ? (
        <form
          className="px-1 pb-1"
          onSubmit={(e) => {
            e.preventDefault();
            onRename?.(pad, draft);
            onClose();
          }}
        >
          <input
            autoFocus
            aria-label="Pad name"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onFocus={(e) => e.currentTarget.select()}
            className="h-7 w-full rounded-sm border border-input bg-surface-0 px-2 text-xs text-foreground outline-none focus:border-ring"
          />
        </form>
      ) : (
        onRename && (
          <MenuItem disabled={!hasSample} onClick={() => setRenaming(true)}>
            Rename…
          </MenuItem>
        )
      )}

      {onColor && (
        <div className="px-2 py-1.5">
          <div className="mb-1 text-[10.5px] text-muted-foreground">Color</div>
          <div className="grid grid-cols-8 gap-1">
            {PAD_COLORS.map((c) => (
              <button
                key={c}
                type="button"
                aria-label={`Color ${c}`}
                onClick={run(() => onColor(pad, c))}
                className={cn(
                  'size-[18px] rounded-[3px] outline-none ring-offset-1 ring-offset-popover focus-visible:ring-2 focus-visible:ring-ring',
                  settings.color === c && 'ring-2 ring-foreground',
                )}
                style={{ background: c }}
              />
            ))}
          </div>
          {settings.color && (
            <button type="button" className="mt-1 text-[10.5px] text-muted-foreground hover:text-foreground" onClick={run(() => onColor(pad, undefined))}>
              Reset color
            </button>
          )}
        </div>
      )}

      {onChokeGroup && (
        <div className="px-2 py-1.5">
          <div className="mb-1 text-[10.5px] text-muted-foreground">Choke group</div>
          <div className="flex gap-1" role="radiogroup" aria-label="Choke group">
            {CHOKE_GROUPS.map((g) => (
              <button
                key={g}
                type="button"
                role="radio"
                aria-checked={settings.chokeGroup === g}
                onClick={run(() => onChokeGroup(pad, g))}
                className={cn(
                  'h-6 flex-1 rounded-sm border font-mono text-[11px] outline-none focus-visible:ring-2 focus-visible:ring-ring',
                  settings.chokeGroup === g
                    ? 'border-transparent bg-foreground text-background'
                    : 'border-border bg-surface-2 text-muted-foreground hover:text-foreground',
                )}
              >
                {g === 0 ? 'Off' : g}
              </button>
            ))}
          </div>
        </div>
      )}

      <div className="my-1 h-px bg-border" />
      {onCopy && <MenuItem onClick={run(() => onCopy(pad))}>Copy pad</MenuItem>}
      {onPaste && (
        <MenuItem disabled={!canPaste} onClick={run(() => onPaste(pad))}>
          Paste pad
        </MenuItem>
      )}
      {onChop && (
        <MenuItem disabled={!hasSample} onClick={run(() => onChop(pad))}>
          Chop to pads…
        </MenuItem>
      )}
      {onClear && (
        <>
          <div className="my-1 h-px bg-border" />
          <MenuItem destructive disabled={!hasSample} onClick={run(() => onClear(pad))}>
            Clear pad
          </MenuItem>
        </>
      )}
    </div>
  );
}
