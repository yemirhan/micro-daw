import * as Tone from 'tone';
import type { SynthPreset } from '@/types/audio';
import type { EffectParams, FilterType, LfoWaveform } from '@/types/effects';
import { DEFAULT_EFFECT_PARAMS } from '@/types/effects';
import { SYNTH_PRESETS, DEFAULT_VOLUME } from '@/utils/constants';
import { createSynthFromPreset, type SynthPlayer } from '@/utils/synthFactory';
import { midiToNoteName } from '@/utils/noteHelpers';

class AudioEngine {
  private synth: SynthPlayer | null = null;
  private volume: Tone.Volume | null = null;
  private filter: Tone.Filter | null = null;
  private chorus: Tone.Chorus | null = null;
  private reverb: Tone.Reverb | null = null;
  private delay: Tone.FeedbackDelay | null = null;
  private distortion: Tone.Distortion | null = null;
  private eq: Tone.EQ3 | null = null;
  private compressor: Tone.Compressor | null = null;
  private lfo: Tone.LFO | null = null;
  private started = false;
  private currentPresetIndex = 0;

  private effectParams: EffectParams = { ...DEFAULT_EFFECT_PARAMS };

  async start(): Promise<void> {
    if (this.started) return;
    await Tone.start();

    this.volume = new Tone.Volume(DEFAULT_VOLUME).toDestination();
    this.reverb = new Tone.Reverb({ decay: 2.5, wet: 0 });
    this.distortion = new Tone.Distortion({ distortion: 0, wet: 0 });
    this.delay = new Tone.FeedbackDelay({ delayTime: 0.25, feedback: 0.3, wet: 0 });
    this.chorus = new Tone.Chorus({ frequency: 1.5, delayTime: 3.5, depth: 0, wet: 0 }).start();
    this.compressor = new Tone.Compressor({ threshold: -24, ratio: 4, attack: 0.003, release: 0.25 });
    this.eq = new Tone.EQ3({ low: 0, mid: 0, high: 0 });
    this.filter = new Tone.Filter({ frequency: 18000, type: 'lowpass', Q: 1 });

    // Chain: Filter → EQ → Compressor → Chorus → Delay → Distortion → Reverb → Volume → Destination
    this.filter.connect(this.eq);
    this.eq.connect(this.compressor);
    this.compressor.connect(this.chorus);
    this.chorus.connect(this.delay);
    this.delay.connect(this.distortion);
    this.distortion.connect(this.reverb);
    this.reverb.connect(this.volume);

    this.createSynth(SYNTH_PRESETS[0]);
    this.started = true;
  }

  private createSynth(preset: SynthPreset): void {
    if (this.synth) {
      this.synth.releaseAll();
      this.synth.dispose();
    }
    this.synth = createSynthFromPreset(preset);
    this.synth.connect(this.filter!);
  }

  noteOn(midiNote: number, velocity: number): void {
    if (!this.synth || !this.started) return;
    const noteName = midiToNoteName(midiNote);
    this.synth.triggerAttack(noteName, Tone.now(), velocity);
  }

  noteOff(midiNote: number): void {
    if (!this.synth || !this.started) return;
    const noteName = midiToNoteName(midiNote);
    this.synth.triggerRelease(noteName, Tone.now());
  }

  releaseAll(): void {
    this.synth?.releaseAll();
  }

  mute(): void {
    if (this.volume) this.volume.mute = true;
  }

  unmute(): void {
    if (this.volume) this.volume.mute = false;
  }

  setVolume(db: number): void {
    if (this.volume) {
      this.volume.volume.value = db;
    }
  }

  setPreset(index: number): void {
    if (index < 0 || index >= SYNTH_PRESETS.length) return;
    this.currentPresetIndex = index;
    if (this.started) {
      this.createSynth(SYNTH_PRESETS[index]);
    }
  }

  /** Apply a live parameter change to the current synth without recreating it */
  setSynthParam(key: string, value: unknown): void {
    if (!this.synth) return;
    this.synth.set({ [key]: value });
  }

  /** Get the current resolved preset */
  getCurrentPreset(): SynthPreset {
    return SYNTH_PRESETS[this.currentPresetIndex] ?? SYNTH_PRESETS[0];
  }

  setReverbWet(value: number): void {
    if (this.reverb) {
      this.reverb.wet.value = value;
      this.effectParams.reverbWet = value;
    }
  }

  setChorusDepth(value: number): void {
    if (this.chorus) {
      this.chorus.depth = value;
      this.chorus.wet.value = value > 0 ? 0.5 : 0;
      this.effectParams.chorusDepth = value;
    }
  }

  setFilterCutoff(hz: number): void {
    if (this.filter) {
      this.filter.frequency.value = hz;
      this.effectParams.filterCutoff = hz;
      this.updateLfoRange();
    }
  }

  setFilterResonance(q: number): void {
    if (this.filter) {
      this.filter.Q.value = q;
      this.effectParams.filterResonance = q;
    }
  }

  setFilterType(type: FilterType): void {
    if (this.filter) {
      this.filter.type = type;
      this.effectParams.filterType = type;
    }
  }

  setFilterLfoRate(rate: number): void {
    this.effectParams.filterLfoRate = rate;
    if (this.lfo) {
      this.lfo.frequency.value = rate;
    }
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
    if (this.lfo) {
      this.lfo.type = wave;
    }
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
      // Restore static cutoff
      if (this.filter) {
        this.filter.frequency.value = this.effectParams.filterCutoff;
      }
    }
  }

  private updateLfoRange(): void {
    if (!this.lfo || this.effectParams.filterLfoDepth === 0) return;
    const cutoff = this.effectParams.filterCutoff;
    const depth = this.effectParams.filterLfoDepth;
    const minFreq = Math.max(60, cutoff * (1 - 0.75 * depth));
    const maxFreq = Math.min(18000, cutoff * (1 + 0.75 * depth));
    this.lfo.min = minFreq;
    this.lfo.max = maxFreq;
  }

  setDelayTime(value: number): void {
    if (this.delay) {
      this.delay.delayTime.value = value;
      this.effectParams.delayTime = value;
    }
  }

  setDelayFeedback(value: number): void {
    if (this.delay) {
      this.delay.feedback.value = value;
      this.effectParams.delayFeedback = value;
    }
  }

  setDelayWet(value: number): void {
    if (this.delay) {
      this.delay.wet.value = value;
      this.effectParams.delayWet = value;
    }
  }

  setDistortionAmount(value: number): void {
    if (this.distortion) {
      this.distortion.distortion = value;
      this.effectParams.distortionAmount = value;
    }
  }

  setDistortionWet(value: number): void {
    if (this.distortion) {
      this.distortion.wet.value = value;
      this.effectParams.distortionWet = value;
    }
  }

  setEqLow(value: number): void {
    if (this.eq) {
      this.eq.low.value = value;
      this.effectParams.eqLow = value;
    }
  }

  setEqMid(value: number): void {
    if (this.eq) {
      this.eq.mid.value = value;
      this.effectParams.eqMid = value;
    }
  }

  setEqHigh(value: number): void {
    if (this.eq) {
      this.eq.high.value = value;
      this.effectParams.eqHigh = value;
    }
  }

  setCompThreshold(value: number): void {
    if (this.compressor) {
      this.compressor.threshold.value = value;
      this.effectParams.compThreshold = value;
    }
  }

  setCompRatio(value: number): void {
    if (this.compressor) {
      this.compressor.ratio.value = value;
      this.effectParams.compRatio = value;
    }
  }

  setCompAttack(value: number): void {
    if (this.compressor) {
      this.compressor.attack.value = value;
      this.effectParams.compAttack = value;
    }
  }

  setCompRelease(value: number): void {
    if (this.compressor) {
      this.compressor.release.value = value;
      this.effectParams.compRelease = value;
    }
  }

  getVolume(): number {
    return this.volume?.volume.value ?? DEFAULT_VOLUME;
  }

  getEffectParams(): EffectParams {
    return { ...this.effectParams };
  }

  getPresetIndex(): number {
    return this.currentPresetIndex;
  }

  isStarted(): boolean {
    return this.started;
  }

  dispose(): void {
    this.synth?.releaseAll();
    this.synth?.dispose();
    this.disconnectLfo();
    this.filter?.dispose();
    this.eq?.dispose();
    this.compressor?.dispose();
    this.chorus?.dispose();
    this.delay?.dispose();
    this.distortion?.dispose();
    this.reverb?.dispose();
    this.volume?.dispose();
    this.synth = null;
    this.filter = null;
    this.eq = null;
    this.compressor = null;
    this.chorus = null;
    this.delay = null;
    this.distortion = null;
    this.reverb = null;
    this.volume = null;
    this.started = false;
  }
}

export const audioEngine = new AudioEngine();
