/**
 * What to do with pads whose pack can't be downloaded any more (the catalog no
 * longer lists it, review L-c): clear them, or put the built-in factory kit's
 * sound for the same pad back on them. One undo step either way.
 */
import { createFactoryKit } from '@/audio/sampler/factoryKit';
import { arrangementEngine } from '@/services/ArrangementEngine';
import type { PadIndex, PadSample, PadSettings } from '@/types/sampler';

export type MissingPadFix = 'clear' | 'builtin';

export interface PadFix {
  pad: PadIndex;
  sample: PadSample | null;
  /** Settings reset with the new sample (the old trim belonged to the old file). */
  patch: Partial<PadSettings> | null;
}

/** Pure: the sample (and setting resets) each pad gets for `fix`. */
export function planMissingPadFix(pads: readonly PadIndex[], fix: MissingPadFix): PadFix[] {
  const factory = fix === 'builtin' ? createFactoryKit() : null;
  return pads.map((pad) => {
    const sample = factory?.pads[pad]?.sample ?? null;
    if (!sample) return { pad, sample: null, patch: null };
    return { pad, sample: { ...sample, source: { ...sample.source } }, patch: { startSec: 0, endSec: null, reverse: false } };
  });
}

/** Apply `fix` to `pads` of the drum track (one undo step). */
export function fixMissingPads(trackId: string, pads: readonly PadIndex[], fix: MissingPadFix): void {
  const plan = planMissingPadFix(pads, fix);
  if (plan.length === 0) return;
  arrangementEngine.beginUndoGroup(fix === 'clear' ? 'Clear pads' : 'Replace pads');
  try {
    for (const { pad, sample, patch } of plan) {
      arrangementEngine.setPadSample(trackId, pad, sample);
      if (patch) arrangementEngine.updatePad(trackId, pad, patch);
    }
  } finally {
    arrangementEngine.endUndoGroup();
  }
}
