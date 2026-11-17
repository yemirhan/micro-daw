import * as Tone from 'tone';
import type { DrumPadId } from '@/types/drums';
import { MonotonicClock, clamp, normVelocity, safeDispose, startSource, velocityGain } from './voices/common';

type GainParam = Tone.Param<'gain'>;

/** 808 metallic oscillator bank frequencies (Hz). */
const METAL_FREQS = [205.3, 304.4, 369.6, 522.7, 540, 800];

/** GM note -> tom pitch (Hz). */
const TOM_PITCH: Record<number, number> = { 41: 82, 43: 98, 45: 118, 47: 138, 48: 158, 50: 185 };

export type DrumVoice = 'kick' | 'snare' | 'rim' | 'chh' | 'phh' | 'ohh' | 'clap' | 'tom' | 'crash' | 'ride';

/** General MIDI percussion -> kit voice. Pads use DRUM_SOUNDS notes (36,38,42,46,39,45,49,51). */
export const GM_DRUM_MAP: Record<number, DrumVoice> = {
  35: 'kick', 36: 'kick',
  37: 'rim', 38: 'snare', 40: 'snare',
  39: 'clap',
  42: 'chh', 44: 'phh', 46: 'ohh',
  41: 'tom', 43: 'tom', 45: 'tom', 47: 'tom', 48: 'tom', 50: 'tom',
  49: 'crash', 52: 'crash', 55: 'crash', 57: 'crash',
  51: 'ride', 53: 'ride', 59: 'ride',
};

const PAD_VOICE: Record<DrumPadId, DrumVoice> = {
  0: 'kick', 1: 'snare', 2: 'chh', 3: 'ohh', 4: 'clap', 5: 'tom', 6: 'crash', 7: 'ride',
};

/** Percussive envelope on a gain param: (re)attack from the current value, short hold, exponential decay. */
function pulse(param: GainParam, t: number, peak: number, tau: number, hold = 0.002, attack = 0.0008): void {
  param.cancelAndHoldAtTime(t);
  param.linearRampToValueAtTime(peak, t + attack);
  param.setTargetAtTime(0, t + attack + hold, tau);
}

/**
 * Synthesized drum kit (808/909-flavoured, tuned for clarity in a mix):
 *  - kick: sine with fast pitch drop + band-passed click + gentle saturation
 *  - snare: two pitched body oscillators + bright filtered noise (wires)
 *  - hats: 6-oscillator square "metal" bank -> band-pass/high-pass + noise sizzle; closed chokes open
 *  - clap: band-passed noise with 3 rapid re-triggers and a short tail
 *  - toms: pitched sines (GM 41..50 give 6 pitches) with stick noise
 *  - crash/ride: metal bank + noise through different filters, long tails
 *
 * Every event is scheduled with sample-accurate automation at `time`, so the
 * same kit works for live pads, sequencer playback and offline export.
 * Continuous sources (noise, metal bank) start lazily on the first hit.
 */
export class DrumKit {
  readonly output: Tone.Gain;

  private started = false;
  private readonly noise: Tone.Noise;
  private readonly metal: Tone.Oscillator[];
  private readonly metalMix: Tone.Gain;

  private readonly kickOsc: Tone.Oscillator;
  private readonly kickAmp: Tone.Gain;
  private readonly kickClickBP: Tone.Filter;
  private readonly kickClickAmp: Tone.Gain;
  private readonly kickBus: Tone.Gain;
  private readonly kickShaper: Tone.WaveShaper;
  private readonly kickClock = new MonotonicClock();

  private readonly snareOsc1: Tone.Oscillator;
  private readonly snareOsc2: Tone.Oscillator;
  private readonly snareBodyAmp: Tone.Gain;
  private readonly snareNoiseHP: Tone.Filter;
  private readonly snareNoiseBP: Tone.Filter;
  private readonly snareNoiseAmp: Tone.Gain;
  private readonly snareBus: Tone.Gain;
  private readonly snareShaper: Tone.WaveShaper;
  private readonly snareClock = new MonotonicClock();

  private readonly clapBP: Tone.Filter;
  private readonly clapAmp: Tone.Gain;

  private readonly hatBP: Tone.Filter;
  private readonly hatHP: Tone.Filter;
  private readonly hatNoiseHP: Tone.Filter;
  private readonly hatNoiseGain: Tone.Gain;
  private readonly closedAmp: Tone.Gain;
  private readonly openAmp: Tone.Gain;

  private readonly crashBP: Tone.Filter;
  private readonly crashNoiseHP: Tone.Filter;
  private readonly crashAmp: Tone.Gain;
  private readonly rideBP: Tone.Filter;
  private readonly rideNoiseHP: Tone.Filter;
  private readonly rideAmp: Tone.Gain;

  private readonly tomOsc: Tone.Oscillator;
  private readonly tomAmp: Tone.Gain;
  private readonly tomNoiseBP: Tone.Filter;
  private readonly tomNoiseAmp: Tone.Gain;
  private readonly tomClock = new MonotonicClock();
  private readonly extraNodes: Tone.ToneAudioNode[] = [];

  constructor() {
    this.output = new Tone.Gain(1);
    this.noise = new Tone.Noise({ type: 'white' });

    // Metal bank
    this.metalMix = new Tone.Gain(1 / METAL_FREQS.length);
    this.metal = METAL_FREQS.map((f) => {
      const o = new Tone.Oscillator({ type: 'square', frequency: f });
      o.connect(this.metalMix);
      return o;
    });

    // Kick
    this.kickBus = new Tone.Gain(1);
    this.kickShaper = new Tone.WaveShaper((x: number) => Math.tanh(1.6 * x) / Math.tanh(1.6), 1024);
    this.kickOsc = new Tone.Oscillator({ type: 'sine', frequency: 50 });
    this.kickAmp = new Tone.Gain(0);
    this.kickClickBP = new Tone.Filter({ type: 'bandpass', frequency: 3200, Q: 0.8 });
    this.kickClickAmp = new Tone.Gain(0);
    this.kickOsc.chain(this.kickAmp, this.kickBus);
    this.noise.connect(this.kickClickBP);
    this.kickClickBP.chain(this.kickClickAmp, this.kickBus);
    this.kickBus.chain(this.kickShaper, this.output);

    // Snare (body + wires -> soft saturation: rounds noise peaks, adds crack)
    this.snareBus = new Tone.Gain(1);
    this.snareShaper = new Tone.WaveShaper((x: number) => Math.tanh(1.4 * x) / Math.tanh(1.4), 1024);
    this.snareBus.chain(this.snareShaper, this.output);
    this.snareOsc1 = new Tone.Oscillator({ type: 'triangle', frequency: 185 });
    this.snareOsc2 = new Tone.Oscillator({ type: 'sine', frequency: 330 });
    this.snareBodyAmp = new Tone.Gain(0);
    this.snareOsc1.connect(this.snareBodyAmp);
    this.snareOsc2.connect(this.snareBodyAmp);
    this.snareBodyAmp.connect(this.snareBus);
    this.snareNoiseHP = new Tone.Filter({ type: 'highpass', frequency: 1400, rolloff: -12 });
    this.snareNoiseBP = new Tone.Filter({ type: 'lowpass', frequency: 7500, rolloff: -12 });
    this.snareNoiseAmp = new Tone.Gain(0);
    this.noise.connect(this.snareNoiseHP);
    this.snareNoiseHP.chain(this.snareNoiseBP, this.snareNoiseAmp, this.snareBus);

    // Clap
    this.clapBP = new Tone.Filter({ type: 'bandpass', frequency: 1150, Q: 1.5 });
    this.clapAmp = new Tone.Gain(0);
    this.noise.connect(this.clapBP);
    this.clapBP.chain(this.clapAmp, this.output);

    // Hats
    this.hatBP = new Tone.Filter({ type: 'bandpass', frequency: 10000, Q: 0.9 });
    this.hatHP = new Tone.Filter({ type: 'highpass', frequency: 7000, rolloff: -24 });
    this.hatNoiseHP = new Tone.Filter({ type: 'highpass', frequency: 8500, rolloff: -12 });
    this.hatNoiseGain = new Tone.Gain(0.3);
    this.closedAmp = new Tone.Gain(0);
    this.openAmp = new Tone.Gain(0);
    this.metalMix.connect(this.hatBP);
    this.hatBP.connect(this.hatHP);
    this.noise.connect(this.hatNoiseHP);
    this.hatNoiseHP.connect(this.hatNoiseGain);
    for (const amp of [this.closedAmp, this.openAmp]) {
      this.hatHP.connect(amp);
      this.hatNoiseGain.connect(amp);
      amp.connect(this.output);
    }

    // Cymbals
    this.crashBP = new Tone.Filter({ type: 'bandpass', frequency: 5200, Q: 0.35 });
    this.crashNoiseHP = new Tone.Filter({ type: 'highpass', frequency: 4500, rolloff: -12 });
    this.crashAmp = new Tone.Gain(0);
    this.metalMix.connect(this.crashBP);
    this.crashBP.connect(this.crashAmp);
    this.noise.connect(this.crashNoiseHP);
    this.crashNoiseHP.connect(this.crashAmp);
    this.crashAmp.connect(this.output);

    this.rideBP = new Tone.Filter({ type: 'bandpass', frequency: 5000, Q: 2.5 });
    this.rideNoiseHP = new Tone.Filter({ type: 'highpass', frequency: 7500, rolloff: -12 });
    this.rideAmp = new Tone.Gain(0);
    this.metalMix.connect(this.rideBP);
    this.rideBP.connect(this.rideAmp);
    this.noise.connect(this.rideNoiseHP);
    const rideNoiseLevel = new Tone.Gain(0.25);
    this.rideNoiseHP.chain(rideNoiseLevel, this.rideAmp);
    this.rideAmp.connect(this.output);
    this.extraNodes.push(rideNoiseLevel);

    // Toms
    this.tomOsc = new Tone.Oscillator({ type: 'sine', frequency: 118 });
    this.tomAmp = new Tone.Gain(0);
    this.tomOsc.chain(this.tomAmp, this.output);
    this.tomNoiseBP = new Tone.Filter({ type: 'bandpass', frequency: 500, Q: 1 });
    this.tomNoiseAmp = new Tone.Gain(0);
    this.noise.connect(this.tomNoiseBP);
    this.tomNoiseBP.chain(this.tomNoiseAmp, this.output);
  }

  connect(node: Tone.InputNode): this {
    this.output.connect(node);
    return this;
  }

  /** Trigger one of the 8 pads. */
  trigger(pad: DrumPadId, velocity: number, time: number): void {
    const voice = PAD_VOICE[pad];
    if (voice) this.play(voice, velocity, time, voice === 'tom' ? 45 : undefined);
  }

  /** Trigger by GM drum note. Returns false for unmapped notes. */
  triggerMidi(note: number, velocity: number, time: number): boolean {
    const voice = GM_DRUM_MAP[note];
    if (!voice) return false;
    this.play(voice, velocity, time, note);
    return true;
  }

  play(voice: DrumVoice, velocity: number, time: number, note?: number): void {
    const v = velocityGain(normVelocity(velocity), 0.85);
    const vel = normVelocity(velocity);
    this.ensureStarted(time);
    switch (voice) {
      case 'kick':
        return this.kick(time, v);
      case 'snare':
        return this.snare(time, v, vel, 1);
      case 'rim':
        return this.snare(time, v * 0.7, vel, 0.35);
      case 'clap':
        return this.clap(time, v);
      case 'chh':
        return this.hat(time, v, false);
      case 'phh':
        return this.hat(time, v * 0.6, false);
      case 'ohh':
        return this.hat(time, v, true);
      case 'tom':
        return this.tom(time, v, TOM_PITCH[note ?? 45] ?? 118);
      case 'crash':
        return this.crash(time, v);
      case 'ride':
        return this.ride(time, v);
    }
  }

  dispose(): void {
    safeDispose(
      this.noise, ...this.metal, this.metalMix,
      this.kickOsc, this.kickAmp, this.kickClickBP, this.kickClickAmp, this.kickBus, this.kickShaper,
      this.snareOsc1, this.snareOsc2, this.snareBodyAmp, this.snareNoiseHP, this.snareNoiseBP, this.snareNoiseAmp, this.snareBus, this.snareShaper,
      this.clapBP, this.clapAmp,
      this.hatBP, this.hatHP, this.hatNoiseHP, this.hatNoiseGain, this.closedAmp, this.openAmp,
      this.crashBP, this.crashNoiseHP, this.crashAmp, this.rideBP, this.rideNoiseHP, this.rideAmp,
      this.tomOsc, this.tomAmp, this.tomNoiseBP, this.tomNoiseAmp,
      ...this.extraNodes,
      this.output,
    );
  }

  // -------------------------------------------------------------------------

  private ensureStarted(time: number): void {
    if (this.started) return;
    this.started = true;
    startSource(this.noise, time);
    for (const o of this.metal) startSource(o, time);
  }

  private kick(time: number, v: number): void {
    const t = this.kickClock.next(time);
    const f = this.kickOsc.frequency;
    f.cancelScheduledValues(t);
    f.setValueAtTime(190, t);
    f.exponentialRampToValueAtTime(56, t + 0.045);
    f.exponentialRampToValueAtTime(46, t + 0.35);
    startSource(this.kickOsc, t);
    this.kickOsc.stop(t + 0.9);
    pulse(this.kickAmp.gain, t, 0.8 * v, 0.1, 0.03);
    pulse(this.kickClickAmp.gain, t, 0.45 * v, 0.004, 0.001);
  }

  private snare(time: number, v: number, vel: number, noiseAmount: number): void {
    const t = this.snareClock.next(time);
    for (const [osc, f] of [[this.snareOsc1, 185], [this.snareOsc2, 330]] as const) {
      osc.frequency.cancelScheduledValues(t);
      osc.frequency.setValueAtTime(f * 1.35, t);
      osc.frequency.exponentialRampToValueAtTime(f, t + 0.03);
      startSource(osc, t);
      osc.stop(t + 0.5);
    }
    pulse(this.snareBodyAmp.gain, t, 0.75 * v, 0.045, 0.004);
    this.snareNoiseBP.frequency.setValueAtTime(5000 + 4000 * vel, t);
    pulse(this.snareNoiseAmp.gain, t, 0.85 * v * noiseAmount, 0.075, 0.006);
  }

  private clap(t: number, v: number): void {
    const g = this.clapAmp.gain;
    g.cancelAndHoldAtTime(t);
    const bursts = [0, 0.011, 0.022];
    for (const b of bursts) {
      g.setValueAtTime(2.6 * v, t + b);
      g.setTargetAtTime(0.02, t + b + 0.0005, 0.003);
    }
    g.setValueAtTime(2.8 * v, t + 0.033);
    g.setTargetAtTime(0, t + 0.034, 0.07);
  }

  private hat(t: number, v: number, open: boolean): void {
    if (open) {
      pulse(this.openAmp.gain, t, 1.25 * v, 0.14, 0.01);
    } else {
      // Choke the open hat, like a real hi-hat pedal.
      this.openAmp.gain.cancelAndHoldAtTime(t);
      this.openAmp.gain.linearRampToValueAtTime(0, t + 0.01);
      pulse(this.closedAmp.gain, t, 1.5 * v, 0.016, 0.002);
    }
  }

  private tom(time: number, v: number, f: number): void {
    const t = this.tomClock.next(time);
    const fr = this.tomOsc.frequency;
    fr.cancelScheduledValues(t);
    fr.setValueAtTime(f * 1.6, t);
    fr.exponentialRampToValueAtTime(f, t + 0.05);
    fr.exponentialRampToValueAtTime(f * 0.9, t + 0.5);
    startSource(this.tomOsc, t);
    this.tomOsc.stop(t + 1.2);
    const tau = clamp(0.2 * Math.sqrt(118 / f), 0.12, 0.3);
    pulse(this.tomAmp.gain, t, 0.62 * v, tau, 0.01);
    this.tomNoiseBP.frequency.setValueAtTime(f * 5, t);
    pulse(this.tomNoiseAmp.gain, t, 0.25 * v, 0.015, 0.001);
  }

  private crash(t: number, v: number): void {
    const g = this.crashAmp.gain;
    g.cancelAndHoldAtTime(t);
    g.linearRampToValueAtTime(0.42 * v, t + 0.003);
    g.setTargetAtTime(0.19 * v, t + 0.003, 0.06);
    g.setTargetAtTime(0, t + 0.15, 0.6);
  }

  private ride(t: number, v: number): void {
    pulse(this.rideAmp.gain, t, 1.05 * v, 0.28, 0.003);
  }
}
