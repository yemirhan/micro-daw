import { useCallback, useSyncExternalStore } from 'react';
import { noteRepeat } from '@/services/NoteRepeat';
import type { NoteRepeatRate } from '@/types/sampler';

const subscribe = (cb: () => void) => noteRepeat.subscribe(cb);
const getEnabled = () => noteRepeat.isEnabled();
const getRate = () => noteRepeat.getRate();

/** Note-repeat settings (the service holds the state; this hook mirrors it). */
export function useNoteRepeat() {
  const enabled = useSyncExternalStore(subscribe, getEnabled);
  const rate = useSyncExternalStore(subscribe, getRate);
  const toggle = useCallback(() => noteRepeat.toggle(), []);
  const setRate = useCallback((r: NoteRepeatRate) => noteRepeat.setRate(r), []);
  return { enabled, rate, toggle, setRate };
}
