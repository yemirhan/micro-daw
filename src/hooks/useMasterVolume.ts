import { useSyncExternalStore } from 'react';
import { getMasterVolumeDb, onMasterVolumeChange, setMasterVolumeDb } from '@/audio/masterBus';

/**
 * The app's master volume (dB, MIN_VOLUME..MAX_VOLUME): the master bus gain
 * every sound passes through. The TopBar slider, the master knob and the
 * legacy volume CC all drive it, so they always agree. Persisted.
 */
export function useMasterVolume(): [number, (db: number) => void] {
  const volume = useSyncExternalStore(onMasterVolumeChange, getMasterVolumeDb, getMasterVolumeDb);
  return [volume, setMasterVolumeDb];
}
