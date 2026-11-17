import * as Tone from 'tone';
import type { ClassicMachineId } from '@/types/sampler';
import ClassicWorker from './classic.worker?worker&inline';
import type { ClassicWorkerRequest, ClassicWorkerResponse } from './classic.worker';
import { CLASSIC_SAMPLE_RATE, synthesizeClassicVoice } from './synth';

export { CLASSIC_SAMPLE_RATE, hasClassicRecipe, synthesizeClassicVoice } from './synth';

function createMonoBuffer(data: Float32Array, sampleRate: number): AudioBuffer {
  let buffer: AudioBuffer | null = null;
  if (typeof AudioBuffer !== 'undefined') {
    try {
      buffer = new AudioBuffer({ numberOfChannels: 1, length: data.length, sampleRate });
    } catch {
      /* fall back to the context factory */
    }
  }
  buffer ??= (Tone.getContext().rawContext as BaseAudioContext).createBuffer(1, data.length, sampleRate);
  buffer.getChannelData(0).set(data);
  return buffer;
}

// ---------------------------------------------------------------------------
// Off-main-thread synthesis (review E2)
// ---------------------------------------------------------------------------

interface Pending {
  resolve: (samples: Float32Array) => void;
  reject: (err: Error) => void;
  machine: ClassicMachineId;
  voice: string;
}

/** undefined = not created yet; null = no worker here (tests, failed start): main thread. */
let worker: Worker | null | undefined;
let nextId = 1;
const pending = new Map<number, Pending>();

/** Main-thread synthesis (fallback): yield first so a kit doesn't block one long task. */
async function synthesizeHere(machine: ClassicMachineId, voice: string): Promise<Float32Array> {
  await new Promise<void>((resolve) => setTimeout(resolve, 0));
  return synthesizeClassicVoice(machine, voice);
}

/** The worker broke (failed to load / crashed): finish what it had on the main thread, and use that from now on. */
function abandonWorker(reason: unknown): void {
  if (worker) {
    console.warn('[classic] synthesis worker unavailable, rendering on the main thread', reason);
    try {
      worker.terminate();
    } catch {
      /* already gone */
    }
  }
  worker = null;
  const waiting = [...pending.values()];
  pending.clear();
  for (const p of waiting) synthesizeHere(p.machine, p.voice).then(p.resolve, p.reject);
}

function getWorker(): Worker | null {
  if (worker !== undefined) return worker;
  if (typeof Worker === 'undefined') return (worker = null);
  try {
    const w = new ClassicWorker();
    w.onmessage = (event: MessageEvent<ClassicWorkerResponse>) => {
      const msg = event.data;
      const p = pending.get(msg.id);
      if (!p) return;
      pending.delete(msg.id);
      if ('samples' in msg) p.resolve(msg.samples);
      else p.reject(new Error(msg.error));
    };
    w.onerror = (event) => {
      event.preventDefault?.();
      abandonWorker(event.message || 'worker error');
    };
    w.onmessageerror = () => abandonWorker('message error');
    worker = w;
  } catch (err) {
    worker = null;
    console.warn('[classic] could not start the synthesis worker', err);
  }
  return worker;
}

/**
 * Synthesize a classic voice off the main thread (Web Worker), falling back to
 * the main thread where workers are unavailable (tests) or failed to start.
 */
export function synthesizeClassicVoiceAsync(machine: ClassicMachineId, voice: string): Promise<Float32Array> {
  const w = getWorker();
  if (!w) return synthesizeHere(machine, voice);
  return new Promise<Float32Array>((resolve, reject) => {
    const id = nextId++;
    pending.set(id, { resolve, reject, machine, voice });
    try {
      w.postMessage({ id, machine, voice } satisfies ClassicWorkerRequest);
    } catch (err) {
      pending.delete(id);
      abandonWorker(err);
      synthesizeHere(machine, voice).then(resolve, reject);
    }
  });
}

/** Test hook: forget the worker (next render creates or skips it again). */
export function resetClassicWorkerForTests(): void {
  try {
    worker?.terminate();
  } catch {
    /* ignore */
  }
  worker = undefined;
  pending.clear();
}

/**
 * Render a classic voice to an AudioBuffer (mono, 44.1 kHz). Not cached here:
 * go through `loadPadBuffer({ kind: 'classic', machine, voice })` for that.
 * The DSP runs in a Web Worker, so loading a kit never blocks the main thread
 * (note repeat / the transport keep their timing); only the AudioBuffer copy
 * happens here.
 */
export async function renderClassicVoice(machine: ClassicMachineId, voice: string): Promise<AudioBuffer> {
  return createMonoBuffer(await synthesizeClassicVoiceAsync(machine, voice), CLASSIC_SAMPLE_RATE);
}
