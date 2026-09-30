import * as Tone from 'tone';
import type { SynthPreset } from '@/types/audio';
import { SYNTH_PRESETS, MAX_POLYPHONY } from '@/utils/constants';

/**
 * Unified interface for all synth types so AudioEngine/trackAudioFactory
 * don't need to know about engine specifics.
 */
export interface SynthPlayer {
  triggerAttack(note: string, time?: number, velocity?: number): void;
  triggerRelease(note: string, time?: number): void;
  triggerAttackRelease(note: string, duration: number | string, time?: number, velocity?: number): void;
  releaseAll(time?: number): void;
  connect(node: Tone.ToneAudioNode): SynthPlayer;
  set(params: Record<string, unknown>): void;
  dispose(): void;
}

/** Wraps Tone.PolySynth to implement SynthPlayer */
class PolySynthPlayer implements SynthPlayer {
  constructor(private synth: Tone.PolySynth) {}
  triggerAttack(note: string, time?: number, velocity?: number) {
    this.synth.triggerAttack(note, time, velocity);
  }
  triggerRelease(note: string, time?: number) {
    this.synth.triggerRelease(note, time);
  }
  triggerAttackRelease(note: string, duration: number | string, time?: number, velocity?: number) {
    this.synth.triggerAttackRelease(note, duration, time, velocity);
  }
  releaseAll(time?: number) {
    this.synth.releaseAll(time);
  }
  connect(node: Tone.ToneAudioNode) {
    this.synth.connect(node);
    return this;
  }
  set(params: Record<string, unknown>) {
    this.synth.set(params as any);
  }
  dispose() {
    this.synth.releaseAll();
    this.synth.dispose();
  }
}

/** Wraps monophonic synths (MonoSynth, MembraneSynth) */
class MonoPlayer implements SynthPlayer {
  constructor(
    private synth: Tone.MonoSynth | Tone.MembraneSynth,
    private isPercussive = false,
  ) {}
  triggerAttack(note: string, time?: number, velocity?: number) {
    this.synth.triggerAttack(note, time, velocity);
  }
  triggerRelease(_note: string, time?: number) {
    if (!this.isPercussive) this.synth.triggerRelease(time);
  }
  triggerAttackRelease(note: string, duration: number | string, time?: number, velocity?: number) {
    this.synth.triggerAttackRelease(note, duration, time, velocity);
  }
  releaseAll(time?: number) {
    this.synth.triggerRelease(time);
  }
  connect(node: Tone.ToneAudioNode) {
    this.synth.connect(node);
    return this;
  }
  set(params: Record<string, unknown>) {
    this.synth.set(params as any);
  }
  dispose() {
    this.synth.dispose();
  }
}

/** Wraps PluckSynth — triggerRelease is a no-op */
class PluckPlayer implements SynthPlayer {
  constructor(private synth: Tone.PluckSynth) {}
  triggerAttack(note: string, time?: number, _velocity?: number) {
    this.synth.triggerAttack(note, time);
  }
  triggerRelease() {
    // PluckSynth has no release
  }
  triggerAttackRelease(note: string, _duration: number | string, time?: number, _velocity?: number) {
    this.synth.triggerAttack(note, time);
  }
  releaseAll() {
    // no-op
  }
  connect(node: Tone.ToneAudioNode) {
    this.synth.connect(node);
    return this;
  }
  set(params: Record<string, unknown>) {
    this.synth.set(params as any);
  }
  dispose() {
    this.synth.dispose();
  }
}

/**
 * Creates a SynthPlayer from a SynthPreset.
 * Handles all engine types and wraps them in a uniform interface.
 */
export function createSynthFromPreset(preset: SynthPreset): SynthPlayer {
  switch (preset.engine) {
    case 'synth': {
      const synth = new Tone.PolySynth(Tone.Synth, {
        maxPolyphony: MAX_POLYPHONY,
        oscillator: preset.oscillator as Tone.OmniOscillatorOptions,
        envelope: preset.envelope,
      } as any);
      return new PolySynthPlayer(synth);
    }
    case 'fmsynth': {
      const synth = new Tone.PolySynth(Tone.FMSynth, {
        maxPolyphony: MAX_POLYPHONY,
        oscillator: preset.oscillator as Tone.OmniOscillatorOptions,
        envelope: preset.envelope,
        modulationIndex: preset.modulationIndex,
        harmonicity: preset.harmonicity,
        ...(preset.modulationEnvelope ? { modulationEnvelope: preset.modulationEnvelope } : {}),
      } as any);
      return new PolySynthPlayer(synth);
    }
    case 'amsynth': {
      const synth = new Tone.PolySynth(Tone.AMSynth, {
        maxPolyphony: MAX_POLYPHONY,
        oscillator: preset.oscillator as Tone.OmniOscillatorOptions,
        envelope: preset.envelope,
        harmonicity: preset.harmonicity,
        ...(preset.modulationEnvelope ? { modulationEnvelope: preset.modulationEnvelope } : {}),
      } as any);
      return new PolySynthPlayer(synth);
    }
    case 'monosynth': {
      const synth = new Tone.MonoSynth({
        oscillator: preset.oscillator as Tone.OmniOscillatorOptions,
        envelope: preset.envelope,
        filterEnvelope: preset.filterEnvelope,
      } as any);
      return new MonoPlayer(synth);
    }
    case 'plucksynth': {
      const synth = new Tone.PluckSynth({
        attackNoise: preset.attackNoise,
        resonance: preset.resonance,
        dampening: preset.dampening,
        release: preset.release,
      });
      return new PluckPlayer(synth);
    }
    case 'membranesynth': {
      const synth = new Tone.MembraneSynth({
        oscillator: preset.oscillator as Tone.OmniOscillatorOptions,
        envelope: preset.envelope,
        pitchDecay: preset.pitchDecay,
        octaves: preset.octaves,
      } as any);
      return new MonoPlayer(synth, true);
    }
  }
}

/**
 * Resolves a preset by index, optionally merging overrides on top.
 */
export function resolvePreset(presetIndex: number, overrides?: Record<string, unknown>): SynthPreset {
  const base = SYNTH_PRESETS[presetIndex] ?? SYNTH_PRESETS[0];
  if (!overrides || Object.keys(overrides).length === 0) return base;

  // Deep merge overrides onto base preset
  const merged = structuredClone(base) as unknown as Record<string, unknown>;
  for (const [key, value] of Object.entries(overrides)) {
    if (value !== null && typeof value === 'object' && !Array.isArray(value) && typeof merged[key] === 'object') {
      merged[key] = { ...(merged[key] as Record<string, unknown>), ...(value as Record<string, unknown>) };
    } else {
      merged[key] = value;
    }
  }
  return merged as unknown as SynthPreset;
}
