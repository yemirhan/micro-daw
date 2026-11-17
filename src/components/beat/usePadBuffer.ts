import { useEffect, useReducer, useState } from 'react';
import { getCachedPadBuffer, loadPadBuffer, onPadBuffersInvalidated, packIdOfSource, padSourceKey } from '@/audio/sampler/bufferCache';
import type { PadSampleSource } from '@/types/sampler';

export type PadBufferState =
  | { status: 'empty'; buffer: null }
  | { status: 'loading'; buffer: null }
  | { status: 'ready'; buffer: AudioBuffer }
  | { status: 'missing'; buffer: null };

const EMPTY: PadBufferState = { status: 'empty', buffer: null };
const LOADING: PadBufferState = { status: 'loading', buffer: null };
const MISSING: PadBufferState = { status: 'missing', buffer: null };

function initial(source: PadSampleSource | null | undefined): PadBufferState {
  if (!source) return EMPTY;
  const cached = getCachedPadBuffer(source);
  return cached ? { status: 'ready', buffer: cached } : LOADING;
}

/**
 * The decoded buffer of a pad sample (cached; loads on demand). 'missing' = the
 * file could not be loaded. Pack sounds reload when their pack is installed,
 * updated or removed (the cache key carries the pack's generation).
 */
export function usePadBuffer(source: PadSampleSource | null | undefined): PadBufferState {
  const key = source ? padSourceKey(source) : '';
  const packId = source ? packIdOfSource(source) : null;
  // Re-render on invalidation of this pad's pack: padSourceKey() then returns the new key.
  const [, bump] = useReducer((n: number) => n + 1, 0);
  useEffect(() => {
    if (!packId) return;
    return onPadBuffersInvalidated((id) => {
      if (id === packId) bump();
    });
  }, [packId]);
  const [state, setState] = useState<{ key: string; value: PadBufferState }>(() => ({ key, value: initial(source) }));

  // Reset synchronously when the sample changes (no flash of the previous waveform).
  let current = state;
  if (state.key !== key) {
    current = { key, value: initial(source) };
    setState(current);
  }

  useEffect(() => {
    if (!source || current.value.status !== 'loading') return;
    let alive = true;
    loadPadBuffer(source).then(
      (buffer) => alive && setState({ key, value: { status: 'ready', buffer } }),
      () => alive && setState({ key, value: MISSING }),
    );
    return () => {
      alive = false;
    };
    // `source` identity may change every render; the key is what matters.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, current.value.status]);

  return current.value;
}
