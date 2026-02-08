import type { Marker } from '@/types/arrangement';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';

interface MarkerJumpDropdownProps {
  markers: Marker[];
  onSeekToMarker: (id: string) => void;
}

export function MarkerJumpDropdown({ markers, onSeekToMarker }: MarkerJumpDropdownProps) {
  if (markers.length === 0) return null;

  return (
    <Select onValueChange={onSeekToMarker}>
      <SelectTrigger
        className="h-6 w-24 border-0 bg-transparent text-[11px] gap-1"
        style={{ color: 'var(--muted-foreground)' }}
      >
        <SelectValue placeholder="Markers" />
      </SelectTrigger>
      <SelectContent>
        {markers.map((m) => (
          <SelectItem key={m.id} value={m.id}>
            <span className="flex items-center gap-1.5">
              <span
                className="inline-block h-2 w-2 rounded-full"
                style={{ backgroundColor: m.color }}
              />
              <span className="text-xs">{m.name}</span>
              <span className="text-[10px] text-muted-foreground">
                {Math.floor(m.beat / 4) + 1}:{Math.floor(m.beat % 4) + 1}
              </span>
            </span>
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
