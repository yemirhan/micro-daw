import { useMemo } from 'react';
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
  const keys = useMemo(() => {
    const result: number[] = [];
    for (let i = PIANO_START; i <= PIANO_END; i++) {
      result.push(i);
    }
    return result;
  }, []);

  const whiteKeys = keys.filter((k) => !isBlackKey(k));
  const blackKeys = keys.filter((k) => isBlackKey(k));

  const whiteKeyWidth = compact ? 32 : 40;
  const whiteKeyGap = 1;
  const whiteKeyStep = whiteKeyWidth + whiteKeyGap;
  const blackKeyOffset = compact ? 12 : 15;

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
            onNoteOn={onNoteOn}
            onNoteOff={onNoteOff}
            showLabel={!compact}
            compact={compact}
          />
        ))}
        {/* Black keys overlaid */}
        {blackKeys.map((k) => {
          const whiteIndex = whiteKeys.filter((w) => w < k).length;
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
                onNoteOn={onNoteOn}
                onNoteOff={onNoteOff}
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
