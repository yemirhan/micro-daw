import * as Tone from 'tone';
import type { SynthPreset } from '@/types/audio';
import type { EffectParams, FilterType, LfoWaveform } from '@/types/effects';
import { DEFAULT_EFFECT_PARAMS } from '@/types/effects';
import { SYNTH_PRESETS, DEFAULT_VOLUME } from '@/utils/constants';
import { createSynthFromPreset, resolvePreset, type SynthPlayer } from '@/utils/synthFactory';
import { RoomReverb } from '@/audio/RoomReverb';
import { ensureMasterBus } from '@/audio/masterBus';
import { loadPianoSamples, isPianoLoaded } from '@/audio/pianoSamples';
import { mergeOverrides, pathToObject } from '@/audio/presetParams';

/**
 * Live notes are scheduled at the audio clock's "now" plus this tiny offset,
 * bypassing Tone's look-ahead (100 ms, which every key press used to wait
 * for). 4 ms keeps short attacks from being partially skipped. The Transport
 * keeps Tone's default look-ahead (sequencer timing is unaffected).
 */
const LIVE_OFFSET = 0.004;
/** How long a replaced instrument may ring out before it is disposed. */
const RETIRE_MS = 1200;

type FxNode = Tone.ToneAudioNode | RoomReverb;

/**
 * Live-play engine: the instrument you hear when playing the on-screen piano
 * or a MIDI keyboard.
 *
 * Signal flow:
 *   instrument -> [distortion] -> filter -> [EQ] -> [compressor] -> [chorus] -> [delay] -> [reverb] -> volume -> master bus
 * Stages in brackets are bypassed while they would have no effect (saves CPU
 * and the compressor's 6 ms look-ahead latency).
 */
class AudioEngine {
  private instrument: SynthPlayer | null = null;
  private fxInput: Tone.Gain | null = null;
  private volume: Tone.Volume | null = null;
  private filter: Tone.Filter | null = null;
  private chorus: Tone.Chorus | null = null;
  private reverb: RoomReverb | null = null;
  private delay: Tone.PingPongDelay | null = null;
  private distortion: Tone.Distortion | null = null;
  private eq: Tone.EQ3 | null = null;
  private compressor: Tone.Compressor | null = null;
  private lfo: Tone.LFO | null = null;
  private started = false;
  private starting: Promise<void> | null = null;
  private currentPresetIndex = 0;
  private overrides: Record<string, unknown> = {};
  private activeStages = '';
  private muted = false;

  private effectParams: EffectParams = { ...DEFAULT_EFFECT_PARAMS };

  start(): Promise<void> {
    if (this.started) return Promise.resolve();
    this.starting ??= this.doStart().finally(() => {
      this.starting = null;
    });
    return this.starting;
  }

  private async doStart(): Promise<void> {
    await Tone.start();
    ensureMasterBus();

    const p = this.effectParams;
    this.volume = new Tone.Volume(DEFAULT_VOLUME).toDestination();
    this.volume.mute = this.muted;
    this.fxInput = new Tone.Gain(1);
    this.distortion = new Tone.Distortion({ distortion: p.distortionAmount, wet: p.distortionWet, oversample: '2x' });
    this.filter = new Tone.Filter({ frequency: p.filterCutoff, type: p.filterType, Q: p.filterResonance });
    this.eq = new Tone.EQ3({ low: p.eqLow, mid: p.eqMid, high: p.eqHigh });
    this.compressor = new Tone.Compressor({
      threshold: p.compThreshold,
      ratio: p.compRatio,
      attack: p.compAttack,
      release: p.compRelease,
      knee: 6,
    });
    this.chorus = new Tone.Chorus({
      frequency: 0.7,
      delayTime: 3.5,
      spread: 180,
      depth: chorusDepthFor(p.chorusDepth),
      wet: chorusWetFor(p.chorusDepth),
    }).start();
    this.delay = new Tone.PingPongDelay({ delayTime: p.delayTime, feedback: p.delayFeedback, wet: p.delayWet });
    this.reverb = new RoomReverb({ decay: 1.8, preDelay: 0.022, damping: 0.55, wet: p.reverbWet });
    this.rewire(true);

    this.createInstrument();
    if (p.filterLfoDepth > 0) {
      this.connectLfo();
      this.updateLfoRange();
    }
    this.started = true;

    // Decode the piano samples in the background; the piano preset plays a
    // synthesized fallback until they are ready.
    loadPianoSamples().catch(() => undefined);
  }

  private createInstrument(): void {
    const old = this.instrument;
    if (old) {
      old.releaseAll();
      setTimeout(() => old.dispose(), RETIRE_MS);
    }
    const preset = resolvePreset(this.currentPresetIndex, this.overrides);
    this.instrument = createSynthFromPreset(preset);
    this.instrument.connect(this.fxInput!);
  }

  // -------------------------------------------------------------------------
  // Notes
  // -------------------------------------------------------------------------

  private liveTime(): number {
    return Tone.immediate() + LIVE_OFFSET;
  }

  /** Play a note now. velocity 0..1. Re-striking a sounding note re-uses its voice. */
  noteOn(midiNote: number, velocity: number): void {
    if (!this.instrument || !this.started) return;
    this.instrument.noteOn(midiNote, velocity, this.liveTime());
  }

  /** Release a note. Always honoured (even while muted). */
  noteOff(midiNote: number): void {
    if (!this.instrument) return;
    this.instrument.noteOff(midiNote, this.liveTime());
  }

  /** Silence every live voice (fast fade), including ones whose noteOff was lost. */
  releaseAll(): void {
    this.instrument?.releaseAll(this.liveTime());
  }

  /** Voices still sounding (held + release tails). 0 = nothing audible from the live synth. */
  getActiveVoiceCount(): number {
    return this.instrument?.getActiveVoiceCount() ?? 0;
  }

  /** MIDI notes currently held on the live instrument. */
  getHeldNotes(): number[] {
    return this.instrument?.getHeldNotes() ?? [];
  }

  /** True once the sampled piano is decoded (before that, the piano preset uses a synthesized fallback). */
  isPianoReady(): boolean {
    return isPianoLoaded();
  }

  mute(): void {
    this.muted = true;
    if (this.volume) this.volume.mute = true;
  }

  unmute(): void {
    this.muted = false;
    if (this.volume) this.volume.mute = false;
  }

  setVolume(db: number): void {
    if (this.volume) {
      this.volume.volume.rampTo(db, 0.03);
    }
  }

  // -------------------------------------------------------------------------
  // Presets
  // -------------------------------------------------------------------------

  setPreset(index: number): void {
    if (!Number.isInteger(index) || index < 0 || index >= SYNTH_PRESETS.length) return;
    this.currentPresetIndex = index;
    this.overrides = {};
    if (this.started) this.createInstrument();
  }

  /**
   * Live parameter change on the current instrument, without re-creating it.
   * `key` is a top-level preset key ('filter', value = partial object) or a
   * dotted path ('filter.cutoff', value = number).
   */
  setSynthParam(key: string, value: unknown): void {
    const patch = key.includes('.') ? pathToObject(key, value) : { [key]: value };
    this.overrides = mergeOverrides(this.overrides, patch);
    this.instrument?.set(patch);
  }

  /** Overrides applied on top of the current preset (SynthEditor state). */
  getSynthOverrides(): Record<string, unknown> {
    return structuredClone(this.overrides);
  }

  /** The current preset with live edits applied. */
  getCurrentPreset(): SynthPreset {
    return resolvePreset(this.currentPresetIndex, this.overrides);
  }

  getPresetIndex(): number {
    return this.currentPresetIndex;
  }

  // -------------------------------------------------------------------------
  // Global effects (EffectsPanel / MIDI CC)
  // -------------------------------------------------------------------------

  setReverbWet(value: number): void {
    this.effectParams.reverbWet = value;
    if (this.reverb) {
      this.reverb.wet.rampTo(value, 0.03);
      this.rewire();
    }
  }

  setChorusDepth(value: number): void {
    this.effectParams.chorusDepth = value;
    if (this.chorus) {
      this.chorus.depth = chorusDepthFor(value);
      this.chorus.wet.rampTo(chorusWetFor(value), 0.03);
      this.rewire();
    }
  }

  setFilterCutoff(hz: number): void {
    this.effectParams.filterCutoff = hz;
    if (this.filter) {
      if (!this.lfo) this.filter.frequency.rampTo(hz, 0.02);
      this.updateLfoRange();
    }
  }

  setFilterResonance(q: number): void {
    this.effectParams.filterResonance = q;
    if (this.filter) this.filter.Q.rampTo(q, 0.02);
  }

  setFilterType(type: FilterType): void {
    this.effectParams.filterType = type;
    if (this.filter) this.filter.type = type;
  }

  setFilterLfoRate(rate: number): void {
    this.effectParams.filterLfoRate = rate;
    if (this.lfo) this.lfo.frequency.value = rate;
  }

  setFilterLfoDepth(depth: number): void {
    this.effectParams.filterLfoDepth = depth;
    if (depth === 0) {
      this.disconnectLfo();
    } else {
      this.connectLfo();
      this.updateLfoRange();
    }
  }

  setFilterLfoWave(wave: LfoWaveform): void {
    this.effectParams.filterLfoWave = wave;
    if (this.lfo) this.lfo.type = wave;
  }

  private connectLfo(): void {
    if (!this.filter) return;
    if (!this.lfo) {
      this.lfo = new Tone.LFO({
        frequency: this.effectParams.filterLfoRate,
        type: this.effectParams.filterLfoWave,
      }).start();
      this.lfo.connect(this.filter.frequency);
    }
  }

  private disconnectLfo(): void {
    if (this.lfo) {
      this.lfo.stop();
      this.lfo.dispose();
      this.lfo = null;
      if (this.filter) this.filter.frequency.value = this.effectParams.filterCutoff;
    }
  }

  private updateLfoRange(): void {
    if (!this.lfo || this.effectParams.filterLfoDepth === 0) return;
    const cutoff = this.effectParams.filterCutoff;
    const depth = this.effectParams.filterLfoDepth;
    this.lfo.min = Math.max(60, cutoff * (1 - 0.75 * depth));
    this.lfo.max = Math.min(18000, cutoff * (1 + 0.75 * depth));
  }

  setDelayTime(value: number): void {
    this.effectParams.delayTime = value;
    if (this.delay) this.delay.delayTime.rampTo(value, 0.05);
  }

  setDelayFeedback(value: number): void {
    this.effectParams.delayFeedback = value;
    if (this.delay) this.delay.feedback.rampTo(value, 0.03);
  }

  setDelayWet(value: number): void {
    this.effectParams.delayWet = value;
    if (this.delay) {
      this.delay.wet.rampTo(value, 0.03);
      this.rewire();
    }
  }

  setDistortionAmount(value: number): void {
    this.effectParams.distortionAmount = value;
    if (this.distortion) {
      this.distortion.distortion = value;
      this.rewire();
    }
  }

  setDistortionWet(value: number): void {
    this.effectParams.distortionWet = value;
    if (this.distortion) {
      this.distortion.wet.rampTo(value, 0.03);
      this.rewire();
    }
  }

  setEqLow(value: number): void {
    this.effectParams.eqLow = value;
    if (this.eq) {
      this.eq.low.rampTo(value, 0.03);
      this.rewire();
    }
  }

  setEqMid(value: number): void {
    this.effectParams.eqMid = value;
    if (this.eq) {
      this.eq.mid.rampTo(value, 0.03);
      this.rewire();
    }
  }

  setEqHigh(value: number): void {
    this.effectParams.eqHigh = value;
    if (this.eq) {
      this.eq.high.rampTo(value, 0.03);
      this.rewire();
    }
  }

  setCompThreshold(value: number): void {
    this.effectParams.compThreshold = value;
    if (this.compressor) this.compressor.threshold.value = value;
  }

  setCompRatio(value: number): void {
    this.effectParams.compRatio = value;
    if (this.compressor) {
      this.compressor.ratio.value = value;
      this.rewire();
    }
  }

  setCompAttack(value: number): void {
    this.effectParams.compAttack = value;
    if (this.compressor) this.compressor.attack.value = value;
  }

  setCompRelease(value: number): void {
    this.effectParams.compRelease = value;
    if (this.compressor) this.compressor.release.value = value;
  }

  getVolume(): number {
    const v = this.volume?.volume.value;
    return typeof v === 'number' && Number.isFinite(v) ? v : DEFAULT_VOLUME;
  }

  getEffectParams(): EffectParams {
    return { ...this.effectParams };
  }

  isStarted(): boolean {
    return this.started;
  }

  /** Rebuild the FX chain so only stages that currently do something are in the signal path. */
  private rewire(force = false): void {
    if (!this.fxInput || !this.volume) return;
    const p = this.effectParams;
    const stages: [string, FxNode | null, boolean][] = [
      ['dist', this.distortion, p.distortionWet > 0 && p.distortionAmount > 0],
      ['filter', this.filter, true],
      ['eq', this.eq, p.eqLow !== 0 || p.eqMid !== 0 || p.eqHigh !== 0],
      ['comp', this.compressor, p.compRatio > 1.01],
      ['chorus', this.chorus, p.chorusDepth > 0],
      ['delay', this.delay, p.delayWet > 0],
      ['reverb', this.reverb, p.reverbWet > 0],
    ];
    const key = stages.filter(([, , on]) => on).map(([k]) => k).join('>');
    if (!force && key === this.activeStages) return;
    this.activeStages = key;

    this.fxInput.disconnect();
    for (const [, node] of stages) node?.disconnect();
    let prev: Tone.ToneAudioNode = this.fxInput;
    for (const [, node, on] of stages) {
      if (!on || !node) continue;
      if (node instanceof RoomReverb) {
        prev.connect(node.input);
        prev = node.output;
      } else {
        prev.connect(node);
        prev = node;
      }
    }
    prev.connect(this.volume);
  }

  dispose(): void {
    this.instrument?.releaseAll();
    this.instrument?.dispose();
    this.disconnectLfo();
    for (const n of [
      this.fxInput,
      this.distortion,
      this.filter,
      this.eq,
      this.compressor,
      this.chorus,
      this.delay,
      this.reverb,
      this.volume,
    ]) {
      try {
        n?.dispose();
      } catch {
        /* already disposed */
      }
    }
    this.instrument = null;
    this.fxInput = null;
    this.filter = null;
    this.eq = null;
    this.compressor = null;
    this.chorus = null;
    this.delay = null;
    this.distortion = null;
    this.reverb = null;
    this.volume = null;
    this.activeStages = '';
    this.started = false;
  }
}

/** Chorus amount (0..1) -> modulation depth: kept gentle so it shimmers instead of wobbling. */
function chorusDepthFor(amount: number): number {
  return Math.max(0, Math.min(1, amount)) * 0.6;
}

function chorusWetFor(amount: number): number {
  return amount > 0 ? 0.15 + 0.35 * Math.min(1, amount) : 0;
}

export const audioEngine = new AudioEngine();
