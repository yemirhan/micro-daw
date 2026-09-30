import { useState, useCallback } from 'react';
import { audioEngine } from '@/services/AudioEngine';
import { SYNTH_PRESETS, DEFAULT_VOLUME } from '@/utils/constants';

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

  const setSynthParam = useCallback((key: string, value: unknown) => {
    audioEngine.setSynthParam(key, value);
  }, []);

  const getCurrentPreset = useCallback(() => {
    return SYNTH_PRESETS[presetIndex] ?? SYNTH_PRESETS[0];
  }, [presetIndex]);

  return { started, volume, presetIndex, start, changeVolume, changePreset, setSynthParam, getCurrentPreset };
}
