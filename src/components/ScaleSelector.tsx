import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { getRootNoteOptions, getScaleOptions } from '@/utils/scales';
import type { ScaleName } from '@/types/music';

interface ScaleSelectorProps {
  selectedRoot: number;
  selectedScale: ScaleName | null;
  onRootChange: (root: number) => void;
  onScaleChange: (scale: ScaleName | null) => void;
}

const rootOptions = getRootNoteOptions();
const scaleOptions = getScaleOptions();

export function ScaleSelector({
  selectedRoot,
  selectedScale,
  onRootChange,
  onScaleChange,
}: ScaleSelectorProps) {
  return (
    <div className="flex items-center gap-2">
      <Select
        value={String(selectedRoot)}
        onValueChange={(v) => onRootChange(Number(v))}
      >
        <SelectTrigger className="h-7 w-16 text-xs">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {rootOptions.map((opt) => (
            <SelectItem key={opt.value} value={String(opt.value)} className="text-xs">
              {opt.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      <Select
        value={selectedScale ?? 'none'}
        onValueChange={(v) => onScaleChange(v === 'none' ? null : (v as ScaleName))}
      >
        <SelectTrigger className="h-7 w-36 text-xs">
          <SelectValue placeholder="Scale" />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="none" className="text-xs">
            Off
          </SelectItem>
          {scaleOptions.map((opt) => (
            <SelectItem key={opt.value} value={opt.value} className="text-xs">
              {opt.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  );
}
