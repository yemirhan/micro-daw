import { Usb } from 'lucide-react';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import type { MidiDeviceInfo } from '@/types/midi';

interface MidiDeviceSelectorProps {
  devices: MidiDeviceInfo[];
  activeDeviceId: string | null;
  onSelect: (id: string) => void;
}

export function MidiDeviceSelector({ devices, activeDeviceId, onSelect }: MidiDeviceSelectorProps) {
  return (
    <div className="flex items-center gap-2">
      <Usb className="size-4 text-muted-foreground" />
      <Select value={activeDeviceId ?? ''} onValueChange={onSelect}>
        <SelectTrigger size="sm" className="w-[180px]">
          <SelectValue placeholder="No MIDI device" />
        </SelectTrigger>
        <SelectContent>
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
      <div
        className="size-2 rounded-full transition-colors"
        style={{
          backgroundColor: activeDeviceId ? 'oklch(0.65 0.20 155)' : 'var(--muted-foreground)',
          opacity: activeDeviceId ? 1 : 0.3,
          boxShadow: activeDeviceId ? '0 0 6px 1px oklch(0.65 0.20 155 / 0.5)' : 'none',
        }}
      />
    </div>
  );
}
