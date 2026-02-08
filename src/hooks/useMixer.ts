import { useState, useEffect, useRef, useCallback } from 'react';
import { arrangementEngine } from '@/services/ArrangementEngine';
import type { Track } from '@/types/arrangement';

interface MixerLevels {
  tracks: Map<string, number>;
  master: number;
}

export function useMixer(tracks: Track[], active: boolean) {
  const [levels, setLevels] = useState<MixerLevels>({ tracks: new Map(), master: -Infinity });
  const rafRef = useRef(0);

  useEffect(() => {
    if (!active) {
      if (rafRef.current) cancelAnimationFrame(rafRef.current);
      return;
    }

    const update = () => {
      const trackLevels = new Map<string, number>();
      for (const track of tracks) {
        trackLevels.set(track.id, arrangementEngine.getTrackLevel(track.id));
      }
      setLevels({
        tracks: trackLevels,
        master: arrangementEngine.getMasterLevel(),
      });
      rafRef.current = requestAnimationFrame(update);
    };

    rafRef.current = requestAnimationFrame(update);
    return () => {
      if (rafRef.current) cancelAnimationFrame(rafRef.current);
    };
  }, [tracks, active]);

  return levels;
}
