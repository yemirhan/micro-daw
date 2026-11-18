/**
 * N5: per-sample status for Song audio regions: a revision that changes when
 * the sample's audio was replaced in place (redraw the waveform) or its
 * availability changed, and its state ('unavailable': the pack was removed,
 * the clip keeps playing the audio loaded earlier but will not load next
 * time; 'missing': not loaded at all, the clip is silent). Driven by
 * `sampleManager.onSamplesChanged` (reloads + availability changes).
 */
import { useSyncExternalStore } from 'react';
import { sampleManager } from '@/services/SampleManager';

/**
 * 'ok'; 'unavailable': the file could not be re-read (pack removed) but the
 * audio loaded earlier still plays; 'missing': the sample is not loaded at
 * all (its file was gone when the project opened): the clip is silent.
 */
export type SampleState = 'ok' | 'unavailable' | 'missing';

export interface SampleStatus {
  /** Changes whenever the sample's audio was replaced in place or its availability changed. */
  version: number;
  state: SampleState;
}

const listeners = new Set<() => void>();
let stopWatching: (() => void) | null = null;

function notify(): void {
  for (const cb of [...listeners]) cb();
}

function subscribe(cb: () => void): () => void {
  listeners.add(cb);
  stopWatching ??= sampleManager.onSamplesChanged(notify);
  return () => {
    listeners.delete(cb);
    if (listeners.size === 0) {
      stopWatching?.();
      stopWatching = null;
    }
  };
}

export function sampleStateOf(sampleId: string): SampleState {
  if (sampleManager.isUnavailable(sampleId)) return 'unavailable';
  return sampleManager.getSample(sampleId) ? 'ok' : 'missing';
}

/** Snapshot key: primitive, so useSyncExternalStore sees a stable value until something changed. */
function key(sampleId: string | undefined): string {
  if (!sampleId) return '0:ok';
  return `${sampleManager.getRevision(sampleId)}:${sampleStateOf(sampleId)}`;
}

/** Status of an audio region's sample (undefined for MIDI regions: always ok). */
export function useSampleStatus(sampleId: string | undefined): SampleStatus {
  const k = useSyncExternalStore(subscribe, () => key(sampleId), () => key(sampleId));
  const i = k.indexOf(':');
  return { version: Number(k.slice(0, i)), state: k.slice(i + 1) as SampleState };
}
