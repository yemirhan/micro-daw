/**
 * Instrument preset model.
 *
 * Every preset is a flat, JSON-serialisable object: identity/metadata fields
 * plus the parameters of ONE engine. Per-track `synthOverrides` are partial
 * objects of the same shape that get deep-merged on top (see resolvePreset).
 *
 * Engines:
 *  - 'subtractive': 2 oscillators (+unison "supersaw") + sub + noise -> per-voice
 *    filter with its own envelope, key tracking, velocity -> amp/filter, pitch
 *    envelope, LFO (vibrato / filter wobble), optional mono/legato + glide.
 *    Custom `partials` on an oscillator turns it into an additive/organ voice.
 *  - 'fm': 2-operator FM voice (modulation envelope, velocity -> index,
 *    keyboard scaling) + an optional decaying "ping" partial (EP tines, mallets).
 *  - 'pluck': Karplus-Strong string (Tone.PluckSynth) per voice, velocity aware.
 *  - 'sampler': multi-sampled acoustic piano (bundled Salamander Grand samples).
 */

export type SynthEngine = 'subtractive' | 'fm' | 'pluck' | 'sampler';
export type PresetCategory = 'keys' | 'bass' | 'lead' | 'pad' | 'pluck' | 'brass' | 'bell' | 'percussion' | 'fx';
export type PresetVibe = 'chill' | 'bright' | 'dark' | 'punchy' | 'dreamy' | 'aggressive' | 'organic' | 'experimental';

export type OscWave = 'sine' | 'triangle' | 'sawtooth' | 'square' | 'pulse';

export interface ADSR {
  attack: number;  // s
  decay: number;   // s
  sustain: number; // 0..1
  release: number; // s
}

export interface OscParams {
  wave: OscWave;
  /** 0..1 mix level (0 = oscillator off, costs no CPU). */
  level: number;
  /** Coarse tuning in semitones. */
  semi: number;
  /** Fine tuning in cents. */
  fine: number;
  /** Unison voices per oscillator (1 = plain, 2..7 = detuned stack / supersaw). */
  unison: number;
  /** Unison detune spread in cents (total). */
  spread: number;
  /** Pulse width (-1..1) for 'pulse'. */
  width?: number;
  /** Additive partial amplitudes (organ drawbars...). Overrides `wave` when set. */
  partials?: number[];
}

export interface SubOscParams {
  level: number; // 0..1
  wave: 'sine' | 'triangle' | 'square';
  octave: number; // -1 or -2
}

export interface NoiseParams {
  level: number; // 0..1
  type: 'white' | 'pink' | 'brown';
  /** 0 = sustained (follows the amp envelope); > 0 = one-shot transient of this decay (s). */
  decay: number;
}

export type VoiceFilterType = 'lowpass' | 'highpass' | 'bandpass' | 'notch';

export interface VoiceFilterParams {
  type: VoiceFilterType;
  cutoff: number;     // Hz (at middle C, before key tracking)
  resonance: number;  // Q
  rolloff: -12 | -24;
  /** 0..1: how much the cutoff follows the played note (1 = fully). */
  keyTrack: number;
  /** Filter envelope depth in octaves (may be negative). */
  envAmount: number;
  /** 0..1: how much velocity scales the envelope depth (and brightness). */
  velocity: number;
}

export interface AmpParams extends ADSR {
  /** 0..1 velocity sensitivity (0 = always full level). */
  velocity: number;
}

export interface PitchEnvParams {
  amount: number; // semitones at attack
  decay: number;  // s to settle on pitch
}

export interface LfoParams {
  rate: number;     // Hz
  vibrato: number;  // cents (pitch depth)
  filter: number;   // cents (cutoff depth)
  /** s before vibrato fades in (approx; per-instrument). */
  delay: number;
}

/** Instrument-level "character" processing after the voices. */
export interface CharacterParams {
  /** 0..1 soft saturation. */
  drive: number;
  /** 0..1 stereo chorus/ensemble amount. */
  chorus: number;
  /** Tremolo / auto-pan depth 0..1 (EP, vibes). */
  tremolo: number;
  tremoloRate: number; // Hz
  /** Output tone: lowpass Hz (20000 = open). */
  tone: number;
  /** High-shelf (3.2 kHz) gain in dB: + brighter / - darker. */
  presence: number;
}

interface PresetBase {
  /** Stable id (saved projects map through presetIndex; ids survive reordering). */
  id: string;
  name: string;
  engine: SynthEngine;
  category: PresetCategory;
  vibe: PresetVibe;
  description?: string;
  /** Output trim in dB (loudness matching between presets). */
  gain: number;
  /** Max simultaneous voices. */
  polyphony: number;
  character: CharacterParams;
}

export interface SubtractivePreset extends PresetBase {
  engine: 'subtractive';
  oscA: OscParams;
  oscB: OscParams;
  sub: SubOscParams;
  noise: NoiseParams;
  filter: VoiceFilterParams;
  filterEnv: ADSR;
  amp: AmpParams;
  pitchEnv: PitchEnvParams;
  lfo: LfoParams;
  /** Monophonic with a last-note-priority note stack. */
  mono: boolean;
  /** Mono only: overlapping notes glide without re-triggering envelopes. */
  legato: boolean;
  /** Portamento time (s). Mono: always; poly: 0 recommended. */
  glide: number;
}

export interface FMPreset extends PresetBase {
  engine: 'fm';
  carrier: { wave: 'sine' | 'triangle' | 'square' | 'sawtooth' };
  /** Modulator : carrier frequency ratio. */
  ratio: number;
  /** Peak modulation index (at full velocity, middle C). */
  index: number;
  /** Modulation index envelope (shape of the brightness over time). */
  modEnv: ADSR;
  /** 0..1 velocity -> modulation index. */
  velocityToIndex: number;
  /** 0..1 reduce index for high notes (keeps the top end sweet). */
  keyScale: number;
  amp: AmpParams;
  /** Decaying sine partial added at note-on (EP tine, mallet strike). */
  ping: { ratio: number; level: number; decay: number };
  /** Carrier detune in cents (adds slow beating against the ping). */
  detune: number;
  lfo: LfoParams;
}

export interface PluckPreset extends PresetBase {
  engine: 'pluck';
  attackNoise: number; // 0.1..20
  dampening: number;   // Hz
  resonance: number;   // 0..0.999
  release: number;     // s
  /** 0..1 velocity -> brightness (dampening). */
  velocity: number;
}

export interface SamplerPreset extends PresetBase {
  engine: 'sampler';
  sample: 'piano';
  /** Damper release (s). */
  release: number;
  /** Velocity curve exponent (1 = linear, >1 = softer touch). */
  velocityCurve: number;
}

export type SynthPreset = SubtractivePreset | FMPreset | PluckPreset | SamplerPreset;
