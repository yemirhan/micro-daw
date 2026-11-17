import type * as Tone from 'tone';

/**
 * Uniform interface for every instrument (live engine + per-track synths +
 * offline export). Note arguments may be note names ('C#4') or MIDI numbers.
 * Velocity is 0..1 (values > 1 are treated as 0..127 and normalised).
 */
export interface SynthPlayer {
  readonly output: Tone.ToneAudioNode;
  /** Low-level note on (MIDI). `time` defaults to Tone.now(). */
  noteOn(midi: number, velocity: number, time?: number): void;
  noteOff(midi: number, time?: number): void;
  triggerAttack(note: string | number, time?: number, velocity?: number): void;
  triggerRelease(note: string | number, time?: number): void;
  /** Self-releasing note (sequencer playback, previews, export). */
  triggerAttackRelease(note: string | number, duration: number | string, time?: number, velocity?: number): void;
  /** Silence every voice (fast fade) and forget all held notes. */
  releaseAll(time?: number): void;
  connect(node: Tone.InputNode): SynthPlayer;
  /** Live parameter change: a partial preset (nested objects merge). Never re-creates the instrument. */
  set(params: Record<string, unknown>): void;
  /** Voices still sounding at `time` (held + release tails). */
  getActiveVoiceCount(time?: number): number;
  /** MIDI notes whose key is held (no noteOff yet). */
  getHeldNotes(): number[];
  dispose(): void;
}
