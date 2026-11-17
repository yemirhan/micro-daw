import * as Tone from 'tone';
import type { KitState, PadIndex, PadSampleSource } from '@/types/sampler';
import { ensureMasterBus } from '@/audio/masterBus';
import { normVelocity } from '@/audio/voices/common';
import { SamplerKit } from '@/audio/sampler/SamplerKit';
import { createFactoryKit, factoryPadForGmNote } from '@/audio/sampler/factoryKit';
import { loadPreviewBuffer } from '@/audio/sampler/bufferCache';

/**
 * Where live pad hits go (the active drum track registers one). Velocity is
 * 0..1. `time` is an AudioContext time to play at (sample-accurate note
 * repeat); omitted = now (Tone.immediate()).
 */
export interface DrumSink {
  hit(pad: PadIndex, velocity: number, time?: number): void;
  release(pad: PadIndex, time?: number): void;
}

const PREVIEW_GAIN = 0.9;
const PREVIEW_FADE = 0.01;

/**
 * Live pad facade (on-screen pads, computer keys, MIDI pads).
 *
 * Hits go to the registered sink (the active drum track's SamplerKit, so its
 * FX apply and loop recording sees them). Without a sink they play on an own
 * fallback SamplerKit (factory kit by default). Also hosts the sample-browser
 * preview player.
 */
class DrumEngine {
  private volume: Tone.Volume | null = null;
  private kit: SamplerKit | null = null;
  private fallbackKit: KitState = createFactoryKit();
  private sink: DrumSink | null = null;
  private started = false;
  private muted = false;
  private previewSource: Tone.ToneBufferSource | null = null;
  private previewToken = 0;

  async start(): Promise<void> {
    if (this.started) return;
    ensureMasterBus();
    this.volume = new Tone.Volume(-6).toDestination();
    this.kit = new SamplerKit(this.fallbackKit).connect(this.volume);
    this.started = true;
    // Factory buffers render in the background; hits before that are silent.
    void this.kit.ready;
  }

  /**
   * Hit a pad. velocity 0..1 (0..127 is normalised). `time` = AudioContext
   * time to play at (e.g. a note-repeat step scheduled ahead); omitted = now.
   */
  hit(pad: PadIndex, velocity: number, time?: number): void {
    if (this.muted) return;
    const v = normVelocity(velocity);
    // A step handed out late (main thread stalled) plays now rather than with its start cut off.
    const at = time !== undefined && Number.isFinite(time) ? Math.max(time, Tone.immediate()) : undefined;
    try {
      if (this.sink) {
        if (at === undefined) this.sink.hit(pad, v);
        else this.sink.hit(pad, v, at);
      } else this.kit?.trigger(pad, v, at ?? Tone.immediate());
    } catch (err) {
      console.warn('[drums] hit failed', err);
    }
  }

  /** Pad released (matters for pads in 'gate' mode). */
  release(pad: PadIndex): void {
    try {
      if (this.sink) this.sink.release(pad);
      else this.kit?.release(pad, Tone.immediate());
    } catch (err) {
      console.warn('[drums] release failed', err);
    }
  }

  /**
   * Legacy: hit by General MIDI drum note (mapped to the closest factory pad).
   * Returns false for notes with no factory equivalent.
   */
  hitByMidiNote(midiNote: number, velocity: number): boolean {
    const pad = factoryPadForGmNote(midiNote);
    if (pad === null) return false;
    this.hit(pad, velocity);
    return true;
  }

  /** Route live hits to the active drum track (null = use the fallback kit). */
  setSink(sink: DrumSink | null): void {
    if (this.sink === sink) return;
    this.kit?.stopAll();
    this.sink = sink;
  }

  getSink(): DrumSink | null {
    return this.sink;
  }

  /** Kit played when no sink is registered. */
  setFallbackKit(kit: KitState): void {
    this.fallbackKit = kit;
    this.kit?.setKit(kit);
  }

  getFallbackKit(): KitState {
    return this.fallbackKit;
  }

  /** Audition a sample (sample browser). Stops the previous preview. */
  async preview(source: PadSampleSource): Promise<void> {
    this.stopPreview();
    const token = this.previewToken;
    await this.start();
    let buffer: AudioBuffer;
    try {
      buffer = await loadPreviewBuffer(source);
    } catch (err) {
      console.warn('[drums] preview failed', err);
      return;
    }
    if (token !== this.previewToken || !this.volume || this.muted) return;
    try {
      const src = new Tone.ToneBufferSource({
        url: buffer,
        fadeOut: PREVIEW_FADE,
        curve: 'linear',
        onended: () => {
          if (this.previewSource === src) this.previewSource = null;
        },
      }).connect(this.volume);
      src.start(Tone.immediate(), 0, undefined, PREVIEW_GAIN);
      this.previewSource = src;
    } catch (err) {
      console.warn('[drums] preview failed', err);
    }
  }

  stopPreview(): void {
    this.previewToken++;
    const src = this.previewSource;
    this.previewSource = null;
    if (!src) return;
    try {
      src.stop(Tone.immediate());
    } catch {
      /* already ended */
    }
  }

  mute(): void {
    this.muted = true;
    this.stopPreview();
    if (this.volume) this.volume.mute = true;
  }

  unmute(): void {
    this.muted = false;
    if (this.volume) this.volume.mute = false;
  }

  setVolume(db: number): void {
    if (this.volume) {
      this.volume.volume.value = db;
    }
  }

  isStarted(): boolean {
    return this.started;
  }

  dispose(): void {
    this.stopPreview();
    this.kit?.dispose();
    this.volume?.dispose();
    this.kit = null;
    this.volume = null;
    this.sink = null;
    this.started = false;
  }
}

export const drumEngine = new DrumEngine();
