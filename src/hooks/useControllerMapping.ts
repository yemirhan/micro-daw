import { useCallback, useSyncExternalStore } from 'react';
import {
  controllerMapping,
  type LearnState,
  type MidiActivity,
} from '@/services/ControllerMapping';
import type { ControllerProfile } from '@/types/sampler';

const subscribe = (cb: () => void) => controllerMapping.subscribe(cb);
const getProfile = () => controllerMapping.getProfile();
const getLearnState = () => controllerMapping.getLearnState();

/** Controller profile + learn state, re-rendering on every change. */
export function useControllerMapping(): {
  profile: ControllerProfile;
  learn: LearnState;
} {
  const profile = useSyncExternalStore(subscribe, getProfile);
  const learn = useSyncExternalStore(subscribe, getLearnState);
  return { profile, learn };
}

/**
 * Last MIDI note-on / CC received (for live activity indicators). Re-renders on
 * every incoming message, so only use it in UI that is actually on screen.
 */
export function useMidiActivity(enabled = true): MidiActivity | null {
  const subscribeActivity = useCallback(
    (cb: () => void) => (enabled ? controllerMapping.onActivity(cb) : () => undefined),
    [enabled],
  );
  return useSyncExternalStore(subscribeActivity, () => controllerMapping.getLastActivity());
}
