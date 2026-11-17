import type { ReactNode } from 'react';
import {
  ArrowLeft,
  CircleHelp,
  Eraser,
  FoldVertical,
  Grid3x3,
  LocateFixed,
  Maximize2,
  MousePointer2,
  Pencil,
  UnfoldVertical,
  ZoomIn,
  ZoomOut,
} from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Slider } from '@/components/ui/slider';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { cn } from '@/lib/utils';
import type { PianoRollTool } from '@/types/pianoRollEditor';
import {
  findSnapOption,
  formatBeats,
  NOTE_LENGTH_OPTIONS,
  SNAP_OPTIONS,
} from '@/utils/pianoRollHelpers';

const TOOLS: { id: PianoRollTool; icon: typeof MousePointer2; label: string; key: string }[] = [
  { id: 'pointer', icon: MousePointer2, label: 'Select', key: 'V' },
  { id: 'draw', icon: Pencil, label: 'Draw', key: 'D' },
  { id: 'eraser', icon: Eraser, label: 'Erase', key: 'E' },
];

interface PianoRollToolbarProps {
  regionName: string;
  trackName: string;
  trackColor: string;
  tool: PianoRollTool;
  snap: number;
  drawLength: number;
  isDrum: boolean;
  follow: boolean;
  /** 0..1 horizontal zoom slider position */
  zoom: number;
  noteCount: number;
  selectedCount: number;
  status: string;
  onBack: () => void;
  onToolChange: (tool: PianoRollTool) => void;
  onSnapChange: (beats: number) => void;
  onDrawLengthChange: (beats: number) => void;
  onQuantize: () => void;
  onFollowChange: (follow: boolean) => void;
  onZoomChange: (t: number) => void;
  onZoomStep: (dir: 1 | -1) => void;
  onRowZoomStep: (dir: 1 | -1) => void;
  onFit: () => void;
}

const Divider = () => <div className="mx-1 h-5 w-px shrink-0 bg-border" />;

function ToolButton({
  active,
  title,
  onClick,
  children,
}: {
  active?: boolean;
  title: string;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <Button
      variant="ghost"
      size="icon-xs"
      className={cn(
        'h-7 w-7 text-muted-foreground',
        active && 'bg-primary/15 text-primary ring-1 ring-primary/40 hover:bg-primary/20 hover:text-primary',
      )}
      title={title}
      aria-label={title}
      aria-pressed={active}
      onClick={onClick}
    >
      {children}
    </Button>
  );
}

export function PianoRollToolbar(props: PianoRollToolbarProps) {
  const {
    regionName,
    trackName,
    trackColor,
    tool,
    snap,
    drawLength,
    isDrum,
    follow,
    zoom,
    noteCount,
    selectedCount,
    status,
  } = props;
  const snapId = findSnapOption(snap)?.id ?? 'off';
  const lengthOpt = findSnapOption(drawLength, NOTE_LENGTH_OPTIONS);
  const lengthId = lengthOpt?.id ?? 'custom';

  return (
    <div className="flex h-11 shrink-0 items-center gap-1 border-b border-border bg-card px-2">
      <Button variant="ghost" size="xs" className="h-7 gap-1 px-2 text-xs" onClick={props.onBack} title="Close editor (Esc)">
        <ArrowLeft />
        Back
      </Button>
      <div className="flex min-w-0 max-w-[220px] items-center gap-2 px-1">
        <span className="h-2.5 w-2.5 shrink-0 rounded-full" style={{ backgroundColor: trackColor }} />
        <div className="flex min-w-0 flex-col leading-tight">
          <span className="truncate text-xs font-semibold text-foreground">{regionName}</span>
          <span className="truncate text-[10px] text-muted-foreground">{trackName}</span>
        </div>
      </div>

      <Divider />

      <div className="flex items-center gap-0.5" role="group" aria-label="Tools">
        {TOOLS.map(({ id, icon: Icon, label, key }) => (
          <ToolButton key={id} active={tool === id} title={`${label} (${key})`} onClick={() => props.onToolChange(id)}>
            <Icon className="size-3.5" />
          </ToolButton>
        ))}
      </div>

      <Divider />

      <label className="flex items-center gap-1.5 text-[10px] font-semibold tracking-wide text-muted-foreground uppercase">
        Grid
        <Select
          value={snapId}
          onValueChange={(id) => props.onSnapChange(SNAP_OPTIONS.find((o) => o.id === id)?.beats ?? 0)}
        >
          <SelectTrigger size="sm" className="h-7 w-[84px] px-2 font-mono text-xs normal-case" title="Snap / grid (hold Cmd/Ctrl or Alt while dragging to bypass)">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {SNAP_OPTIONS.map((o) => (
              <SelectItem key={o.id} value={o.id} className="font-mono text-xs">
                {o.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </label>

      {!isDrum && (
        <label className="ml-1 flex items-center gap-1.5 text-[10px] font-semibold tracking-wide text-muted-foreground uppercase">
          Length
          <Select
            value={lengthId}
            onValueChange={(id) => {
              const opt = NOTE_LENGTH_OPTIONS.find((o) => o.id === id);
              if (opt) props.onDrawLengthChange(opt.beats);
            }}
          >
            <SelectTrigger size="sm" className="h-7 w-[84px] px-2 font-mono text-xs normal-case" title="Length of new notes (follows the last drawn/resized note)">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {!lengthOpt && (
                <SelectItem value="custom" className="font-mono text-xs">
                  {formatBeats(drawLength)}
                </SelectItem>
              )}
              {NOTE_LENGTH_OPTIONS.map((o) => (
                <SelectItem key={o.id} value={o.id} className="font-mono text-xs">
                  {o.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </label>
      )}

      <Button
        variant="ghost"
        size="xs"
        className="ml-1 h-7 gap-1 px-2 text-xs text-muted-foreground hover:text-foreground"
        onClick={props.onQuantize}
        disabled={snap <= 0 || noteCount === 0}
        title="Quantize selected notes to the grid, or all notes if none are selected (Q)"
      >
        <Grid3x3 />
        Quantize
      </Button>

      <div className="min-w-2 flex-1" />

      <span className="hidden font-mono text-[11px] text-muted-foreground tabular-nums xl:inline" aria-live="polite">
        {status}
      </span>
      <span className="ml-2 font-mono text-[11px] text-muted-foreground tabular-nums">
        {selectedCount > 0 ? `${selectedCount}/${noteCount} sel` : `${noteCount} notes`}
      </span>

      <Divider />

      <ToolButton active={follow} title="Follow playhead" onClick={() => props.onFollowChange(!follow)}>
        <LocateFixed className="size-3.5" />
      </ToolButton>
      <ToolButton title="Zoom out (-)" onClick={() => props.onZoomStep(-1)}>
        <ZoomOut className="size-3.5" />
      </ToolButton>
      <Slider
        className="w-24"
        min={0}
        max={1000}
        step={1}
        value={[Math.round(zoom * 1000)]}
        onValueChange={([v]) => props.onZoomChange(v / 1000)}
        aria-label="Horizontal zoom"
      />
      <ToolButton title="Zoom in (+)" onClick={() => props.onZoomStep(1)}>
        <ZoomIn className="size-3.5" />
      </ToolButton>
      <ToolButton title="Shorter rows (Alt+wheel)" onClick={() => props.onRowZoomStep(-1)}>
        <FoldVertical className="size-3.5" />
      </ToolButton>
      <ToolButton title="Taller rows (Alt+wheel)" onClick={() => props.onRowZoomStep(1)}>
        <UnfoldVertical className="size-3.5" />
      </ToolButton>
      <ToolButton title="Fit notes to view (F)" onClick={props.onFit}>
        <Maximize2 className="size-3.5" />
      </ToolButton>

      <Divider />
      <ShortcutsPopover />
    </div>
  );
}

const SHORTCUTS: [string, string][] = [
  ['V / D / E', 'Select / Draw / Erase tool'],
  ['Click (Draw)', 'Add note · drag to set length'],
  ['Double-click', 'Add note (Select) · delete note'],
  ['Right-click / drag', 'Delete notes'],
  ['Drag note', 'Move (time + pitch)'],
  ['Alt + drag note', 'Duplicate'],
  ['Drag note edges', 'Resize start / end'],
  ['Cmd/Ctrl or Alt held', 'Bypass snap while dragging'],
  ['Shift + click / drag', 'Add to selection'],
  ['Cmd/Ctrl + A', 'Select all'],
  ['Cmd/Ctrl + C / X / V', 'Copy / cut / paste at playhead'],
  ['Cmd/Ctrl + D', 'Duplicate selection'],
  ['Delete / Backspace', 'Delete selection'],
  ['↑ / ↓', 'Transpose a semitone'],
  ['Shift + ↑ / ↓', 'Transpose an octave'],
  ['← / →', 'Nudge by grid step'],
  ['Alt + ← / →', 'Shorten / lengthen'],
  ['Q', 'Quantize'],
  ['Space', 'Play / stop from the cue'],
  ['Cmd/Ctrl + wheel', 'Zoom horizontally'],
  ['Alt + wheel', 'Zoom vertically'],
  ['+ / −  ·  F', 'Zoom · fit'],
  ['Esc', 'Cancel drag · close editor'],
];

function ShortcutsPopover() {
  return (
    <Popover>
      <PopoverTrigger asChild>
        <Button variant="ghost" size="icon-xs" className="h-7 w-7 text-muted-foreground" title="Keyboard shortcuts" aria-label="Keyboard shortcuts">
          <CircleHelp className="size-3.5" />
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-80 p-3">
        <div className="mb-2 text-xs font-semibold text-foreground">Piano roll shortcuts</div>
        <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-[11px]">
          {SHORTCUTS.map(([keys, desc]) => (
            <div key={keys} className="contents">
              <dt className="font-mono whitespace-nowrap text-muted-foreground">{keys}</dt>
              <dd className="text-foreground">{desc}</dd>
            </div>
          ))}
        </dl>
      </PopoverContent>
    </Popover>
  );
}
