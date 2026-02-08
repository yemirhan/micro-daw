export interface EffectParams {
  reverbWet: number;    // 0-1
  chorusDepth: number;  // 0-1
  filterCutoff: number; // Hz (60-18000)
  filterResonance: number; // Q (0-20)
  delayTime: number;    // seconds (0-1)
  delayFeedback: number; // 0-1
  delayWet: number;     // 0-1
  distortionAmount: number; // 0-1
  distortionWet: number; // 0-1
  eqLow: number;        // dB (-12 to 12)
  eqMid: number;        // dB (-12 to 12)
  eqHigh: number;       // dB (-12 to 12)
  compThreshold: number; // dB (-60 to 0)
  compRatio: number;     // 1-20
  compAttack: number;    // seconds (0-1)
  compRelease: number;   // seconds (0-1)
}

export const DEFAULT_EFFECT_PARAMS: EffectParams = {
  reverbWet: 0,
  chorusDepth: 0,
  filterCutoff: 18000,
  filterResonance: 1,
  delayTime: 0.25,
  delayFeedback: 0.3,
  delayWet: 0,
  distortionAmount: 0,
  distortionWet: 0,
  eqLow: 0,
  eqMid: 0,
  eqHigh: 0,
  compThreshold: -24,
  compRatio: 4,
  compAttack: 0.003,
  compRelease: 0.25,
};

export interface TrackEffectState {
  reverb: { enabled: boolean; wet: number };
  delay: { enabled: boolean; time: number; feedback: number; wet: number };
  chorus: { enabled: boolean; depth: number };
  distortion: { enabled: boolean; amount: number; wet: number };
  eq: { enabled: boolean; low: number; mid: number; high: number };
  compressor: { enabled: boolean; threshold: number; ratio: number; attack: number; release: number };
  filter: { enabled: boolean; cutoff: number; resonance: number };
}

export const DEFAULT_TRACK_EFFECTS: TrackEffectState = {
  reverb: { enabled: false, wet: 0.3 },
  delay: { enabled: false, time: 0.25, feedback: 0.3, wet: 0.3 },
  chorus: { enabled: false, depth: 0.5 },
  distortion: { enabled: false, amount: 0.3, wet: 0.5 },
  eq: { enabled: false, low: 0, mid: 0, high: 0 },
  compressor: { enabled: false, threshold: -24, ratio: 4, attack: 0.003, release: 0.25 },
  filter: { enabled: false, cutoff: 18000, resonance: 1 },
};

export interface CCMapping {
  cc: number;
  effect: keyof EffectParams;
  label: string;
}
