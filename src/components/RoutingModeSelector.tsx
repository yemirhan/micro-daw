import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group';
import type { RoutingMode } from '@/utils/constants';

interface RoutingModeSelectorProps {
  mode: RoutingMode;
  onChange: (mode: RoutingMode) => void;
}

const ITEMS: { value: RoutingMode; label: string; title: string }[] = [
  {
    value: 'auto',
    label: 'Auto',
    title: 'Auto: notes mapped to pads in your controller profile trigger pads; other notes play the synth',
  },
  { value: 'keys', label: 'Keys', title: 'Keys: every key plays the synth' },
  { value: 'split', label: 'Split', title: 'Split: keys below the split point (C4 by default) trigger the 16 pads; the rest play the synth' },
];

/** Segmented control: how incoming MIDI notes are routed. */
export function RoutingModeSelector({ mode, onChange }: RoutingModeSelectorProps) {
  return (
    <ToggleGroup
      type="single"
      value={mode}
      onValueChange={(v) => {
        if (v) onChange(v as RoutingMode);
      }}
      size="sm"
      className="h-7 gap-0.5 rounded-md border border-border bg-surface-0 p-0.5"
      aria-label="MIDI note routing"
    >
      {ITEMS.map((item) => (
        <ToggleGroupItem
          key={item.value}
          value={item.value}
          title={item.title}
          className="h-[22px] min-w-0 rounded-[4px]! px-2 text-[11px] font-semibold data-[state=on]:bg-surface-3 data-[state=on]:text-foreground data-[state=on]:shadow-[inset_0_1px_0_oklch(1_0_0/0.06)]"
        >
          {item.label}
        </ToggleGroupItem>
      ))}
    </ToggleGroup>
  );
}
