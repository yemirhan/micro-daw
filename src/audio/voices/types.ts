import type * as Tone from 'tone';

/** Per-instrument modulation sources voices can connect to (additive, in cents). */
export interface ModBus {
  /** LFO scaled to vibrato depth in cents -> oscillator detune. */
  vibrato: Tone.ToneAudioNode;
  /** LFO scaled to filter depth in cents -> filter detune. */
  filter: Tone.ToneAudioNode;
}

/**
 * One monophonic voice. The instrument (PolyInstrument) owns allocation;
 * a voice just plays what it is told, at the given AudioContext time.
 */
export interface Voice<P> {
  readonly output: Tone.ToneAudioNode;
  /** Start a note. `glideFrom` (MIDI) slides pitch from that note (mono/portamento). */
  attack(midi: number, time: number, velocity: number, glideFrom?: number | null): void;
  /** Legato: move pitch to `midi` without re-triggering envelopes. */
  glide(midi: number, time: number): void;
  /** Key up. Returns the time the voice will be silent. */
  release(time: number): number;
  /** Fast fade-out (voice steal / panic). Returns the time the voice will be silent. */
  hardStop(time: number): number;
  /** Seconds the release tail lasts with the current params. */
  releaseDuration(): number;
  /** Apply new parameters live (no re-creation, no audible reset of sounding notes). */
  update(params: P): void;
  dispose(): void;
}
