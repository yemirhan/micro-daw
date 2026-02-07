import { useState, useCallback } from 'react';
import { audioEngine } from '@/services/AudioEngine';
import { drumEngine } from '@/services/DrumEngine';

export function useAudioInput() {
  const [muted, setMuted] = useState(false);

  const toggleMute = useCallback(() => {
    setMuted((prev) => {
      const next = !prev;
      if (next) {
        audioEngine.mute();
        drumEngine.mute();
      } else {
        audioEngine.unmute();
        drumEngine.unmute();
      }
      return next;
    });
  }, []);

  return { muted, toggleMute };
}
