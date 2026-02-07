import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { SYNTH_PRESETS } from '@/utils/constants';

interface InstrumentSelectorProps {
  presetIndex: number;
  onChange: (index: number) => void;
}

export function InstrumentSelector({ presetIndex, onChange }: InstrumentSelectorProps) {
  return (
    <div className="flex items-center gap-2">
      <span className="text-muted-foreground text-[11px] font-semibold uppercase tracking-wider">
        Synth
      </span>
      <Select
        value={String(presetIndex)}
        onValueChange={(val) => onChange(Number(val))}
      >
        <SelectTrigger size="sm" className="w-[130px]">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {SYNTH_PRESETS.map((preset, i) => (
            <SelectItem key={i} value={String(i)}>
              {preset.name}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  );
}
