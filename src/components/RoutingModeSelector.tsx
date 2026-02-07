import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group';
import type { RoutingMode } from '@/utils/constants';

interface RoutingModeSelectorProps {
  mode: RoutingMode;
  onChange: (mode: RoutingMode) => void;
}

export function RoutingModeSelector({ mode, onChange }: RoutingModeSelectorProps) {
  return (
    <ToggleGroup
      type="single"
      value={mode}
      onValueChange={(v) => {
        if (v) onChange(v as RoutingMode);
      }}
      size="sm"
      className="gap-0"
    >
      <ToggleGroupItem value="auto" className="rounded-r-none text-[11px] font-semibold px-2 h-7">
        Auto
      </ToggleGroupItem>
      <ToggleGroupItem value="keys" className="rounded-none text-[11px] font-semibold px-2 h-7">
        Keys
      </ToggleGroupItem>
      <ToggleGroupItem value="split" className="rounded-l-none text-[11px] font-semibold px-2 h-7">
        Split
      </ToggleGroupItem>
    </ToggleGroup>
  );
}
