import { Plus, ZoomIn, ZoomOut, Download, MousePointer2, Scissors, FileAudio, Flag, FolderPlus, Magnet } from 'lucide-react';
import { Button } from '@/components/ui/button';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Tip } from '@/components/ui/tip';
import { cn } from '@/lib/utils';
import { SNAP_VALUES, MIN_PX_PER_BEAT, MAX_PX_PER_BEAT, DEFAULT_PX_PER_BEAT } from '@/utils/constants';

export type ArrangementTool = 'pointer' | 'cut';

interface ArrangementToolbarProps {
  snapValue: number;
  pxPerBeat: number;
  canUndo: boolean;
  canRedo: boolean;
  tool: ArrangementTool;
  onSnapChange: (value: number) => void;
  onZoomChange: (pxPerBeat: number) => void;
  onAddSynthTrack: () => void;
  onAddDrumTrack: () => void;
  onImportAudio?: () => void;
  onUndo: () => void;
  onRedo: () => void;
  onExport: () => void;
  onToolChange: (tool: ArrangementTool) => void;
  onAddMarker?: () => void;
  onCreateGroup?: () => void;
}

const SNAP_LABELS: Record<number, string> = {
  0.25: '1/16',
  0.5: '1/8',
  1: '1/4',
  2: '1/2',
  4: 'Bar',
};

/** Arrangement toolbar. Undo/redo live in the top bar (props kept for compatibility). */
export function ArrangementToolbar({
  snapValue,
  pxPerBeat,
  onSnapChange,
  onZoomChange,
  onAddSynthTrack,
  onAddDrumTrack,
  onImportAudio,
  onExport,
  tool,
  onToolChange,
  onAddMarker,
  onCreateGroup,
}: ArrangementToolbarProps) {
  const zoomPct = Math.round((pxPerBeat / DEFAULT_PX_PER_BEAT) * 100);
  return (
    <div className="flex h-9 shrink-0 items-center gap-2 border-b border-border bg-surface-1 px-2">
      {/* Add tracks */}
      <div className="flex items-center gap-1">
        <Tip label="Add a synth track">
          <Button variant="outline" size="sm" className="h-7 gap-1 px-2 text-xs font-semibold" onClick={onAddSynthTrack}>
            <Plus className="size-3.5 text-primary" /> Synth
          </Button>
        </Tip>
        <Tip label="Add a drum track">
          <Button variant="outline" size="sm" className="h-7 gap-1 px-2 text-xs font-semibold" onClick={onAddDrumTrack}>
            <Plus className="size-3.5 text-primary" /> Drums
          </Button>
        </Tip>
        {onImportAudio && (
          <Tip label="Import an audio file" shortcut="⌘I">
            <Button variant="ghost" size="sm" className="h-7 gap-1.5 px-2 text-xs" onClick={onImportAudio}>
              <FileAudio className="size-3.5" /> Audio
            </Button>
          </Tip>
        )}
      </div>

      <div className="h-4 w-px bg-border" />

      <div className="flex items-center gap-0.5">
        {onAddMarker && (
          <Tip label="Add marker at playhead" shortcut="⇧M">
            <Button variant="ghost" size="sm" className="h-7 gap-1.5 px-2 text-xs" onClick={onAddMarker}>
              <Flag className="size-3.5" /> Marker
            </Button>
          </Tip>
        )}
        {onCreateGroup && (
          <Tip label="Group tracks (selected regions' tracks, or all)" shortcut="⌘G">
            <Button variant="ghost" size="sm" className="h-7 gap-1.5 px-2 text-xs" onClick={onCreateGroup}>
              <FolderPlus className="size-3.5" /> Group
            </Button>
          </Tip>
        )}
      </div>

      <div className="h-4 w-px bg-border" />

      {/* Tool selector */}
      <div className="flex h-7 items-center gap-0.5 rounded-md border border-border bg-surface-0 p-0.5" role="radiogroup" aria-label="Edit tool">
        {([
          { id: 'pointer', icon: MousePointer2, label: 'Pointer: select, move, resize', key: 'V' },
          { id: 'cut', icon: Scissors, label: 'Cut: click a region to split it', key: 'C' },
        ] as const).map(({ id, icon: Icon, label, key }) => (
          <Tip key={id} label={label} shortcut={key}>
            <button
              type="button"
              role="radio"
              aria-checked={tool === id}
              aria-label={label}
              onClick={() => onToolChange(id)}
              className={cn(
                'flex h-[22px] w-7 items-center justify-center rounded-[4px] transition-colors duration-150 focus-visible:outline-2 focus-visible:outline-ring',
                tool === id ? 'bg-primary text-primary-foreground' : 'text-muted-foreground hover:bg-surface-3 hover:text-foreground',
              )}
            >
              <Icon className="size-3.5" />
            </button>
          </Tip>
        ))}
      </div>

      {/* Snap */}
      <Select value={String(snapValue)} onValueChange={(v) => onSnapChange(Number(v))}>
        <SelectTrigger size="sm" className="h-7 w-[86px] gap-1.5 px-2 text-xs" aria-label="Snap to grid" title="Snap to grid">
          <Magnet className="size-3.5" />
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {SNAP_VALUES.map((v) => (
            <SelectItem key={v} value={String(v)}>{SNAP_LABELS[v] ?? String(v)}</SelectItem>
          ))}
        </SelectContent>
      </Select>

      {/* Zoom */}
      <div className="flex items-center">
        <Tip label="Zoom out" shortcut="⌘ scroll">
          <Button variant="ghost" size="icon-sm" aria-label="Zoom out" onClick={() => onZoomChange(Math.max(MIN_PX_PER_BEAT, pxPerBeat - 10))} disabled={pxPerBeat <= MIN_PX_PER_BEAT}>
            <ZoomOut />
          </Button>
        </Tip>
        <button
          type="button"
          className="h-7 w-11 rounded-md text-center font-mono text-[11px] text-muted-foreground tabular-nums transition-colors hover:bg-surface-3 hover:text-foreground"
          onClick={() => onZoomChange(DEFAULT_PX_PER_BEAT)}
          title="Reset zoom"
        >
          {zoomPct}%
        </button>
        <Tip label="Zoom in" shortcut="⌘ scroll">
          <Button variant="ghost" size="icon-sm" aria-label="Zoom in" onClick={() => onZoomChange(Math.min(MAX_PX_PER_BEAT, pxPerBeat + 10))} disabled={pxPerBeat >= MAX_PX_PER_BEAT}>
            <ZoomIn />
          </Button>
        </Tip>
      </div>

      <div className="flex-1" />

      {/* Export */}
      <Tip label="Export WAV or MIDI" shortcut="⌘E">
        <Button variant="outline" size="sm" className="h-7 gap-1.5 px-2.5 text-xs font-semibold" onClick={onExport}>
          <Download className="size-3.5" /> Export
        </Button>
      </Tip>
    </div>
  );
}
