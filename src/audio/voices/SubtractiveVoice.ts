import * as Tone from 'tone';
import type { SubtractivePreset } from '@/types/audio';
import type { ModBus, Voice } from './types';
import {
  HARD_STOP_FADE,
  MIN_GATE,
  MonotonicClock,
  clamp,
  gainToDb,
  midiToFreq,
  omniOptions,
  oscDetune,
  safeDispose,
  startSource,
  type StartStop,
  stopSource,
  updateOmni,
  velocityGain,
} from './common';

type OmniOpts = ConstructorParameters<typeof Tone.OmniOscillator>[0];
type AnySource = StartStop;

/** White/pink noise is much hotter than a tonal oscillator at the same peak. */
const NOISE_TRIM = 0.45;
const MAX_CUTOFF = 18000;

/**
 * Classic analog-style voice:
 *
 *   oscA (unison) --+
 *   oscB (unison) --+--> mixer --> filter (cutoff + envelope/LFO via detune) --> amp env --> out
 *   sub osc -------+
 *   noise -> env --+
 *
 * Pitch is a single frequency signal shared by all oscillators (glide /
 * pitch envelope act on it). The filter envelope is applied in CENTS on the
 * filter's detune, so sweeps are exponential (musical) and their depth can be
 * set per note (velocity) with sample accuracy.
 */
export class SubtractiveVoice implements Voice<SubtractivePreset> {
  readonly output: Tone.AmplitudeEnvelope;

  private p: SubtractivePreset;
  private readonly freq: Tone.Signal<'frequency'>;
  private readonly oscA: Tone.OmniOscillator<Tone.Oscillator>;
  private readonly oscB: Tone.OmniOscillator<Tone.Oscillator>;
  private readonly sub: Tone.Oscillator;
  private readonly noise: Tone.Noise;
  private readonly noiseEnv: Tone.AmplitudeEnvelope;
  private readonly mixer: Tone.Gain;
  private readonly filter: Tone.Filter;
  private readonly filterEnv: Tone.Envelope;
  private readonly envDepth: Tone.Gain;
  private readonly clock = new MonotonicClock();
  private note = 60;
  private velocity = 0.8;
  private sources: AnySource[];

  constructor(
    preset: SubtractivePreset,
    private readonly mod: ModBus | null,
  ) {
    this.p = preset;
    this.freq = new Tone.Signal({ value: 261.63, units: 'frequency' });
    this.oscA = new Tone.OmniOscillator(omniOptions(preset.oscA) as OmniOpts);
    this.oscB = new Tone.OmniOscillator(omniOptions(preset.oscB) as OmniOpts);
    this.sub = new Tone.Oscillator({ type: preset.sub.wave });
    this.noise = new Tone.Noise({ type: preset.noise.type });
    this.noiseEnv = new Tone.AmplitudeEnvelope({ attack: 0.001, decay: 0.05, sustain: 1, release: 0.05 });
    this.mixer = new Tone.Gain(1);
    this.filter = new Tone.Filter({
      type: preset.filter.type,
      frequency: preset.filter.cutoff,
      Q: preset.filter.resonance,
      rolloff: preset.filter.rolloff,
    });
    this.filterEnv = new Tone.Envelope({ ...preset.filterEnv });
    this.envDepth = new Tone.Gain(0);
    this.output = new Tone.AmplitudeEnvelope({ ...this.ampEnvOptions(preset) });

    // pitch
    this.freq.connect(this.oscA.frequency);
    this.freq.connect(this.oscB.frequency);
    this.freq.connect(this.sub.frequency);
    // audio
    this.oscA.connect(this.mixer);
    this.oscB.connect(this.mixer);
    this.sub.connect(this.mixer);
    this.noise.connect(this.noiseEnv);
    this.noiseEnv.connect(this.mixer);
    this.mixer.connect(this.filter);
    this.filter.connect(this.output);
    // filter envelope (cents, additive on detune)
    this.filterEnv.connect(this.envDepth);
    this.envDepth.connect(this.filter.detune);
    // modulation
    if (mod) {
      mod.vibrato.connect(this.oscA.detune);
      mod.vibrato.connect(this.oscB.detune);
      mod.vibrato.connect(this.sub.detune);
      mod.filter.connect(this.filter.detune);
    }
    this.sources = [this.oscA, this.oscB, this.sub, this.noise] as unknown as AnySource[];
    this.applyStatic(preset);
    // `omniOptions` carries no detune: tune the oscillators here, as `update()` does via
    // `updateOmni`, so a voice built from a preset (or after a live edit) plays its semi/fine.
    this.oscA.detune.value = oscDetune(preset.oscA);
    this.oscB.detune.value = oscDetune(preset.oscB);
  }

  // -------------------------------------------------------------------------
  // Note events
  // -------------------------------------------------------------------------

  attack(midi: number, time: number, velocity: number, glideFrom?: number | null): void {
    const t = this.clock.next(time);
    const p = this.p;
    this.note = midi;
    this.velocity = velocity;
    const f = midiToFreq(midi);

    if (p.pitchEnv.amount !== 0 && p.pitchEnv.decay > 0) {
      this.freq.cancelScheduledValues(t);
      this.freq.setValueAtTime(f * Math.pow(2, p.pitchEnv.amount / 12), t);
      this.freq.exponentialRampToValueAtTime(f, t + p.pitchEnv.decay);
    } else if (glideFrom != null && p.glide > 0.001) {
      this.freq.exponentialRampTo(f, p.glide, t);
    } else {
      this.freq.cancelScheduledValues(t);
      this.freq.setValueAtTime(f, t);
    }

    this.startSources(t);
    this.filter.frequency.setValueAtTime(this.cutoffFor(midi, velocity), t);
    this.envDepth.gain.setValueAtTime(this.envCents(midi, velocity), t);
    this.filterEnv.triggerAttack(t);
    this.noiseEnv.triggerAttack(t);
    this.output.triggerAttack(t, velocityGain(velocity, p.amp.velocity));
  }

  glide(midi: number, time: number): void {
    const t = Math.max(time, this.clock.lastTime);
    this.note = midi;
    const g = Math.max(0.005, this.p.glide);
    this.freq.exponentialRampTo(midiToFreq(midi), g, t);
    this.filter.frequency.linearRampTo(this.cutoffFor(midi, this.velocity), g, t);
  }

  release(time: number): number {
    const t = Math.max(time, this.clock.lastTime + MIN_GATE);
    this.output.triggerRelease(t);
    this.filterEnv.triggerRelease(t);
    this.noiseEnv.triggerRelease(t);
    const end = t + this.releaseDuration();
    this.stopSources(end + 0.05);
    return end;
  }

  hardStop(time: number): number {
    const t = Math.max(time, this.clock.lastTime + 0.003);
    const rel = this.output.release;
    const nRel = this.noiseEnv.release;
    this.output.release = HARD_STOP_FADE;
    this.noiseEnv.release = HARD_STOP_FADE;
    this.output.triggerRelease(t);
    this.noiseEnv.triggerRelease(t);
    this.filterEnv.triggerRelease(t);
    this.output.release = rel;
    this.noiseEnv.release = nRel;
    const end = t + HARD_STOP_FADE;
    this.stopSources(end + 0.03);
    return end;
  }

  releaseDuration(): number {
    return Math.max(this.p.amp.release, 0.005);
  }

  // -------------------------------------------------------------------------
  // Parameters
  // -------------------------------------------------------------------------

  update(preset: SubtractivePreset): void {
    const prev = this.p;
    this.p = preset;
    updateOmni(this.oscA, preset.oscA);
    updateOmni(this.oscB, preset.oscB);
    if (prev.sub.wave !== preset.sub.wave) this.sub.type = preset.sub.wave;
    if (prev.noise.type !== preset.noise.type) this.noise.type = preset.noise.type;
    if (prev.filter.type !== preset.filter.type) this.filter.type = preset.filter.type;
    if (prev.filter.rolloff !== preset.filter.rolloff) this.filter.rolloff = preset.filter.rolloff;
    this.filterEnv.set({ ...preset.filterEnv });
    this.output.set(this.ampEnvOptions(preset));
    this.applyStatic(preset);
    // Follow cutoff/resonance changes on the sounding note.
    const now = Tone.now();
    this.filter.frequency.linearRampTo(this.cutoffFor(this.note, this.velocity), 0.03, now);
    this.envDepth.gain.linearRampTo(this.envCents(this.note, this.velocity), 0.03, now);
  }

  dispose(): void {
    if (this.mod) {
      for (const dst of [this.oscA.detune, this.oscB.detune, this.sub.detune]) {
        try {
          this.mod.vibrato.disconnect(dst);
        } catch {
          /* not connected */
        }
      }
      try {
        this.mod.filter.disconnect(this.filter.detune);
      } catch {
        /* not connected */
      }
    }
    safeDispose(
      this.freq,
      this.oscA,
      this.oscB,
      this.sub,
      this.noise,
      this.noiseEnv,
      this.mixer,
      this.filter,
      this.filterEnv,
      this.envDepth,
      this.output,
    );
  }

  // -------------------------------------------------------------------------

  private ampEnvOptions(p: SubtractivePreset) {
    return {
      attack: Math.max(0.001, p.amp.attack),
      decay: Math.max(0.001, p.amp.decay),
      sustain: clamp(p.amp.sustain, 0, 1),
      release: Math.max(0.005, p.amp.release),
    };
  }

  /** Levels, tuning, resonance -- everything that is a plain value. */
  private applyStatic(p: SubtractivePreset): void {
    const noiseSustained = p.noise.decay <= 0;
    const sum =
      p.oscA.level + p.oscB.level + p.sub.level + (noiseSustained ? p.noise.level * NOISE_TRIM : 0);
    const norm = 1 / Math.max(1, sum);
    this.oscA.volume.value = gainToDb(p.oscA.level * norm);
    this.oscB.volume.value = gainToDb(p.oscB.level * norm);
    this.sub.volume.value = gainToDb(p.sub.level * norm);
    this.noise.volume.value = gainToDb(p.noise.level * norm * NOISE_TRIM);
    this.sub.detune.value = Math.round(clamp(p.sub.octave, -2, -1)) * 1200;
    this.filter.Q.value = clamp(p.filter.resonance, 0.1, 30);
    if (noiseSustained) {
      this.noiseEnv.set({ attack: 0.001, decay: 0.01, sustain: 1, release: Math.max(0.005, p.amp.release) });
    } else {
      this.noiseEnv.set({ attack: 0.001, decay: p.noise.decay, sustain: 0, release: p.noise.decay });
    }
  }

  private cutoffFor(midi: number, velocity: number): number {
    const f = this.p.filter;
    const keyed = f.cutoff * Math.pow(2, (f.keyTrack * (midi - 60)) / 12);
    const vel = Math.pow(2, f.velocity * (velocity - 0.75) * 1.5);
    return clamp(keyed * vel, 20, MAX_CUTOFF);
  }

  private envCents(midi: number, velocity: number): number {
    const f = this.p.filter;
    const octaves = f.envAmount * (1 - f.velocity + f.velocity * velocity);
    if (octaves <= 0) return Math.max(-8, octaves) * 1200;
    const base = this.cutoffFor(midi, velocity);
    const headroom = Math.max(0, Math.log2(20000 / base));
    return Math.min(octaves, headroom) * 1200;
  }

  private startSources(t: number): void {
    const p = this.p;
    const levels = [p.oscA.level, p.oscB.level, p.sub.level, p.noise.level];
    this.sources.forEach((src, i) => {
      if (levels[i] > 0) startSource(src, t);
    });
  }

  private stopSources(t: number): void {
    // Source.stop() is a no-op for sources that never started; scheduled
    // (future) starts are covered too, unlike checking `state` at now().
    for (const src of this.sources) stopSource(src, t);
  }
}
