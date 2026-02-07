import { useState, useMemo } from 'react';
import { SCALE_DEFINITIONS, getScaleNotes } from '@/utils/scales';
import { detectChord } from '@/utils/chordDetection';
import type { ScaleName } from '@/types/music';
import type { ChordInfo } from '@/types/music';

export function useLearningMode(activeNotes: Map<number, unknown>) {
  const [selectedRoot, setSelectedRoot] = useState(0); // C
  const [selectedScale, setSelectedScale] = useState<ScaleName | null>(null);

  const highlightedNotes = useMemo(() => {
    if (!selectedScale) return new Set<number>();
    const scale = SCALE_DEFINITIONS.find((s) => s.name === selectedScale);
    if (!scale) return new Set<number>();
    return getScaleNotes(selectedRoot, scale);
  }, [selectedRoot, selectedScale]);

  const detectedChord: ChordInfo | null = useMemo(() => {
    const notes = Array.from(activeNotes.keys());
    return detectChord(notes);
  }, [activeNotes]);

  return {
    selectedRoot,
    setSelectedRoot,
    selectedScale,
    setSelectedScale,
    highlightedNotes,
    detectedChord,
  };
}
