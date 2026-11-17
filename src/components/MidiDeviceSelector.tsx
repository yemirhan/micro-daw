import { Cable } from 'lucide-react';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { cn } from '@/lib/utils';
import type { MidiDeviceInfo } from '@/types/midi';

interface MidiDeviceSelectorProps {
  devices: MidiDeviceInfo[];
  activeDeviceId: string | null;
  onSelect: (id: string) => void;
}

export function MidiDeviceSelector({ devices, activeDeviceId, onSelect }: MidiDeviceSelectorProps) {
  const connected = !!activeDeviceId;
  return (
    <Select value={activeDeviceId ?? ''} onValueChange={onSelect}>
      <SelectTrigger
        size="sm"
        className="w-[168px] gap-2 px-2 text-xs"
        aria-label="MIDI input device"
        title={connected ? 'MIDI input' : 'No MIDI input connected'}
      >
        <span className="flex min-w-0 items-center gap-2">
          <span className="relative flex size-4 shrink-0 items-center justify-center">
            <Cable className={cn('size-3.5', connected ? 'text-foreground' : 'text-muted-foreground')} />
            <span
              className={cn(
                'absolute -right-0.5 -bottom-0.5 size-1.5 rounded-full ring-2 ring-surface-2',
                connected ? 'bg-signal-play' : 'bg-surface-4',
              )}
            />
          </span>
          <SelectValue placeholder={<span className="text-muted-foreground">No MIDI input</span>} />
        </span>
      </SelectTrigger>
      <SelectContent position="popper" align="end" className="min-w-[220px]">
        {devices.length === 0 ? (
          <SelectItem value="none" disabled>
            No devices found
          </SelectItem>
        ) : (
          devices.map((device) => (
            <SelectItem key={device.id} value={device.id}>
              {device.name}
            </SelectItem>
          ))
        )}
      </SelectContent>
    </Select>
  );
}
