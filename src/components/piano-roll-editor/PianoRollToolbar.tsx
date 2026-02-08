import { ArrowLeft, MousePointer2, Pencil, Eraser, Grid3X3 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Label } from '@/components/ui/label';
import { cn } from '@/lib/utils';
import type { PianoRollTool } from '@/types/pianoRollEditor';
import { SNAP_VALUES } from '@/utils/constants';

interface PianoRollToolbarProps {
  tool: PianoRollTool;
  snapValue: number;
  regionName?: string;
  onToolChange: (tool: PianoRollTool) => void;
  onSnapChange: (value: number) => void;
  onBack: () => void;
  onToolPointerDown?: (tool: PianoRollTool) => void;
  onToolPointerUp?: () => void;
  onQuantize?: () => void;
}

const SNAP_LABELS: Record<number, string> = {
  0.25: '1/16',
  0.5: '1/8',
  1: '1/4',
  2: '1/2',
  4: 'Bar',
};

const TOOLS: { id: PianoRollTool; icon: typeof MousePointer2; label: string }[] = [
  { id: 'pointer', icon: MousePointer2, label: 'Pointer (V)' },
  { id: 'draw', icon: Pencil, label: 'Draw (D)' },
  { id: 'eraser', icon: Eraser, label: 'Eraser (E)' },
];

export function PianoRollToolbar({
  tool,
  snapValue,
  regionName,
  onToolChange,
  onSnapChange,
  onBack,
  onToolPointerDown,
  onToolPointerUp,
  onQuantize,
}: PianoRollToolbarProps) {
  return (
    <div className="flex items-center gap-3 border-b border-border bg-card/60 backdrop-blur-sm px-3 py-1.5">
      <Button
        variant="ghost"
        size="sm"
        className="h-6 gap-1 px-2 text-[11px]"
        onClick={onBack}
      >
        <ArrowLeft className="h-3 w-3" />
        Back
      </Button>

      {regionName && (
        <span className="text-xs font-medium text-muted-foreground truncate max-w-[120px]">
          {regionName}
        </span>
      )}

      <div className="h-4 w-px bg-border/60" />

      {/* Tool selector */}
      <div className="flex items-center gap-0.5">
        {TOOLS.map(({ id, icon: Icon, label }) => (
          <Button
            key={id}
            variant="ghost"
            size="sm"
            className={cn(
              "h-6 w-6 p-0",
              tool === id && "bg-primary/20 text-primary ring-1 ring-primary/40 hover:bg-primary/30"
            )}
            title={label}
            onClick={() => onToolChange(id)}
            onPointerDown={(e) => {
              if (e.button === 0 && onToolPointerDown) {
                onToolPointerDown(id);
              }
            }}
            onPointerUp={() => {
              if (onToolPointerUp) {
                onToolPointerUp();
              }
            }}
          >
            <Icon className="h-3.5 w-3.5" />
          </Button>
        ))}
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

      {onQuantize && (
        <>
          <div className="h-4 w-px bg-border/60" />
          <Button
            variant="ghost"
            size="sm"
            className="h-6 gap-1 px-2 text-[11px]"
            onClick={onQuantize}
            title="Quantize (Q)"
          >
            <Grid3X3 className="h-3 w-3" />
            Quantize
          </Button>
        </>
      )}
    </div>
  );
}
