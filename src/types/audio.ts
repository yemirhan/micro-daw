import type { RecursivePartial } from 'tone/build/esm/core/util/Interface';
import type { OmniOscillatorOptions } from 'tone/build/esm/source/oscillator/OscillatorInterface';

export type SynthEngine = 'synth' | 'fmsynth' | 'amsynth' | 'monosynth' | 'plucksynth' | 'membranesynth';
export type PresetCategory = 'keys' | 'bass' | 'lead' | 'pad' | 'pluck' | 'brass' | 'bell' | 'percussion' | 'fx';
export type PresetVibe = 'chill' | 'bright' | 'dark' | 'punchy' | 'dreamy' | 'aggressive' | 'organic' | 'experimental';

interface SynthPresetBase {
  name: string;
  engine: SynthEngine;
  category: PresetCategory;
  vibe: PresetVibe;
}

export interface BasicSynthPreset extends SynthPresetBase {
  engine: 'synth';
  oscillator: RecursivePartial<OmniOscillatorOptions>;
  envelope: { attack: number; decay: number; sustain: number; release: number };
}

export interface FMSynthPreset extends SynthPresetBase {
  engine: 'fmsynth';
  oscillator: RecursivePartial<OmniOscillatorOptions>;
  envelope: { attack: number; decay: number; sustain: number; release: number };
  modulationIndex: number;
  harmonicity: number;
  modulationEnvelope?: { attack: number; decay: number; sustain: number; release: number };
}

export interface AMSynthPreset extends SynthPresetBase {
  engine: 'amsynth';
  oscillator: RecursivePartial<OmniOscillatorOptions>;
  envelope: { attack: number; decay: number; sustain: number; release: number };
  harmonicity: number;
  modulationEnvelope?: { attack: number; decay: number; sustain: number; release: number };
}

export interface MonoSynthPreset extends SynthPresetBase {
  engine: 'monosynth';
  oscillator: RecursivePartial<OmniOscillatorOptions>;
  envelope: { attack: number; decay: number; sustain: number; release: number };
  filterEnvelope: {
    attack: number;
    decay: number;
    sustain: number;
    release: number;
    baseFrequency: number;
    octaves: number;
  };
}

export interface PluckSynthPreset extends SynthPresetBase {
  engine: 'plucksynth';
  attackNoise: number;
  resonance: number;
  dampening: number;
  release: number;
}

export interface MembraneSynthPreset extends SynthPresetBase {
  engine: 'membranesynth';
  oscillator: RecursivePartial<OmniOscillatorOptions>;
  envelope: { attack: number; decay: number; sustain: number; release: number };
  pitchDecay: number;
  octaves: number;
}

export type SynthPreset =
  | BasicSynthPreset
  | FMSynthPreset
  | AMSynthPreset
  | MonoSynthPreset
  | PluckSynthPreset
  | MembraneSynthPreset;
