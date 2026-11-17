import * as Tone from 'tone';
import type { FMPreset, SamplerPreset } from '@/types/audio';
import type { SynthPlayer } from './types';
import { CharacterChain } from './CharacterChain';
import { PolyInstrument } from './PolyInstrument';
import { FMVoice } from './voices/FMVoice';
import { PolyVoiceAllocator } from './voiceAllocator';
import { mergePresetParams } from './presetParams';
import { getPianoBuffers, loadPianoSamples, type PianoBuffers } from './pianoSamples';
import { MIN_GATE, clamp, durationToSeconds, midiToName, normVelocity, noteToMidi, safeDispose, withContext } from './voices/common';

/** The Salamander samples are mastered quietly; +6 dB brings them in line with the synths. */
const SAMPLER_MAKEUP = 2;

/**
 * Multi-sampled acoustic piano (Tone.Sampler over the bundled Salamander set).
 *
 * The allocator does the note bookkeeping (Sampler itself has no voice
 * limit and stacks sources on re-strikes): a re-struck key releases its
 * previous source, polyphony is capped, and noteOff/releaseAll always find
 * what is sounding. Until the samples are decoded (a few hundred ms after
 * start) notes go to a synthesized FM piano so the first key press is never
 * silent; their noteOffs are routed back to it.
 */
export class SampledPiano implements SynthPlayer {
  readonly output: Tone.ToneAudioNode;

  private preset: SamplerPreset;
  private sampler: Tone.Sampler | null = null;
  private readonly fallback: PolyInstrument<FMPreset>;
  private readonly onFallback = new Set<number>();
  private readonly allocator: PolyVoiceAllocator;
  private readonly bus: Tone.Gain;
  private readonly chain: CharacterChain;
  private readonly context: Tone.BaseContext;
  private disposed = false;

  /** `buffers` overrides the shared sample cache (tests / custom sample sets). */
  constructor(preset: SamplerPreset, fallbackPreset: FMPreset, buffers?: PianoBuffers | null) {
    this.preset = preset;
    this.context = Tone.getContext();
    this.allocator = new PolyVoiceAllocator(Math.max(8, Math.round(preset.polyphony)));
    this.chain = new CharacterChain(preset.character, preset.gain);
    this.bus = new Tone.Gain(SAMPLER_MAKEUP);
    this.bus.connect(this.chain.input);
    this.output = this.chain.output;

    this.fallback = new PolyInstrument(fallbackPreset, (p, mod) => new FMVoice(p, mod));
    this.fallback.connect(this.bus);

    const ready = buffers ?? getPianoBuffers();
    if (ready) {
      this.createSampler(ready);
    } else {
      loadPianoSamples()
        .then((b) => {
          if (!this.disposed) this.createSampler(b);
        })
        .catch(() => {
          /* keep the synthesized fallback */
        });
    }
  }

  /** True once real samples are in use. */
  isSampled(): boolean {
    return this.sampler !== null;
  }

  noteOn(midi: number, velocity: number, time?: number): void {
    if (this.disposed || midi < 0 || midi > 127) return;
    const t = time ?? Tone.now();
    const vel = normVelocity(velocity);
    if (!this.sampler) {
      this.onFallback.add(midi);
      this.fallback.noteOn(midi, vel, t);
      return;
    }
    this.strike(midi, vel, t);
  }

  noteOff(midi: number, time?: number): void {
    if (this.disposed) return;
    const t = time ?? Tone.now();
    if (this.onFallback.delete(midi)) this.fallback.noteOff(midi, t);
    const released = this.allocator.noteOff(midi, t, this.releaseSeconds());
    if (released.length && this.sampler) this.sampler.triggerRelease(midiToName(midi), t);
  }

  triggerAttack(note: string | number, time?: number, velocity = 0.8): void {
    this.noteOn(noteToMidi(note), velocity, time);
  }

  triggerRelease(note: string | number, time?: number): void {
    this.noteOff(noteToMidi(note), time);
  }

  triggerAttackRelease(note: string | number, duration: number | string, time?: number, velocity = 0.8): void {
    if (this.disposed) return;
    const midi = noteToMidi(note);
    if (midi < 0 || midi > 127) return;
    const t = time ?? Tone.now();
    const dur = Math.max(MIN_GATE * 2, durationToSeconds(duration));
    const vel = normVelocity(velocity);
    if (!this.sampler) {
      this.fallback.triggerAttackRelease(midi, dur, t, vel);
      return;
    }
    const alloc = this.allocator.oneShot(midi, t, dur, this.releaseSeconds(), vel);
    this.releaseForAllocation(alloc.retrigger ? midi : alloc.stolen ? alloc.previousNote : null, t);
    const name = midiToName(midi);
    this.sampler.triggerAttack(name, t, this.velocityToGain(vel));
    this.sampler.triggerRelease(name, t + dur);
  }

  releaseAll(time?: number): void {
    if (this.disposed) return;
    const t = time ?? Tone.now();
    this.fallback.releaseAll(t);
    this.onFallback.clear();
    this.allocator.releaseAll(t);
    this.sampler?.releaseAll(t);
  }

  connect(node: Tone.InputNode): SynthPlayer {
    this.output.connect(node);
    return this;
  }

  set(params: Record<string, unknown>): void {
    if (this.disposed) return;
    this.preset = mergePresetParams(this.preset, params);
    if (this.sampler) this.sampler.release = this.releaseSeconds();
    this.allocator.maxVoices = Math.max(8, Math.round(this.preset.polyphony));
    withContext(this.context, () => this.chain.update(this.preset.character, this.preset.gain));
  }

  getPreset(): SamplerPreset {
    return this.preset;
  }

  getActiveVoiceCount(time?: number): number {
    const t = time ?? Tone.immediate();
    return this.allocator.activeVoiceCount(t) + this.fallback.getActiveVoiceCount(t);
  }

  getHeldNotes(): number[] {
    return [...new Set([...this.allocator.heldNotes(), ...this.fallback.getHeldNotes()])].sort((a, b) => a - b);
  }

  dispose(): void {
    if (this.disposed) return;
    this.releaseAll();
    this.disposed = true;
    this.fallback.dispose();
    safeDispose(this.sampler, this.bus, this.chain);
    this.sampler = null;
  }

  // -------------------------------------------------------------------------

  private createSampler(buffers: PianoBuffers): void {
    const urls: Record<string, Tone.ToneAudioBuffer> = {};
    for (const [midi, buf] of buffers) urls[midiToName(midi)] = buf;
    const sampler = withContext(
      this.context,
      () => new Tone.Sampler({ urls, attack: 0, release: this.releaseSeconds(), curve: 'exponential' }),
    );
    sampler.connect(this.bus);
    this.sampler = sampler;
  }

  private strike(midi: number, vel: number, t: number): void {
    const alloc = this.allocator.noteOn(midi, t, vel);
    this.releaseForAllocation(alloc.retrigger ? midi : alloc.stolen ? alloc.previousNote : null, t);
    this.sampler?.triggerAttack(midiToName(midi), t, this.velocityToGain(vel));
  }

  /** Re-strike: release the old source of the same key; steal: release the stolen key. */
  private releaseForAllocation(note: number | null, t: number): void {
    if (note === null || !this.sampler) return;
    this.sampler.triggerRelease(midiToName(note), t);
  }

  private velocityToGain(vel: number): number {
    const curve = clamp(this.preset.velocityCurve, 0.5, 3);
    return 0.06 + 0.94 * Math.pow(clamp(vel, 0, 1), curve);
  }

  private releaseSeconds(): number {
    return clamp(this.preset.release, 0.05, 5);
  }
}
