import { memo, useCallback, useEffect, useRef, useState } from 'react';
import { cn } from '@/lib/utils';
import { noteEventBus } from '@/services/NoteEventBus';
import type { PadIndex, PadSettings } from '@/types/sampler';
import {
  PAD_KEY_HINTS,
  dragHasSample,
  hardwareNoteLabel,
  padColor,
  padLabel,
  padName,
  padRows,
  readDroppedSample,
  velocityFromPointer,
  type DroppedSample,
  type PadLayout,
} from './beatHelpers';
import { PadContextMenu, type PadMenuActions } from './PadContextMenu';
import { PadThumb } from './PadThumb';

export interface PadGridProps extends PadMenuActions {
  pads: PadSettings[];
  selectedPad: PadIndex;
  layout?: PadLayout;
  /** Incoming hardware note per pad (controller profile), shown in the corner. */
  hardwareNotes?: (number | null)[];
  /** Show the computer-key hint on each pad. */
  showKeyHints?: boolean;
  canPaste?: boolean;
  /** Show one bank only (2×4, like the MPK Mini's 8 pads): 'A' = pads 1–8, 'B' = 9–16. */
  bank?: 'A' | 'B';
  /** Smaller pads without the waveform thumbnail (instrument dock). */
  compact?: boolean;
  onHit: (pad: PadIndex, velocity: number) => void;
  onRelease?: (pad: PadIndex) => void;
  onSelect: (pad: PadIndex) => void;
  onDropSample?: (pad: PadIndex, sample: DroppedSample) => void;
  /** Resolves a Finder file to its path (defaults to `window.electronAPI.getPathForFile`). */
  getPathForFile?: (file: File) => string;
  className?: string;
}

/** Flash a pad's light without re-rendering React (Web Animations; no-op where unsupported). */
function flash(el: HTMLElement | null | undefined, velocity: number) {
  if (!el) return;
  const light = el.querySelector<HTMLElement>('[data-pad-light]');
  const v = Math.min(1, Math.max(0.25, velocity > 1 ? velocity / 127 : velocity));
  light?.animate?.([{ opacity: 0.35 + 0.6 * v }, { opacity: 0 }], { duration: 260, easing: 'cubic-bezier(0.2, 0.7, 0.3, 1)' });
  el.animate?.([{ transform: 'scale(0.965)' }, { transform: 'scale(1)' }], { duration: 140, easing: 'cubic-bezier(0.22, 1, 0.36, 1)' });
}

/**
 * The 16 pads. Pads flash on every hit published on the NoteEventBus (MIDI,
 * mouse or keyboard alike); clicking hits + selects, with velocity from the
 * vertical click position (higher = harder).
 */
export function PadGrid({
  pads,
  selectedPad,
  layout = '4x4',
  hardwareNotes,
  showKeyHints = true,
  canPaste = false,
  bank,
  compact = false,
  onHit,
  onRelease,
  onSelect,
  onDropSample,
  getPathForFile,
  className,
  ...menuActions
}: PadGridProps) {
  const padEls = useRef(new Map<PadIndex, HTMLElement>());
  const [dragOver, setDragOver] = useState<PadIndex | null>(null);
  const [menu, setMenu] = useState<{ pad: PadIndex; x: number; y: number } | null>(null);

  useEffect(
    () =>
      noteEventBus.subscribe((e) => {
        if (e.type === 'drum') flash(padEls.current.get(e.padId), e.velocity);
      }),
    [],
  );

  const registerEl = useCallback((pad: PadIndex, el: HTMLElement | null) => {
    if (el) padEls.current.set(pad, el);
    else padEls.current.delete(pad);
  }, []);

  const handleDrop = useCallback(
    (pad: PadIndex, dt: DataTransfer) => {
      setDragOver(null);
      const resolve = getPathForFile ?? (typeof window !== 'undefined' ? window.electronAPI?.getPathForFile : undefined);
      const sample = readDroppedSample(dt, resolve);
      if (!sample) return;
      onDropSample?.(pad, sample);
      onSelect(pad);
    },
    [getPathForFile, onDropSample, onSelect],
  );

  const openMenu = useCallback((pad: PadIndex, x: number, y: number) => {
    onSelect(pad);
    setMenu({ pad, x, y });
  }, [onSelect]);
  const closeMenu = useCallback(() => setMenu(null), []);

  const rows = bank ? bankRows(bank) : padRows(layout);
  const cols = rows[0].length;
  const hasMenu = Object.values(menuActions).some(Boolean);

  return (
    <div
      role="grid"
      aria-label="Pads"
      className={cn('grid', compact ? 'gap-1' : 'gap-1.5', className)}
      style={{ gridTemplateColumns: `repeat(${cols}, minmax(0, 1fr))`, gridTemplateRows: `repeat(${rows.length}, minmax(0, 1fr))` }}
    >
      {rows.flatMap((row) =>
        row.map((pad, c) => (
          <Pad
            key={pad}
            pad={pad}
            settings={pads[pad]}
            selected={pad === selectedPad}
            dragOver={pad === dragOver}
            hardwareNote={hardwareNotes?.[pad] ?? null}
            keyHint={showKeyHints && !compact ? PAD_KEY_HINTS[pad] : undefined}
            compact={compact}
            // Gap between the two banks in the 2x8 layout, like the hardware's bank switch.
            bankGap={!bank && layout === '2x8' && c === 4}
            registerEl={registerEl}
            onHit={onHit}
            onRelease={onRelease}
            onSelect={onSelect}
            onDragOverPad={setDragOver}
            onDropPad={onDropSample ? handleDrop : undefined}
            onMenu={hasMenu ? openMenu : undefined}
          />
        )),
      )}
      {menu && pads[menu.pad] && (
        <PadContextMenu
          pad={menu.pad}
          settings={pads[menu.pad]}
          x={menu.x}
          y={menu.y}
          canPaste={canPaste}
          onClose={closeMenu}
          {...menuActions}
        />
      )}
    </div>
  );
}

/** One bank as two rows of four, pad 1 bottom-left (MPK Mini layout). */
function bankRows(bank: 'A' | 'B'): PadIndex[][] {
  const o = bank === 'A' ? 0 : 8;
  return [
    [o + 4, o + 5, o + 6, o + 7],
    [o, o + 1, o + 2, o + 3],
  ];
}

interface PadProps {
  pad: PadIndex;
  compact: boolean;
  settings: PadSettings | undefined;
  selected: boolean;
  dragOver: boolean;
  hardwareNote: number | null;
  keyHint?: string;
  bankGap: boolean;
  registerEl: (pad: PadIndex, el: HTMLElement | null) => void;
  onHit: (pad: PadIndex, velocity: number) => void;
  onRelease?: (pad: PadIndex) => void;
  onSelect: (pad: PadIndex) => void;
  onDragOverPad: (pad: PadIndex | null) => void;
  onDropPad?: (pad: PadIndex, dt: DataTransfer) => void;
  onMenu?: (pad: PadIndex, x: number, y: number) => void;
}

const Pad = memo(function Pad({
  pad,
  compact,
  settings,
  selected,
  dragOver,
  hardwareNote,
  keyHint,
  bankGap,
  registerEl,
  onHit,
  onRelease,
  onSelect,
  onDragOverPad,
  onDropPad,
  onMenu,
}: PadProps) {
  /** Pointer or Enter/Space hold in progress (one press at a time; released exactly once). */
  const held = useRef<'pointer' | 'key' | null>(null);
  const color = padColor(settings, pad);
  const name = padName(settings);
  const empty = !settings?.sample;

  const releaseRef = useRef(() => {});
  const release = () => {
    if (!held.current) return;
    held.current = null;
    onRelease?.(pad);
  };
  useEffect(() => {
    releaseRef.current = release;
  });
  // Unmount (view switch, layout change) mid-press: never leave a pad held.
  useEffect(() => () => releaseRef.current(), []);
  const isHoldKey = (key: string) => key === 'Enter' || key === ' ';

  return (
    <button
      ref={(el) => registerEl(pad, el)}
      type="button"
      role="gridcell"
      data-pad={pad}
      data-selected={selected || undefined}
      aria-selected={selected}
      aria-label={`Pad ${padLabel(pad)}${name ? `: ${name}` : ', empty'}`}
      className={cn(
        'group/pad relative isolate flex min-h-0 min-w-0 flex-col overflow-hidden rounded-[7px] text-left outline-none select-none',
        'bg-surface-2 transition-[box-shadow,background-color] duration-100',
        'shadow-[inset_0_1px_0_oklch(1_0_0/0.06),inset_0_-2px_0_oklch(0_0_0/0.35),0_1px_2px_oklch(0_0_0/0.4)]',
        'hover:bg-surface-3 focus-visible:ring-2 focus-visible:ring-ring/70',
        bankGap && 'ml-2',
        selected && 'bg-surface-3',
        dragOver && 'bg-surface-4',
      )}
      style={{
        // Selected: a lit outline in the pad's own colour (hardware LED feel).
        boxShadow: dragOver
          ? `inset 0 0 0 2px var(--primary)`
          : selected
            ? `inset 0 0 0 2px ${color}, 0 0 0 1px oklch(0 0 0 / 0.5)`
            : undefined,
      }}
      onPointerDown={(e) => {
        if (e.button !== 0) return;
        e.preventDefault();
        const rect = e.currentTarget.getBoundingClientRect();
        release();
        held.current = 'pointer';
        e.currentTarget.setPointerCapture?.(e.pointerId);
        onSelect(pad);
        onHit(pad, velocityFromPointer(e.clientY, rect.top, rect.height));
      }}
      onPointerUp={() => held.current === 'pointer' && release()}
      onPointerCancel={() => held.current === 'pointer' && release()}
      onLostPointerCapture={() => held.current === 'pointer' && release()}
      onKeyDown={(e) => {
        if (!isHoldKey(e.key) || e.metaKey || e.ctrlKey || e.altKey) return;
        // Always swallow: Space would reach the app's play/stop, Enter would "click".
        e.preventDefault();
        e.stopPropagation();
        // Auto-repeat keydowns (and a second key while one is down) don't re-trigger.
        if (e.repeat || held.current) return;
        held.current = 'key';
        onSelect(pad);
        onHit(pad, 0.8);
      }}
      onKeyUp={(e) => {
        if (!isHoldKey(e.key)) return;
        e.preventDefault();
        e.stopPropagation();
        if (held.current === 'key') release();
      }}
      onBlur={() => held.current === 'key' && release()}
      onContextMenu={(e) => {
        e.preventDefault();
        onMenu?.(pad, e.clientX, e.clientY);
      }}
      onDragEnter={(e) => {
        if (!onDropPad || !dragHasSample(e.dataTransfer)) return;
        e.preventDefault();
        onDragOverPad(pad);
      }}
      onDragOver={(e) => {
        if (!onDropPad || !dragHasSample(e.dataTransfer)) return;
        e.preventDefault();
        e.dataTransfer.dropEffect = 'copy';
      }}
      onDragLeave={(e) => {
        if (e.currentTarget.contains(e.relatedTarget as Node | null)) return;
        onDragOverPad(null);
      }}
      onDrop={(e) => {
        if (!onDropPad) return;
        e.preventDefault();
        onDropPad(pad, e.dataTransfer);
      }}
    >
      {/* Colour strip: the pad's identity, always visible. */}
      <span aria-hidden className="absolute inset-x-0 top-0 h-[3px]" style={{ background: color, opacity: empty ? 0.35 : 0.95 }} />

      {/* Hit light — animated imperatively on every hit. */}
      <span
        aria-hidden
        data-pad-light
        className="pointer-events-none absolute inset-0 -z-10 opacity-0"
        style={{ background: `radial-gradient(120% 90% at 50% 100%, ${color}, color-mix(in oklab, ${color} 55%, transparent))` }}
      />

      <span className={cn('flex items-start justify-between gap-1 px-2 font-mono text-[11px] leading-none', compact ? 'pt-1.5' : 'pt-2')}>
        <span className={cn('font-semibold', selected ? 'text-foreground' : 'text-muted-foreground')}>{padLabel(pad)}</span>
        {!compact && (
          <span className="text-muted-foreground tabular-nums" title={hardwareNote != null ? `MIDI note ${hardwareNote}` : 'No MIDI note mapped'}>
            {hardwareNoteLabel(hardwareNote)}
          </span>
        )}
      </span>

      <span
        className={cn(
          'mt-auto px-2 text-[12px] leading-[1.15] font-semibold break-words',
          compact ? 'mb-1.5 line-clamp-1' : 'line-clamp-2',
          empty ? 'font-medium text-muted-foreground italic' : 'text-foreground',
        )}
      >
        {dragOver ? 'Drop to load' : name || 'Empty'}
      </span>

      {!compact && (
        <span className="relative mx-2 mt-1 mb-1.5 flex h-[18px] items-end">
          {settings?.sample ? (
            <PadThumb settings={settings} color={color} className="h-full w-full" />
          ) : (
            <span className="h-px w-full bg-hairline-strong" />
          )}
          {keyHint && (
            <span className="absolute right-0 bottom-0 rounded-[3px] bg-surface-0/90 px-1 font-mono text-[11px] leading-[14px] text-muted-foreground opacity-0 transition-opacity group-hover/pad:opacity-100 group-focus-visible/pad:opacity-100 [[data-show-keys]_&]:opacity-100">
              {keyHint}
            </span>
          )}
        </span>
      )}
    </button>
  );
});
