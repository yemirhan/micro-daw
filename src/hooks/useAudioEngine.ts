import { useState, useCallback } from 'react';
import { audioEngine } from '@/services/AudioEngine';
import { DEFAULT_VOLUME } from '@/utils/constants';

export function useAudioEngine() {
  const [started, setStarted] = useState(false);
  const [volume, setVolume] = useState(DEFAULT_VOLUME);
  const [presetIndex, setPresetIndex] = useState(0);

  const start = useCallback(async () => {
    await audioEngine.start();
    setStarted(true);
  }, []);

  const changeVolume = useCallback((db: number) => {
    audioEngine.setVolume(db);
    setVolume(db);
  }, []);

  const changePreset = useCallback((index: number) => {
    audioEngine.setPreset(index);
    setPresetIndex(index);
  }, []);

  return { started, volume, presetIndex, start, changeVolume, changePreset };
}
