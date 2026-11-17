import * as Tone from 'tone';
import type { ClassicMachineId } from '@/types/sampler';
import { SR, finalize } from './dsp';
import { CLASSIC_RECIPES } from './voices';

/** Sample rate of every rendered classic voice (mono). */
export const CLASSIC_SAMPLE_RATE = SR;

export function hasClassicRecipe(machine: ClassicMachineId, voice: string): boolean {
  return Object.prototype.hasOwnProperty.call(CLASSIC_RECIPES[machine] ?? {}, voice);
}

/**
 * Pure: synthesize a classic voice as mono 44.1 kHz samples, normalized to
 * -1 dBFS with the silent tail trimmed. Deterministic (seeded noise).
 */
export function synthesizeClassicVoice(machine: ClassicMachineId, voice: string): Float32Array {
  if (!hasClassicRecipe(machine, voice)) throw new Error(`Unknown classic voice: ${machine}/${voice}`);
  const recipe = CLASSIC_RECIPES[machine][voice];
  return finalize(recipe.render(), { maxSec: recipe.maxSec });
}

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

/**
 * Render a classic voice to an AudioBuffer (mono, 44.1 kHz). Not cached here:
 * go through `loadPadBuffer({ kind: 'classic', machine, voice })` for that.
 * Yields to the event loop first so loading a whole kit doesn't block a frame
 * per voice back to back.
 */
export async function renderClassicVoice(machine: ClassicMachineId, voice: string): Promise<AudioBuffer> {
  await new Promise<void>((resolve) => setTimeout(resolve, 0));
  return createMonoBuffer(synthesizeClassicVoice(machine, voice), CLASSIC_SAMPLE_RATE);
}
