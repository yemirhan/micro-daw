import { useCallback, useEffect, useLayoutEffect, useMemo, useRef } from 'react';
import { PianoKey } from '@/components/PianoKey';
import { PIANO_START, PIANO_END } from '@/utils/constants';
import { isBlackKey, midiToNoteName } from '@/utils/noteHelpers';
import { cn } from '@/lib/utils';
import type { ActiveNote } from '@/hooks/useMidiNotes';

interface PianoRollProps {
  activeNotes: Map<number, ActiveNote>;
  onNoteOn: (note: number, velocity: number) => void;
  onNoteOff: (note: number) => void;
  compact?: boolean;
}

const POINTER_VELOCITY = 0.7;

/**
 * On-screen keyboard. Each pointer (mouse, pen, every finger) holds at most
 * one key; a key is ALWAYS released when its pointer goes up, is cancelled,
 * leaves the keyboard, when the window loses focus / is hidden, or when the
 * keyboard unmounts — so the on-screen keyboard can never leave a stuck note.
 */
export function PianoRoll({ activeNotes, onNoteOn, onNoteOff, compact }: PianoRollProps) {
  /** pointerId -> note held by that pointer */
  const pointerNotes = useRef<Map<number, number>>(new Map());
  // Latest callbacks, so cleanup paths never call a stale handler.
  const onNoteOnRef = useRef(onNoteOn);
  const onNoteOffRef = useRef(onNoteOff);
  useLayoutEffect(() => {
    onNoteOnRef.current = onNoteOn;
    onNoteOffRef.current = onNoteOff;
  });

  const { whiteKeys, blackKeys, whiteIndexMap } = useMemo(() => {
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

    return { whiteKeys: white, blackKeys: black, whiteIndexMap: indexMap };
  }, []);

  const whiteKeyWidth = compact ? 32 : 40;
  const whiteKeyGap = 1;
  const whiteKeyStep = whiteKeyWidth + whiteKeyGap;
  const blackKeyOffset = compact ? 12 : 15;

  const releasePointer = useCallback((pointerId: number) => {
    const note = pointerNotes.current.get(pointerId);
    if (note === undefined) return;
    pointerNotes.current.delete(pointerId);
    // Another pointer may still hold the same key (two fingers): keep it down.
    if (![...pointerNotes.current.values()].includes(note)) onNoteOffRef.current(note);
  }, []);

  const releaseAllPointers = useCallback(() => {
    for (const pointerId of [...pointerNotes.current.keys()]) releasePointer(pointerId);
  }, [releasePointer]);

  const pressPointer = useCallback((pointerId: number, note: number) => {
    const previous = pointerNotes.current.get(pointerId);
    if (previous === note) return;
    if (previous !== undefined) releasePointer(pointerId);
    const alreadyDown = [...pointerNotes.current.values()].includes(note);
    pointerNotes.current.set(pointerId, note);
    if (!alreadyDown) onNoteOnRef.current(note, POINTER_VELOCITY);
  }, [releasePointer]);

  const handleKeyDown = useCallback(
    (note: number, e: React.PointerEvent<HTMLButtonElement>) => {
      // Primary button / touch / pen contact only (right-click opens no note).
      if (e.button !== 0) return;
      e.preventDefault();
      // Touch/pen get implicit pointer capture on the pressed element, which
      // would stop pointerenter on neighbouring keys (no glissando).
      const target = e.currentTarget;
      if (target.hasPointerCapture?.(e.pointerId)) target.releasePointerCapture(e.pointerId);
      pressPointer(e.pointerId, note);
    },
    [pressPointer],
  );

  const handleKeyEnter = useCallback(
    (note: number, e: React.PointerEvent<HTMLButtonElement>) => {
      if (!pointerNotes.current.has(e.pointerId)) return;
      // Mouse button already up (its pointerup happened outside the window).
      if (e.pointerType === 'mouse' && (e.buttons & 1) === 0) {
        releasePointer(e.pointerId);
        return;
      }
      // Slide to new key: release old, press new.
      pressPointer(e.pointerId, note);
    },
    [pressPointer, releasePointer],
  );

  const handleKeyboardLeave = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => releasePointer(e.pointerId),
    [releasePointer],
  );

  // Pointer released / cancelled anywhere, window blurred or hidden: release.
  useEffect(() => {
    const onPointerEnd = (e: PointerEvent) => releasePointer(e.pointerId);
    const onVisibility = () => {
      if (document.visibilityState === 'hidden') releaseAllPointers();
    };
    window.addEventListener('pointerup', onPointerEnd);
    window.addEventListener('pointercancel', onPointerEnd);
    window.addEventListener('blur', releaseAllPointers);
    document.addEventListener('visibilitychange', onVisibility);
    return () => {
      window.removeEventListener('pointerup', onPointerEnd);
      window.removeEventListener('pointercancel', onPointerEnd);
      window.removeEventListener('blur', releaseAllPointers);
      document.removeEventListener('visibilitychange', onVisibility);
      // Safety: release held keys if the keyboard unmounts mid-press.
      releaseAllPointers();
    };
  }, [releasePointer, releaseAllPointers]);

  return (
    <div
      className={cn(
        'group relative isolate overflow-hidden',
        compact
          ? 'rounded-lg border border-border bg-surface-0 px-2 pt-1 pb-2 shadow-[inset_0_1px_3px_oklch(0_0_0/0.35)]'
          : 'flex flex-1 flex-col justify-end rounded-xl border border-border bg-surface-1 px-4 pt-3 pb-4 shadow-[inset_0_1px_0_oklch(1_0_0/0.04)]',
      )}
    >
      {!compact && (
        <div className="relative z-10 mb-3 flex items-center justify-between">
          <p className="daw-label">Keyboard</p>
          <div className="flex items-center gap-1.5 font-mono text-[11px]">
            <span className="flex items-center gap-1.5 rounded-sm bg-surface-3 px-2 py-0.5 text-muted-foreground">
              <span className={cn('size-1.5 rounded-full', activeNotes.size > 0 ? 'bg-primary' : 'bg-surface-4')} />
              held {activeNotes.size}
            </span>
          </div>
        </div>
      )}

      <div className="relative z-10">
        <div className={cn('relative mb-1 flex', compact ? 'justify-start' : 'justify-center')}>
          <div className="flex" style={{ gap: `${whiteKeyGap}px` }}>
            {whiteKeys.map((k) => (
              <div key={k} className="relative flex h-4 items-end" style={{ width: whiteKeyWidth }}>
                {k % 12 === 0 && (
                  <span className="flex items-center gap-1 font-mono text-[10px] font-medium leading-none text-muted-foreground">
                    <span className="h-2.5 w-px bg-border" />
                    {midiToNoteName(k)}
                  </span>
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
          onPointerLeave={handleKeyboardLeave}
          onContextMenu={(e) => e.preventDefault()}
        >
          {whiteKeys.map((k) => (
            <PianoKey
              key={k}
              midiNote={k}
              isActive={activeNotes.has(k)}
              onPointerDown={handleKeyDown}
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
                  onPointerDown={handleKeyDown}
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
