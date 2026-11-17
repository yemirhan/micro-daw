import type { SynthPreset } from '@/types/audio';
import { SYNTH_PRESETS, FALLBACK_PIANO } from '@/audio/presets';
import { PolyInstrument } from '@/audio/PolyInstrument';
import { SampledPiano } from '@/audio/SampledPiano';
import { SubtractiveVoice } from '@/audio/voices/SubtractiveVoice';
import { FMVoice } from '@/audio/voices/FMVoice';
import { PluckVoice } from '@/audio/voices/PluckVoice';
import { mergePresetParams } from '@/audio/presetParams';
import type { SynthPlayer } from '@/audio/types';

export type { SynthPlayer } from '@/audio/types';

/**
 * Builds the instrument for a preset. Every engine returns the same
 * SynthPlayer interface (voice allocation, live param updates, release-all),
 * so AudioEngine / per-track audio / offline export don't care which it is.
 */
export function createSynthFromPreset(preset: SynthPreset): SynthPlayer {
  switch (preset.engine) {
    case 'subtractive':
      return new PolyInstrument(preset, (p, mod) => new SubtractiveVoice(p, mod));
    case 'fm':
      return new PolyInstrument(preset, (p, mod) => new FMVoice(p, mod));
    case 'pluck':
      return new PolyInstrument(preset, (p) => new PluckVoice(p));
    case 'sampler':
      return new SampledPiano(preset, FALLBACK_PIANO);
    default:
      // Unknown engine (corrupt data): fall back to the default instrument.
      return createSynthFromPreset(SYNTH_PRESETS[0]);
  }
}

/** Preset for a (possibly out-of-range / non-integer) saved index. */
export function presetAt(presetIndex: number): SynthPreset {
  const i = Number.isFinite(presetIndex) ? Math.trunc(presetIndex) : 0;
  return SYNTH_PRESETS[i] ?? SYNTH_PRESETS[0];
}

/**
 * Resolves a preset by index, merging per-track overrides on top. Overrides
 * from older versions (Tone.js synth options) are translated; unknown or
 * ill-typed keys are ignored.
 */
export function resolvePreset(presetIndex: number, overrides?: Record<string, unknown>): SynthPreset {
  return mergePresetParams(presetAt(presetIndex), overrides);
}
