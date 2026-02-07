import { useState, useCallback, useRef } from 'react';
import type { DrumPadId } from '@/types/drums';

export function useDrumPads() {
  const [activePads, setActivePads] = useState<Set<DrumPadId>>(new Set());
  const timeouts = useRef<Map<DrumPadId, ReturnType<typeof setTimeout>>>(new Map());

  // Visual-only — audio + recording is handled by noteOn in useMidiNotes
  const flashPad = useCallback((padId: DrumPadId) => {
    setActivePads((prev) => {
      const next = new Set(prev);
      next.add(padId);
      return next;
    });

    const prev = timeouts.current.get(padId);
    if (prev) clearTimeout(prev);

    const t = setTimeout(() => {
      setActivePads((prev) => {
        const next = new Set(prev);
        next.delete(padId);
        return next;
      });
      timeouts.current.delete(padId);
    }, 150);
    timeouts.current.set(padId, t);
  }, []);

  return { activePads, flashPad };
}
