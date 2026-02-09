import { useCallback, useEffect, useMemo, useRef } from 'react';
import { PianoKey } from '@/components/PianoKey';
import { PIANO_START, PIANO_END } from '@/utils/constants';
import { isBlackKey, midiToNoteName } from '@/utils/noteHelpers';
import { cn } from '@/lib/utils';
import type { ActiveNote } from '@/hooks/useMidiNotes';

interface PianoRollProps {
  activeNotes: Map<number, ActiveNote>;
  onNoteOn: (note: number, velocity: number) => void;
  onNoteOff: (note: number) => void;
  highlightedNotes?: Set<number>;
  compact?: boolean;
}

export function PianoRoll({ activeNotes, onNoteOn, onNoteOff, highlightedNotes, compact }: PianoRollProps) {
  const dragging = useRef(false);
  const currentDragNote = useRef<number | null>(null);

  const { whiteKeys, blackKeys, whiteIndexMap } = useMemo(() => {
    const white: number[] = [];
    const black: number[] = [];
    const indexMap = new Map<number, number>();
    for (let i = PIANO_START; i <= PIANO_END; i++) {
      if (isBlackKey(i)) {
        // Pre-compute the white key index for this black key's position
        indexMap.set(i, white.length);
        black.push(i);
      } else {
        white.push(i);
      }
    }
    return { whiteKeys: white, blackKeys: black, whiteIndexMap: indexMap };
  }, []);

  const whiteKeyWidth = compact ? 32 : 40;
  const whiteKeyGap = 1;
  const whiteKeyStep = whiteKeyWidth + whiteKeyGap;
  const blackKeyOffset = compact ? 12 : 15;

  const handleKeyDown = useCallback(
    (note: number) => {
      dragging.current = true;
      currentDragNote.current = note;
      onNoteOn(note, 0.7);
    },
    [onNoteOn],
  );

  const handleKeyUp = useCallback(
    (_note: number) => {
      if (dragging.current && currentDragNote.current !== null) {
        onNoteOff(currentDragNote.current);
      }
      dragging.current = false;
      currentDragNote.current = null;
    },
    [onNoteOff],
  );

  const handleKeyEnter = useCallback(
    (note: number) => {
      if (!dragging.current) return;
      // Slide to new key: release old, press new
      if (currentDragNote.current !== null && currentDragNote.current !== note) {
        onNoteOff(currentDragNote.current);
      }
      if (currentDragNote.current !== note) {
        currentDragNote.current = note;
        onNoteOn(note, 0.7);
      }
    },
    [onNoteOn, onNoteOff],
  );

  // Handle pointer release outside the piano area
  useEffect(() => {
    const handleGlobalPointerUp = () => {
      if (dragging.current && currentDragNote.current !== null) {
        onNoteOff(currentDragNote.current);
      }
      dragging.current = false;
      currentDragNote.current = null;
    };

    window.addEventListener('pointerup', handleGlobalPointerUp);
    return () => window.removeEventListener('pointerup', handleGlobalPointerUp);
  }, [onNoteOff]);

  // Safety: release held note if the keyboard unmounts mid-drag
  useEffect(() => {
    return () => {
      if (dragging.current && currentDragNote.current !== null) {
        onNoteOff(currentDragNote.current);
      }
      dragging.current = false;
      currentDragNote.current = null;
    };
  }, [onNoteOff]);

  return (
    <div className={cn(
      'flex flex-col items-center justify-end',
      compact ? 'pb-2' : 'flex-1 pb-6',
    )}>
      {/* Octave labels */}
      <div className="relative mb-2 flex">
        <div className="flex" style={{ gap: `${whiteKeyGap}px` }}>
          {whiteKeys.map((k) => (
            <div key={k} className="flex items-center justify-center" style={{ width: whiteKeyWidth }}>
              {k % 12 === 0 && (
                <span className="font-mono text-[10px] font-medium text-muted-foreground">
                  {midiToNoteName(k)}
                </span>
              )}
            </div>
          ))}
        </div>
      </div>
      {/* Piano keys */}
      <div className="relative flex" style={{ gap: `${whiteKeyGap}px` }}>
        {/* White keys */}
        {whiteKeys.map((k) => (
          <PianoKey
            key={k}
            midiNote={k}
            isActive={activeNotes.has(k)}
            isHighlighted={highlightedNotes ? highlightedNotes.has(k % 12) : false}
            onPointerDown={handleKeyDown}
            onPointerUp={handleKeyUp}
            onPointerEnter={handleKeyEnter}
            showLabel={!compact}
            compact={compact}
          />
        ))}
        {/* Black keys overlaid */}
        {blackKeys.map((k) => {
          const whiteIndex = whiteIndexMap.get(k)!;
          const left = whiteIndex * whiteKeyStep - blackKeyOffset;
          return (
            <div
              key={k}
              className="absolute top-0"
              style={{ left: `${left}px` }}
            >
              <PianoKey
                midiNote={k}
                isActive={activeNotes.has(k)}
                isHighlighted={highlightedNotes ? highlightedNotes.has(k % 12) : false}
                onPointerDown={handleKeyDown}
                onPointerUp={handleKeyUp}
                onPointerEnter={handleKeyEnter}
                showLabel={false}
                compact={compact}
              />
            </div>
          );
        })}
      </div>
    </div>
  );
}
