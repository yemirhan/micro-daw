import { useState, useEffect, useCallback } from 'react';
import { midiService } from '@/services/MidiService';
import type { MidiDeviceInfo, MidiCallbacks } from '@/types/midi';

export function useMidi(callbacks: MidiCallbacks) {
  const [devices, setDevices] = useState<MidiDeviceInfo[]>([]);
  const [activeDeviceId, setActiveDeviceId] = useState<string | null>(null);
  const [enabled, setEnabled] = useState(false);

  const enable = useCallback(async () => {
    await midiService.enable();
    setEnabled(true);
    setDevices(midiService.getDevices());
    setActiveDeviceId(midiService.getActiveDeviceId());
  }, []);

  useEffect(() => {
    midiService.setCallbacks(callbacks);
  }, [callbacks]);

  useEffect(() => {
    if (!enabled) return;
    const unsub = midiService.onDeviceChange((newDevices) => {
      setDevices(newDevices);
      setActiveDeviceId(midiService.getActiveDeviceId());
    });
    return unsub;
  }, [enabled]);

  const selectDevice = useCallback((id: string) => {
    midiService.selectDevice(id);
    setActiveDeviceId(id);
  }, []);

  return { devices, activeDeviceId, enabled, enable, selectDevice };
}
