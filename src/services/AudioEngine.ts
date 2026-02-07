import * as Tone from 'tone';
import type { SynthPreset } from '@/types/audio';
import type { EffectParams } from '@/types/effects';
import { SYNTH_PRESETS, DEFAULT_VOLUME, MAX_POLYPHONY } from '@/utils/constants';
import { midiToNoteName } from '@/utils/noteHelpers';

class AudioEngine {
  private synth: Tone.PolySynth | null = null;
  private volume: Tone.Volume | null = null;
  private filter: Tone.Filter | null = null;
  private chorus: Tone.Chorus | null = null;
  private reverb: Tone.Reverb | null = null;
  private started = false;
  private currentPresetIndex = 0;

  private effectParams: EffectParams = {
    reverbWet: 0,
    chorusDepth: 0,
    filterCutoff: 18000,
    filterResonance: 1,
  };

  async start(): Promise<void> {
    if (this.started) return;
    await Tone.start();

    this.volume = new Tone.Volume(DEFAULT_VOLUME).toDestination();
    this.reverb = new Tone.Reverb({ decay: 2.5, wet: 0 });
    this.chorus = new Tone.Chorus({ frequency: 1.5, delayTime: 3.5, depth: 0, wet: 0 }).start();
    this.filter = new Tone.Filter({ frequency: 18000, type: 'lowpass', Q: 1 });

    // Chain: Filter → Chorus → Reverb → Volume → Destination
    this.filter.connect(this.chorus);
    this.chorus.connect(this.reverb);
    this.reverb.connect(this.volume);

    this.createSynth(SYNTH_PRESETS[0]);
    this.started = true;
  }

  private createSynth(preset: SynthPreset): void {
    if (this.synth) {
      this.synth.releaseAll();
      this.synth.dispose();
    }
    this.synth = new Tone.PolySynth(Tone.Synth, {
      maxPolyphony: MAX_POLYPHONY,
      oscillator: preset.oscillator as Tone.OmniOscillatorOptions,
      envelope: preset.envelope,
    });
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
    }
  }

  setFilterResonance(q: number): void {
    if (this.filter) {
      this.filter.Q.value = q;
      this.effectParams.filterResonance = q;
    }
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
    this.filter?.dispose();
    this.chorus?.dispose();
    this.reverb?.dispose();
    this.volume?.dispose();
    this.synth = null;
    this.filter = null;
    this.chorus = null;
    this.reverb = null;
    this.volume = null;
    this.started = false;
  }
}

export const audioEngine = new AudioEngine();
