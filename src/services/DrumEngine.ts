import * as Tone from 'tone';
import type { KitState, PadIndex, PadSampleSource } from '@/types/sampler';
import { ensureMasterBus } from '@/audio/masterBus';
import { normVelocity } from '@/audio/voices/common';
import { SamplerKit, collectVoices, type VoiceHandle } from '@/audio/sampler/SamplerKit';
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
  /**
   * Optional: release every live (gate) voice this sink is holding. Called
   * when drumEngine stops routing to it (active drum track changed), as a
   * safety net next to the per-pad bookkeeping below.
   */
  releaseAllLive?(): void;
}

/** Where a live hit went: a registered sink, or the fallback kit. */
type HitTarget = DrumSink | 'fallback';

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
  /** Mute gate (gain 1 / 0) after the volume: a volume change never un-mutes. */
  private muteGate: Tone.Gain | null = null;
  private kit: SamplerKit | null = null;
  private fallbackKit: KitState = createFactoryKit();
  private sink: DrumSink | null = null;
  private started = false;
  private muted = false;
  private previewSource: Tone.ToneBufferSource | null = null;
  private previewToken = 0;
  /**
   * Per pad: every target that got a hit since the pad was last released.
   * A release goes to these (not to whatever sink is current), so switching
   * the active drum track while a gate pad is held can't strand its voice.
   */
  private padTargets = new Map<PadIndex, Set<HitTarget>>();
  /** Per pad: voices handed out ahead of time (note repeat) that may still be pending. */
  private scheduled = new Map<PadIndex, VoiceHandle[]>();

  async start(): Promise<void> {
    if (this.started) return;
    ensureMasterBus();
    this.muteGate = new Tone.Gain(this.muted ? 0 : 1).toDestination();
    this.volume = new Tone.Volume(-6).connect(this.muteGate);
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
    const sink = this.sink;
    const target: HitTarget = sink ?? 'fallback';
    try {
      const voices = collectVoices(() => {
        if (sink) {
          if (at === undefined) sink.hit(pad, v);
          else sink.hit(pad, v, at);
        } else this.kit?.trigger(pad, v, at ?? Tone.immediate());
      });
      this.remember(pad, target, at === undefined ? [] : voices);
    } catch (err) {
      console.warn('[drums] hit failed', err);
    }
  }

  /**
   * Pad released: hits of this pad handed out ahead of time (note repeat) that
   * have not started yet are cancelled, whatever the pad's mode, and 'gate'
   * voices fade out. Goes to every target that played the pad since its last
   * release (see `padTargets`), not just the current sink.
   */
  release(pad: PadIndex): void {
    const pending = this.scheduled.get(pad);
    this.scheduled.delete(pad);
    if (pending) {
      for (const h of pending) {
        try {
          h.cancelIfPending();
        } catch {
          /* voice already gone */
        }
      }
    }
    const targets = this.padTargets.get(pad);
    this.padTargets.delete(pad);
    const list: HitTarget[] = targets && targets.size > 0 ? [...targets] : [this.sink ?? 'fallback'];
    for (const target of list) {
      try {
        if (target === 'fallback') this.kit?.release(pad, Tone.immediate());
        else target.release(pad);
      } catch (err) {
        console.warn('[drums] release failed', err);
      }
    }
  }

  private remember(pad: PadIndex, target: HitTarget, voices: VoiceHandle[]): void {
    let targets = this.padTargets.get(pad);
    if (!targets) this.padTargets.set(pad, (targets = new Set()));
    targets.add(target);
    if (voices.length === 0) return;
    const now = Tone.immediate();
    // Keep only voices still in the future (bounded: one look-ahead window's worth).
    const list = (this.scheduled.get(pad) ?? []).filter((h) => h.start > now);
    for (const h of voices) if (h.start > now) list.push(h);
    if (list.length > 0) this.scheduled.set(pad, list);
    else this.scheduled.delete(pad);
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
    const prev = this.sink;
    this.kit?.stopAll();
    this.sink = sink;
    try {
      prev?.releaseAllLive?.();
    } catch (err) {
      console.warn('[drums] releaseAllLive failed', err);
    }
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
    if (this.muteGate) this.muteGate.gain.value = 0;
  }

  unmute(): void {
    this.muted = false;
    if (this.muteGate) this.muteGate.gain.value = 1;
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
    this.muteGate?.dispose();
    this.kit = null;
    this.volume = null;
    this.muteGate = null;
    this.sink = null;
    this.padTargets.clear();
    this.scheduled.clear();
    this.started = false;
  }
}

export const drumEngine = new DrumEngine();
