import { Flag } from 'lucide-react';
import type { Marker } from '@/types/arrangement';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
} from '@/components/ui/select';

interface MarkerJumpDropdownProps {
  markers: Marker[];
  onSeekToMarker: (id: string) => void;
}

export function MarkerJumpDropdown({ markers, onSeekToMarker }: MarkerJumpDropdownProps) {
  if (markers.length === 0) return null;

  return (
    // Controlled with an empty value so picking the same marker twice still jumps.
    <Select value="" onValueChange={onSeekToMarker}>
      <SelectTrigger
        size="sm"
        className="w-auto gap-1.5 border-transparent bg-transparent px-2 text-[11px] font-semibold text-muted-foreground hover:bg-surface-3 hover:text-foreground"
        aria-label="Jump to marker"
        title="Jump to marker ( [ / ] )"
      >
        <Flag className="size-3.5" />
        <span className="tabular-nums">{markers.length}</span>
      </SelectTrigger>
      <SelectContent position="popper" align="center" className="min-w-[200px]">
        {markers.map((m) => (
          <SelectItem key={m.id} value={m.id}>
            <span className="flex w-full items-center gap-2">
              <span className="inline-block h-3 w-1 rounded-full" style={{ backgroundColor: m.color }} />
              <span className="text-[13px]">{m.name}</span>
              <span className="ml-auto pl-3 font-mono text-[10.5px] text-muted-foreground">
                {Math.floor(m.beat / 4) + 1}.{Math.floor(m.beat % 4) + 1}
              </span>
            </span>
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
