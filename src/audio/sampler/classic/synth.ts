/**
 * Pure (Tone-free, DOM-free) classic voice synthesis: safe to import in a Web
 * Worker (./classic.worker.ts) as well as on the main thread.
 */
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
