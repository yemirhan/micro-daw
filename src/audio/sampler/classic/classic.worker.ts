/**
 * Web Worker: synthesizes classic voices off the main thread (a cold kit is
 * 15–60 ms of DSP, enough to starve note repeat's 40 ms look-ahead). The
 * samples come back as a transferred Float32Array; see ./render.ts.
 */
import type { ClassicMachineId } from '@/types/sampler';
import { synthesizeClassicVoice } from './synth';

export interface ClassicWorkerRequest {
  id: number;
  machine: ClassicMachineId;
  voice: string;
}

export type ClassicWorkerResponse =
  | { id: number; samples: Float32Array }
  | { id: number; error: string };

const scope = self as unknown as {
  onmessage: ((event: MessageEvent<ClassicWorkerRequest>) => void) | null;
  postMessage(message: ClassicWorkerResponse, transfer?: Transferable[]): void;
};

scope.onmessage = (event) => {
  const { id, machine, voice } = event.data;
  try {
    const samples = synthesizeClassicVoice(machine, voice);
    scope.postMessage({ id, samples }, [samples.buffer]);
  } catch (err) {
    scope.postMessage({ id, error: err instanceof Error ? err.message : String(err) });
  }
};
