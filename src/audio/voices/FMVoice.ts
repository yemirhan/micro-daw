import * as Tone from 'tone';
import type { FMPreset } from '@/types/audio';
import type { ModBus, Voice } from './types';
import {
  HARD_STOP_FADE,
  MIN_GATE,
  MonotonicClock,
  clamp,
  gainToDb,
  midiToFreq,
  safeDispose,
  startSource,
  type StartStop,
  stopSource,
  velocityGain,
} from './common';

type AnySource = StartStop;

/**
 * 2-operator FM voice + "ping" partial.
 *
 *   modulator --> depth (Hz, per note) --> modGain (x modEnv 0..1) --+--> carrier.frequency
 *   carrier ---------------------------------------------------------+--> amp env --> out
 *   ping (sine, ratio) --> ping env (fast decay) -------------------------^
 *
 * The modulation depth is set per note in Hz (index * modulator frequency),
 * scaled by velocity and keyboard position, so soft notes are mellow, hard
 * notes "bark" (EPs), and the top octaves stay sweet.
 */
export class FMVoice implements Voice<FMPreset> {
  readonly output: Tone.AmplitudeEnvelope;

  private p: FMPreset;
  private readonly carrier: Tone.Oscillator;
  private readonly modulator: Tone.Oscillator;
  private readonly depth: Tone.Gain;
  private readonly modGain: Tone.Gain;
  private readonly modEnv: Tone.Envelope;
  private readonly ping: Tone.Oscillator;
  private readonly pingEnv: Tone.AmplitudeEnvelope;
  private readonly clock = new MonotonicClock();

  constructor(
    preset: FMPreset,
    private readonly mod: ModBus | null,
  ) {
    this.p = preset;
    this.carrier = new Tone.Oscillator({ type: preset.carrier.wave });
    this.modulator = new Tone.Oscillator({ type: 'sine' });
    this.depth = new Tone.Gain(0);
    this.modGain = new Tone.Gain(0);
    this.modEnv = new Tone.Envelope({ ...preset.modEnv });
    this.ping = new Tone.Oscillator({ type: 'sine' });
    this.pingEnv = new Tone.AmplitudeEnvelope({ attack: 0.001, decay: 0.1, sustain: 0, release: 0.05 });
    this.output = new Tone.AmplitudeEnvelope(this.ampOptions(preset));

    this.modulator.connect(this.depth);
    this.depth.connect(this.modGain);
    this.modEnv.connect(this.modGain.gain);
    this.modGain.connect(this.carrier.frequency);
    this.carrier.connect(this.output);
    this.ping.connect(this.pingEnv);
    this.pingEnv.connect(this.output);
    if (mod) {
      mod.vibrato.connect(this.carrier.detune);
      mod.vibrato.connect(this.modulator.detune);
      mod.vibrato.connect(this.ping.detune);
    }
    this.applyStatic(preset);
  }

  attack(midi: number, time: number, velocity: number): void {
    const t = this.clock.next(time);
    const p = this.p;
    const f = midiToFreq(midi);
    const ratio = Math.max(0.01, p.ratio);

    this.carrier.frequency.cancelScheduledValues(t);
    this.carrier.frequency.setValueAtTime(f, t);
    this.modulator.frequency.setValueAtTime(f * ratio, t);
    this.ping.frequency.setValueAtTime(Math.min(f * Math.max(0.5, p.ping.ratio), 18000), t);

    const velIdx = 1 - p.velocityToIndex + p.velocityToIndex * clamp(velocity, 0, 1);
    const keyIdx = clamp(Math.pow(2, (-p.keyScale * (midi - 60)) / 24), 0.15, 2);
    const index = Math.max(0, p.index) * velIdx * keyIdx;
    this.depth.gain.setValueAtTime(index * f * ratio, t);

    startSource(this.carrier as unknown as AnySource, t);
    if (index > 0) startSource(this.modulator as unknown as AnySource, t);
    if (p.ping.level > 0) startSource(this.ping as unknown as AnySource, t);

    this.modEnv.triggerAttack(t);
    this.pingEnv.triggerAttack(t, velocityGain(velocity, 0.6));
    this.output.triggerAttack(t, velocityGain(velocity, p.amp.velocity));
  }

  glide(midi: number, time: number): void {
    const t = Math.max(time, this.clock.lastTime);
    const f = midiToFreq(midi);
    this.carrier.frequency.exponentialRampTo(f, 0.03, t);
    this.modulator.frequency.exponentialRampTo(f * Math.max(0.01, this.p.ratio), 0.03, t);
  }

  release(time: number): number {
    const t = Math.max(time, this.clock.lastTime + MIN_GATE);
    this.output.triggerRelease(t);
    this.modEnv.triggerRelease(t);
    this.pingEnv.triggerRelease(t);
    const end = t + this.releaseDuration();
    this.stopSources(end + 0.05);
    return end;
  }

  hardStop(time: number): number {
    const t = Math.max(time, this.clock.lastTime + 0.003);
    const rel = this.output.release;
    this.output.release = HARD_STOP_FADE;
    this.output.triggerRelease(t);
    this.output.release = rel;
    this.modEnv.triggerRelease(t);
    this.pingEnv.triggerRelease(t);
    const end = t + HARD_STOP_FADE;
    this.stopSources(end + 0.03);
    return end;
  }

  releaseDuration(): number {
    return Math.max(0.005, this.p.amp.release);
  }

  update(preset: FMPreset): void {
    const prev = this.p;
    this.p = preset;
    if (prev.carrier.wave !== preset.carrier.wave) this.carrier.type = preset.carrier.wave;
    this.modEnv.set({ ...preset.modEnv });
    this.output.set(this.ampOptions(preset));
    this.applyStatic(preset);
  }

  dispose(): void {
    if (this.mod) {
      for (const dst of [this.carrier.detune, this.modulator.detune, this.ping.detune]) {
        try {
          this.mod.vibrato.disconnect(dst);
        } catch {
          /* not connected */
        }
      }
    }
    safeDispose(
      this.carrier,
      this.modulator,
      this.depth,
      this.modGain,
      this.modEnv,
      this.ping,
      this.pingEnv,
      this.output,
    );
  }

  private ampOptions(p: FMPreset) {
    return {
      attack: Math.max(0.001, p.amp.attack),
      decay: Math.max(0.001, p.amp.decay),
      sustain: clamp(p.amp.sustain, 0, 1),
      release: Math.max(0.005, p.amp.release),
    };
  }

  private applyStatic(p: FMPreset): void {
    const pingLevel = clamp(p.ping.level, 0, 1);
    this.carrier.volume.value = gainToDb(1 / (1 + pingLevel * 0.5));
    this.ping.volume.value = gainToDb(pingLevel);
    this.carrier.detune.value = p.detune;
    const d = Math.max(0.005, p.ping.decay);
    this.pingEnv.set({ attack: 0.001, decay: d, sustain: 0, release: Math.min(d, 0.2) });
  }

  private stopSources(t: number): void {
    for (const src of [this.carrier, this.modulator, this.ping]) stopSource(src as unknown as AnySource, t);
  }
}
