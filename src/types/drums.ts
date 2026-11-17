import type { PadIndex } from './sampler';

/**
 * Legacy name for a pad index. The sampler has 16 pads (see `PadIndex` in
 * `@/types/sampler`); this alias keeps older call sites compiling.
 */
export type DrumPadId = PadIndex;

export interface DrumSound {
  id: DrumPadId;
  name: string;
  shortName: string;
  midiNote: number;
  color: string;
}
