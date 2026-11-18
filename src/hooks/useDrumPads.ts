import { useSyncExternalStore } from 'react';
import type { DrumPadId } from '@/types/drums';

/** How long a hit pad stays lit. */
export const PAD_FLASH_MS = 150;

/**
 * Visual-only pad flash state (audio + recording is handled by noteOn in
 * useMidiNotes). A module store, not React state of the caller: flashing a
 * pad re-renders only components that read `useActivePads()`, never the host
 * that wires MIDI (App). Review E4: App used to re-render twice per pad hit.
 */
let activePads: ReadonlySet<DrumPadId> = new Set();
const timeouts = new Map<DrumPadId, ReturnType<typeof setTimeout>>();
const listeners = new Set<() => void>();

function emit(next: ReadonlySet<DrumPadId>): void {
  activePads = next;
  for (const l of [...listeners]) l();
}

export const padFlash = {
  /** Light pad `padId` for PAD_FLASH_MS (re-hits extend it). */
  flash(padId: DrumPadId): void {
    if (!activePads.has(padId)) emit(new Set(activePads).add(padId));
    const prev = timeouts.get(padId);
    if (prev) clearTimeout(prev);
    timeouts.set(
      padId,
      setTimeout(() => {
        timeouts.delete(padId);
        const next = new Set(activePads);
        next.delete(padId);
        emit(next);
      }, PAD_FLASH_MS),
    );
  },
  getSnapshot: (): ReadonlySet<DrumPadId> => activePads,
  subscribe(listener: () => void): () => void {
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  },
};

/** Pads lit right now (re-renders the caller on every flash). */
export function useActivePads(): ReadonlySet<DrumPadId> {
  return useSyncExternalStore(padFlash.subscribe, padFlash.getSnapshot);
}

/** Legacy shape: `{ activePads, flashPad }` (subscribes the caller to flashes). */
export function useDrumPads() {
  return { activePads: useActivePads(), flashPad: padFlash.flash };
}
