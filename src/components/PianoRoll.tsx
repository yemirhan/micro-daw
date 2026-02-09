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

  const { whiteKeys, blackKeys, whiteIndexMap, highlightCount } = useMemo(() => {
    const white: number[] = [];
    const black: number[] = [];
    const indexMap = new Map<number, number>();

    for (let i = PIANO_START; i <= PIANO_END; i++) {
      if (isBlackKey(i)) {
        // Pre-compute the white key index for this black key's position.
        indexMap.set(i, white.length);
        black.push(i);
      } else {
        white.push(i);
      }
    }

    let highlights = 0;
    if (highlightedNotes) {
      for (const whiteKey of white) {
        if (highlightedNotes.has(whiteKey % 12)) {
          highlights += 1;
        }
      }
    }

    return { whiteKeys: white, blackKeys: black, whiteIndexMap: indexMap, highlightCount: highlights };
  }, [highlightedNotes]);

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
      // Slide to new key: release old, press new.
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

  // Handle pointer release outside the piano area.
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

  // Safety: release held note if the keyboard unmounts mid-drag.
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
    <div
      className={cn(
        'group relative isolate overflow-hidden border border-white/15 bg-card/30 backdrop-blur-sm',
        compact
          ? 'rounded-xl px-2.5 pb-2 pt-2 shadow-[0_14px_30px_-20px_oklch(0.55_0.18_245_/_0.8)]'
          : 'flex flex-1 flex-col justify-end rounded-2xl px-4 pb-5 pt-4 shadow-[0_28px_65px_-30px_oklch(0.55_0.2_250_/_0.8)]',
      )}
      style={{
        background:
          'radial-gradient(140% 110% at 10% -25%, oklch(0.56 0.22 265 / 0.24), transparent 48%), radial-gradient(120% 100% at 90% -35%, oklch(0.68 0.2 210 / 0.18), transparent 55%), linear-gradient(180deg, oklch(0.18 0.016 265 / 0.9), oklch(0.13 0.012 270 / 0.95))',
      }}
    >
      <div
        aria-hidden
        className="pointer-events-none absolute inset-0 opacity-70"
        style={{
          backgroundImage:
            'linear-gradient(to right, oklch(1 0 0 / 0.04) 1px, transparent 1px), linear-gradient(to bottom, oklch(1 0 0 / 0.03) 1px, transparent 1px)',
          backgroundSize: compact ? '20px 20px, 20px 20px' : '28px 28px, 28px 28px',
          maskImage: 'linear-gradient(180deg, black, transparent 75%)',
        }}
      />

      <div
        aria-hidden
        className="pointer-events-none absolute -left-12 top-0 h-24 w-56 rotate-[-14deg] blur-2xl"
        style={{ background: 'linear-gradient(90deg, oklch(0.64 0.2 260 / 0.34), transparent)' }}
      />

      {!compact && (
        <div className="relative z-10 mb-3 flex items-center justify-between">
          <div>
            <p className="font-mono text-[10px] uppercase tracking-[0.22em] text-white/55">Performance Keyboard</p>
            <p className="font-mono text-[11px] text-white/75">Drag across keys for glissando response</p>
          </div>
          <div className="flex items-center gap-1.5">
            <span className="rounded-full border border-emerald-300/35 bg-emerald-300/12 px-2 py-0.5 font-mono text-[10px] text-emerald-200">
              live {activeNotes.size}
            </span>
            <span className="rounded-full border border-sky-300/35 bg-sky-300/12 px-2 py-0.5 font-mono text-[10px] text-sky-200">
              target {highlightCount}
            </span>
          </div>
        </div>
      )}

      <div className="relative z-10">
        <div className={cn('relative mb-2 flex', compact ? 'justify-start' : 'justify-center')}>
          <div className="flex" style={{ gap: `${whiteKeyGap}px` }}>
            {whiteKeys.map((k) => (
              <div key={k} className="relative flex items-center justify-center" style={{ width: whiteKeyWidth }}>
                {k % 12 === 0 && (
                  <>
                    <span className="absolute inset-x-1 top-0 h-px bg-white/20" />
                    <span className="rounded-md border border-white/15 bg-black/25 px-1.5 py-0.5 font-mono text-[10px] font-medium text-white/75 backdrop-blur-sm">
                      {midiToNoteName(k)}
                    </span>
                  </>
                )}
              </div>
            ))}
          </div>
        </div>

        <div
          className="relative flex"
          style={{ gap: `${whiteKeyGap}px` }}
          aria-label="Playable piano keyboard"
          role="group"
        >
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

          {blackKeys.map((k) => {
            const whiteIndex = whiteIndexMap.get(k)!;
            const left = whiteIndex * whiteKeyStep - blackKeyOffset;
            return (
              <div key={k} className="absolute top-0" style={{ left: `${left}px` }}>
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
    </div>
  );
}
