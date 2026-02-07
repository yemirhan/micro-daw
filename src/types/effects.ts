export interface EffectParams {
  reverbWet: number;    // 0-1
  chorusDepth: number;  // 0-1
  filterCutoff: number; // Hz (60-18000)
  filterResonance: number; // Q (0-20)
}

export interface CCMapping {
  cc: number;
  effect: keyof EffectParams;
  label: string;
}
