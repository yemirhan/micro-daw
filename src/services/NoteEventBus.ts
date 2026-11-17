import type { DrumPadId } from '@/types/drums';

/**
 * Synchronous stream of performance events coming out of useMidiNotes (the
 * single source of truth for what the player is doing), regardless of where
 * they came from: MIDI keyboard, MIDI pads, on-screen piano, on-screen pads.
 *
 * Consumers listen here instead of wrapping the on-screen components'
 * callbacks, so MIDI input is handled exactly like mouse input and nothing is lost to React render batching (a quick tap is two
 * events even if React only renders once).
 */
export type NoteEvent =
  | { type: 'noteon'; note: number; velocity: number; channel: number; time: number }
  | { type: 'noteoff'; note: number; channel: number; time: number }
  /**
   * A pad hit, emitted when it is heard. `audioTime` (AudioContext seconds) is
   * set for hits scheduled ahead of time (note repeat): when it plays.
   */
  | { type: 'drum'; padId: DrumPadId; note: number; velocity: number; time: number; audioTime?: number }
  /** Every note was force-released (panic / mode switch). */
  | { type: 'allnotesoff'; time: number };

export type NoteEventListener = (event: NoteEvent) => void;

class NoteEventBus {
  private listeners = new Set<NoteEventListener>();

  subscribe(listener: NoteEventListener): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  emit(event: NoteEvent): void {
    for (const listener of [...this.listeners]) {
      try {
        listener(event);
      } catch (err) {
        // A broken listener must never break note handling (and cause stuck notes).
        console.error('[NoteEventBus] listener failed', err);
      }
    }
  }

  listenerCount(): number {
    return this.listeners.size;
  }
}

export const noteEventBus = new NoteEventBus();

export function eventTime(): number {
  return typeof performance !== 'undefined' ? performance.now() : Date.now();
}
