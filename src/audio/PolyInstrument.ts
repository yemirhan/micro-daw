import * as Tone from 'tone';
import type { FMPreset, LfoParams, PluckPreset, SubtractivePreset } from '@/types/audio';
import type { SynthPlayer } from './types';
import type { ModBus, Voice } from './voices/types';
import { CharacterChain } from './CharacterChain';
import { MonoNoteStack, PolyVoiceAllocator } from './voiceAllocator';
import { mergePresetParams } from './presetParams';
import { HARD_STOP_FADE, MIN_GATE, clamp, durationToSeconds, normVelocity, noteToMidi, safeDispose, withContext } from './voices/common';

export type VoicePreset = SubtractivePreset | FMPreset | PluckPreset;
export type VoiceFactory<P extends VoicePreset> = (preset: P, mod: ModBus) => Voice<P>;

/** Gap between a stolen voice's fade-out and the new note (avoids a click). */
const STEAL_GAP = HARD_STOP_FADE * 0.6;
const NO_LFO: LfoParams = { rate: 5, vibrato: 0, filter: 0, delay: 0 };

function lfoOf(p: VoicePreset): LfoParams {
  return p.engine === 'pluck' ? NO_LFO : p.lfo;
}

/**
 * A polyphonic (or monophonic) instrument made of `Voice`s.
 *
 * - Poly: PolyVoiceAllocator decides which voice plays which note; voices
 *   are created lazily up to the preset's polyphony.
 * - Mono (subtractive `mono: true`): MonoNoteStack gives last-note priority,
 *   legato glides and correct fall-back to still-held keys.
 * - One LFO per instrument feeds every voice (vibrato / filter wobble).
 */
export class PolyInstrument<P extends VoicePreset> implements SynthPlayer {
  readonly output: Tone.ToneAudioNode;

  private preset: P;
  private readonly voices: Voice<P>[] = [];
  private readonly allocator: PolyVoiceAllocator;
  private readonly monoStack: MonoNoteStack | null;
  private monoEnd = -Infinity;
  private monoScheduledEnd = -Infinity;
  private monoScheduledNote: number | null = null;
  private readonly bus: Tone.Gain;
  private readonly chain: CharacterChain;
  private readonly lfo: Tone.LFO;
  private readonly vibratoDepth: Tone.Gain;
  private readonly filterDepth: Tone.Gain;
  private readonly mod: ModBus;
  /** Context the instrument was built in (voices are created lazily in it). */
  private readonly context: Tone.BaseContext;
  private disposed = false;

  constructor(
    preset: P,
    private readonly factory: VoiceFactory<P>,
  ) {
    this.preset = preset;
    this.context = Tone.getContext();
    const isMono = preset.engine === 'subtractive' && preset.mono;
    this.allocator = new PolyVoiceAllocator(isMono ? 1 : Math.max(1, Math.round(preset.polyphony)));
    this.monoStack = isMono ? new MonoNoteStack((preset as SubtractivePreset).legato) : null;

    this.chain = new CharacterChain(preset.character, preset.gain);
    this.bus = new Tone.Gain(1);
    this.bus.connect(this.chain.input);
    this.output = this.chain.output;

    const lfo = lfoOf(preset);
    this.lfo = new Tone.LFO({ frequency: clamp(lfo.rate, 0.05, 30), min: -1, max: 1 });
    this.vibratoDepth = new Tone.Gain(lfo.vibrato);
    this.filterDepth = new Tone.Gain(lfo.filter);
    this.lfo.connect(this.vibratoDepth);
    this.lfo.connect(this.filterDepth);
    if (lfo.vibrato > 0 || lfo.filter > 0) this.lfo.start();
    this.mod = { vibrato: this.vibratoDepth, filter: this.filterDepth };
  }

  // -------------------------------------------------------------------------
  // Notes
  // -------------------------------------------------------------------------

  noteOn(midi: number, velocity: number, time?: number): void {
    if (this.disposed || midi < 0 || midi > 127) return;
    const t = time ?? Tone.now();
    const vel = normVelocity(velocity);

    if (this.monoStack) {
      const wasHeld = this.monoStack.heldNotes().length > 0;
      const action = this.monoStack.noteOn(midi, vel);
      const voice = this.getVoice(0);
      this.onFirstNote(t, !wasHeld && this.monoEnd <= t);
      if (action.type === 'legato') {
        voice.glide(midi, t);
      } else if (action.type === 'attack') {
        voice.attack(midi, t, vel, wasHeld ? action.from : null);
      }
      this.monoEnd = Infinity;
      return;
    }

    this.onFirstNote(t, this.allocator.activeVoiceCount(t) === 0);
    const alloc = this.allocator.noteOn(midi, t, vel);
    const voice = this.getVoice(alloc.voice);
    let at = t;
    if (alloc.stolen) {
      voice.hardStop(t);
      at = t + STEAL_GAP;
    }
    voice.attack(midi, at, vel);
  }

  noteOff(midi: number, time?: number): void {
    if (this.disposed) return;
    const t = time ?? Tone.now();

    if (this.monoStack) {
      const action = this.monoStack.noteOff(midi);
      const voice = this.voices[0];
      if (!voice) return;
      if (action.type === 'release') this.monoEnd = voice.release(t);
      else if (action.type === 'legato') voice.glide(action.note, t);
      return;
    }

    const released = this.allocator.noteOff(midi, t, this.releaseDuration());
    for (const i of released) this.voices[i]?.release(t);
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

    if (this.monoStack) {
      const voice = this.getVoice(0);
      const overlapping = this.monoScheduledEnd > t + 1e-4 && this.monoScheduledNote !== null;
      voice.attack(midi, t, vel, overlapping ? this.monoScheduledNote : null);
      const end = voice.release(t + dur);
      this.monoScheduledEnd = t + dur;
      this.monoScheduledNote = midi;
      this.monoEnd = Math.max(this.monoEnd === Infinity ? -Infinity : this.monoEnd, end);
      return;
    }

    const alloc = this.allocator.oneShot(midi, t, dur, this.releaseDuration(), vel);
    const voice = this.getVoice(alloc.voice);
    let at = t;
    if (alloc.stolen) {
      voice.hardStop(t);
      at = t + STEAL_GAP;
    }
    voice.attack(midi, at, vel);
    voice.release(at + dur);
  }

  releaseAll(time?: number): void {
    if (this.disposed) return;
    const t = time ?? Tone.now();
    if (this.monoStack) {
      this.monoStack.releaseAll();
      this.monoScheduledEnd = -Infinity;
      this.monoScheduledNote = null;
    }
    this.allocator.releaseAll(t);
    // Belt and braces: fade every voice in the pool, whatever the bookkeeping says.
    let end = t;
    for (const v of this.voices) end = Math.max(end, v.hardStop(t));
    this.monoEnd = this.monoStack ? end : -Infinity;
  }

  // -------------------------------------------------------------------------
  // Params / routing
  // -------------------------------------------------------------------------

  connect(node: Tone.InputNode): SynthPlayer {
    this.output.connect(node);
    return this;
  }

  set(params: Record<string, unknown>): void {
    if (this.disposed) return;
    const next = mergePresetParams(this.preset, params);
    this.preset = next;
    for (const v of this.voices) v.update(next);
    withContext(this.context, () => this.chain.update(next.character, next.gain));
    if (!this.monoStack) this.allocator.maxVoices = Math.max(1, Math.round(next.polyphony));
    else this.monoStack.legato = (next as SubtractivePreset).legato;
    const lfo = lfoOf(next);
    this.lfo.frequency.rampTo(clamp(lfo.rate, 0.05, 30), 0.05);
    this.vibratoDepth.gain.rampTo(lfo.vibrato, 0.05);
    this.filterDepth.gain.rampTo(lfo.filter, 0.05);
    if ((lfo.vibrato > 0 || lfo.filter > 0) && this.lfo.state !== 'started') this.lfo.start();
  }

  getPreset(): P {
    return this.preset;
  }

  getActiveVoiceCount(time?: number): number {
    const t = time ?? Tone.immediate();
    if (this.monoStack) return this.monoStack.heldNotes().length > 0 || this.monoEnd > t ? 1 : 0;
    return this.allocator.activeVoiceCount(t);
  }

  getHeldNotes(): number[] {
    if (this.monoStack) return [...this.monoStack.heldNotes()].sort((a, b) => a - b);
    return this.allocator.heldNotes();
  }

  /** Number of voice objects created so far (diagnostics). */
  getPoolSize(): number {
    return this.voices.length;
  }

  dispose(): void {
    if (this.disposed) return;
    this.releaseAll();
    this.disposed = true;
    for (const v of this.voices) v.dispose();
    this.voices.length = 0;
    safeDispose(this.lfo, this.vibratoDepth, this.filterDepth, this.bus, this.chain);
  }

  // -------------------------------------------------------------------------

  private getVoice(index: number): Voice<P> {
    let v = this.voices[index];
    if (!v) {
      const voice = withContext(this.context, () => this.factory(this.preset, this.mod));
      voice.output.connect(this.bus);
      this.voices[index] = voice;
      v = voice;
    }
    return v;
  }

  private releaseDuration(): number {
    const v = this.voices[0];
    if (v) return v.releaseDuration();
    const p = this.preset;
    return p.engine === 'pluck' ? p.release : p.amp.release;
  }

  /** Delayed vibrato: fade the vibrato in after `lfo.delay` when playing from silence. */
  private onFirstNote(t: number, fromSilence: boolean): void {
    const lfo = lfoOf(this.preset);
    if (!fromSilence || lfo.vibrato <= 0 || lfo.delay <= 0) return;
    const g = this.vibratoDepth.gain;
    g.cancelScheduledValues(t);
    g.setValueAtTime(0, t);
    g.linearRampToValueAtTime(lfo.vibrato, t + lfo.delay);
  }
}
