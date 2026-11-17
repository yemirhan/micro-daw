import * as Tone from 'tone';
import type { PluckPreset } from '@/types/audio';
import type { Voice } from './types';
import { HARD_STOP_FADE, MIN_GATE, MonotonicClock, clamp, midiToFreq, safeDispose, velocityGain } from './common';

/** Karplus-Strong output is ~11 dB quieter than the other engines at the same settings. */
const PLUCK_MAKEUP = 3.5;

/**
 * Karplus-Strong string (Tone.PluckSynth) with velocity -> level and
 * velocity -> brightness (loop dampening). One PluckSynth per voice so chords
 * ring out instead of cutting each other off.
 */
export class PluckVoice implements Voice<PluckPreset> {
  readonly output: Tone.Gain;
  private p: PluckPreset;
  private readonly pluck: Tone.PluckSynth;
  private readonly clock = new MonotonicClock();

  constructor(preset: PluckPreset) {
    this.p = preset;
    this.pluck = new Tone.PluckSynth({
      attackNoise: preset.attackNoise,
      dampening: preset.dampening,
      resonance: clamp(preset.resonance, 0, 0.999),
      release: preset.release,
    });
    this.output = new Tone.Gain(0);
    this.pluck.connect(this.output);
  }

  attack(midi: number, time: number, velocity: number): void {
    const t = this.clock.next(time);
    const p = this.p;
    const v = clamp(velocity, 0, 1);
    // Harder pluck = brighter string. (Immediate setter; fine for a plucked attack.)
    this.pluck.dampening = clamp(p.dampening * Math.pow(2, p.velocity * (v - 0.7) * 2), 300, 16000);
    this.pluck.resonance = clamp(p.resonance, 0, 0.999);
    this.output.gain.cancelScheduledValues(t);
    this.output.gain.setValueAtTime(velocityGain(v, p.velocity) * PLUCK_MAKEUP, t);
    try {
      this.pluck.triggerAttack(midiToFreq(midi), t);
    } catch {
      this.pluck.triggerAttack(midiToFreq(midi), t + 0.001);
    }
  }

  glide(): void {
    // Strings don't glide; legato re-plucks are handled by the instrument.
  }

  release(time: number): number {
    const t = Math.max(time, this.clock.lastTime + MIN_GATE);
    this.pluck.triggerRelease(t);
    return t + this.releaseDuration();
  }

  hardStop(time: number): number {
    const t = Math.max(time, this.clock.lastTime + 0.003);
    this.output.gain.cancelAndHoldAtTime(t);
    this.output.gain.linearRampToValueAtTime(0, t + HARD_STOP_FADE);
    const rel = this.pluck.release;
    this.pluck.release = HARD_STOP_FADE;
    this.pluck.triggerRelease(t);
    this.pluck.release = rel;
    return t + HARD_STOP_FADE;
  }

  releaseDuration(): number {
    return Math.max(0.01, Number(this.p.release) || 0.5);
  }

  update(preset: PluckPreset): void {
    this.p = preset;
    this.pluck.attackNoise = clamp(preset.attackNoise, 0.1, 20);
    this.pluck.release = preset.release;
  }

  dispose(): void {
    safeDispose(this.pluck, this.output);
  }
}
