import { useEffect, useLayoutEffect, useRef } from 'react';
import { noteEventBus, type NoteEvent } from '@/services/NoteEventBus';

/**
 * Subscribe to the global note event stream (see NoteEventBus). The handler
 * may change every render; the subscription itself is stable.
 */
export function useNoteEvents(handler: (event: NoteEvent) => void, enabled = true): void {
  const handlerRef = useRef(handler);
  useLayoutEffect(() => {
    handlerRef.current = handler;
  });

  useEffect(() => {
    if (!enabled) return;
    return noteEventBus.subscribe((event) => handlerRef.current(event));
  }, [enabled]);
}
