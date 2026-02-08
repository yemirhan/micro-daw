import { Plus, ZoomIn, ZoomOut, Undo2, Redo2, Download, MousePointer2, Scissors, FileAudio } from 'lucide-react';
import { Button } from '@/components/ui/button';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Label } from '@/components/ui/label';
import { SNAP_VALUES, MIN_PX_PER_BEAT, MAX_PX_PER_BEAT } from '@/utils/constants';

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
}

const SNAP_LABELS: Record<number, string> = {
  0.25: '1/16',
  0.5: '1/8',
  1: '1/4',
  2: '1/2',
  4: 'Bar',
};

export function ArrangementToolbar({
  snapValue,
  pxPerBeat,
  canUndo,
  canRedo,
  onSnapChange,
  onZoomChange,
  onAddSynthTrack,
  onAddDrumTrack,
  onImportAudio,
  onUndo,
  onRedo,
  onExport,
  tool,
  onToolChange,
}: ArrangementToolbarProps) {
  return (
    <div className="flex items-center gap-3 border-b border-border bg-card/60 backdrop-blur-sm px-3 py-1.5">
      {/* Add tracks */}
      <div className="flex items-center gap-1">
        <Button
          variant="outline"
          size="sm"
          className="h-6 gap-1 px-2 text-[11px] font-semibold"
          style={{ borderColor: 'var(--primary)', color: 'var(--primary)' }}
          onClick={onAddSynthTrack}
        >
          <Plus className="h-3 w-3" /> Synth
        </Button>
        <Button variant="outline" size="sm" className="h-6 gap-1 px-2 text-[11px] font-semibold" onClick={onAddDrumTrack}>
          <Plus className="h-3 w-3" /> Drums
        </Button>
        {onImportAudio && (
          <Button variant="outline" size="sm" className="h-6 gap-1 px-2 text-[11px] font-semibold" onClick={onImportAudio}>
            <FileAudio className="h-3 w-3" /> Audio
          </Button>
        )}
      </div>

      <div className="h-4 w-px bg-border/60" />

      {/* Undo / Redo */}
      <div className="flex items-center gap-0.5">
        <Button
          variant="ghost"
          size="sm"
          className="h-6 w-6 p-0"
          disabled={!canUndo}
          onClick={onUndo}
          title="Undo (Ctrl+Z)"
        >
          <Undo2 className="h-3.5 w-3.5" />
        </Button>
        <Button
          variant="ghost"
          size="sm"
          className="h-6 w-6 p-0"
          disabled={!canRedo}
          onClick={onRedo}
          title="Redo (Ctrl+Shift+Z)"
        >
          <Redo2 className="h-3.5 w-3.5" />
        </Button>
      </div>

      <div className="h-4 w-px bg-border/60" />

      {/* Tool selector */}
      <div className="flex items-center gap-0.5">
        <Button
          variant={tool === 'pointer' ? 'default' : 'ghost'}
          size="sm"
          className={`h-6 w-6 p-0 ${tool === 'pointer' ? 'ring-1 ring-primary/50 shadow-sm shadow-primary/30' : ''}`}
          onClick={() => onToolChange('pointer')}
          title="Pointer (V)"
        >
          <MousePointer2 className="h-3.5 w-3.5" />
        </Button>
        <Button
          variant={tool === 'cut' ? 'default' : 'ghost'}
          size="sm"
          className={`h-6 w-6 p-0 ${tool === 'cut' ? 'ring-1 ring-primary/50 shadow-sm shadow-primary/30' : ''}`}
          onClick={() => onToolChange('cut')}
          title="Cut (C)"
        >
          <Scissors className="h-3.5 w-3.5" />
        </Button>
      </div>

      <div className="h-4 w-px bg-border/60" />

      {/* Snap */}
      <div className="flex items-center gap-1.5">
        <Label className="text-[11px] font-semibold text-muted-foreground">Snap</Label>
        <Select
          value={String(snapValue)}
          onValueChange={(v) => onSnapChange(Number(v))}
        >
          <SelectTrigger className="h-6 w-16 border-0 bg-transparent text-[11px]">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {SNAP_VALUES.map((v) => (
              <SelectItem key={v} value={String(v)}>{SNAP_LABELS[v]}</SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <div className="h-4 w-px bg-border/60" />

      {/* Zoom */}
      <div className="flex items-center gap-1">
        <Button
          variant="ghost"
          size="sm"
          className="h-6 w-6 p-0"
          onClick={() => onZoomChange(Math.max(MIN_PX_PER_BEAT, pxPerBeat - 10))}
        >
          <ZoomOut className="h-3 w-3" />
        </Button>
        <span className="w-6 text-center font-mono text-[10px] text-muted-foreground">
          {pxPerBeat}
        </span>
        <Button
          variant="ghost"
          size="sm"
          className="h-6 w-6 p-0"
          onClick={() => onZoomChange(Math.min(MAX_PX_PER_BEAT, pxPerBeat + 10))}
        >
          <ZoomIn className="h-3 w-3" />
        </Button>
      </div>

      <div className="flex-1" />

      {/* Export */}
      <Button
        variant="outline"
        size="sm"
        className="h-6 gap-1 px-2 text-[11px] font-semibold"
        onClick={onExport}
      >
        <Download className="h-3 w-3" /> Export
      </Button>
    </div>
  );
}
