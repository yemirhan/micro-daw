import * as Tone from 'tone';
import type { OscParams } from '@/types/audio';

export const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

export const midiToFreq = (midi: number) => 440 * Math.pow(2, (midi - 69) / 12);

/** Minimum time between a voice's attack and its release (keeps envelopes > 0 so a release is never dropped). */
export const MIN_GATE = 0.012;
/** Fade used by hardStop (releaseAll / voice steal). */
export const HARD_STOP_FADE = 0.015;

/**
 * Velocity (0..1) -> linear gain. `sensitivity` 0 = constant, 1 = full range.
 * Curve ~ vel^1.7: half velocity is about -10 dB, a natural "keyboard" feel.
 */
export function velocityGain(velocity: number, sensitivity: number): number {
  const v = clamp(velocity, 0, 1);
  const s = clamp(sensitivity, 0, 1);
  return 1 - s + s * Math.pow(v, 1.7);
}

/** Normalise velocities that arrive as 0..127 (templates / MIDI files) to 0..1. */
export function normVelocity(velocity: number | undefined): number {
  if (velocity === undefined || !Number.isFinite(velocity)) return 0.8;
  if (velocity > 1) return clamp(velocity / 127, 0, 1);
  return clamp(velocity, 0, 1);
}

export function gainToDb(gain: number): number {
  return gain <= 0 ? -Infinity : 20 * Math.log10(gain);
}

/**
 * Tone sources assert that a restart is strictly later than the previous
 * start; two note-ons in the same audio tick would throw. This keeps a
 * per-voice monotonically increasing start time.
 */
export class MonotonicClock {
  private last = -Infinity;
  next(time: number): number {
    const t = time <= this.last ? this.last + 0.0005 : time;
    this.last = t;
    return t;
  }
  get lastTime(): number {
    return this.last;
  }
}

/** Tone OmniOscillator options for an OscParams block. */
export function omniOptions(p: OscParams): Record<string, unknown> {
  const unison = Math.round(clamp(p.unison ?? 1, 1, 7));
  if (p.partials && p.partials.length > 0) {
    return unison > 1
      ? { type: 'fatcustom', partials: p.partials, count: unison, spread: p.spread }
      : { type: 'custom', partials: p.partials };
  }
  if (p.wave === 'pulse') return { type: 'pulse', width: clamp(p.width ?? 0.4, -0.95, 0.95) };
  return unison > 1 ? { type: `fat${p.wave}`, count: unison, spread: p.spread } : { type: p.wave };
}

/** Apply OscParams to an existing OmniOscillator without recreating it. */
export function updateOmni(osc: Tone.OmniOscillator<Tone.Oscillator>, p: OscParams): void {
  const opts = omniOptions(p);
  const anyOsc = osc as unknown as Record<string, unknown>;
  if (anyOsc.type !== opts.type) anyOsc.type = opts.type;
  if (opts.type === 'pulse') {
    (osc as unknown as { width: { value: number } }).width.value = opts.width as number;
  }
  if (typeof opts.type === 'string' && opts.type.startsWith('fat')) {
    if (anyOsc.count !== opts.count) anyOsc.count = opts.count;
    anyOsc.spread = opts.spread;
  }
  if (opts.partials) anyOsc.partials = opts.partials;
  osc.detune.value = (p.semi ?? 0) * 100 + (p.fine ?? 0);
}

export function safeDispose(...nodes: Array<{ dispose(): unknown } | null | undefined>): void {
  for (const n of nodes) {
    try {
      n?.dispose();
    } catch {
      // already disposed
    }
  }
}

/** Start a source at `time` unless it is silent; guarded against same-time restarts. */
/** Minimal start/stop surface shared by Tone sources. */
export interface StartStop {
  start(time?: number): unknown;
  stop(time?: number): unknown;
}

export function startSource(src: StartStop, time: number): void {
  try {
    src.start(time);
  } catch {
    // Start-time assertion (same tick restart): retry a hair later.
    try {
      src.start(time + 0.001);
    } catch {
      /* give up silently -- never break the note path */
    }
  }
}

export function stopSource(src: StartStop, time: number): void {
  try {
    src.stop(time);
  } catch {
    /* not started */
  }
}

const PITCH_CLASS: Record<string, number> = { C: 0, D: 2, E: 4, F: 5, G: 7, A: 9, B: 11 };

/**
 * Note -> MIDI number. Strings are note names ('C4', 'F#3', 'Bb2', 'Cs4');
 * numbers are taken as MIDI note numbers (the app's convention).
 */
export function noteToMidi(note: string | number): number {
  if (typeof note === 'number') return Math.round(note);
  const m = /^([A-Ga-g])(#|s|b)?(-?\d+)$/.exec(note.trim());
  if (!m) return -1;
  const acc = m[2] === '#' || m[2] === 's' ? 1 : m[2] === 'b' ? -1 : 0;
  return (parseInt(m[3], 10) + 1) * 12 + PITCH_CLASS[m[1].toUpperCase()] + acc;
}

const NAMES = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'];
export function midiToName(midi: number): string {
  const m = Math.round(midi);
  return `${NAMES[((m % 12) + 12) % 12]}${Math.floor(m / 12) - 1}`;
}

/** Duration (seconds or Tone time string like '8n') -> seconds. */
export function durationToSeconds(duration: number | string): number {
  if (typeof duration === 'number') return duration;
  try {
    return Tone.Time(duration).toSeconds();
  } catch {
    return 0.25;
  }
}

/**
 * Run `fn` with Tone's global context set to `ctx`, so nodes created lazily
 * (voices, character stages) land in the instrument's own context. Needed for
 * offline export: Tone.Offline restores the real-time context before its
 * clock fires the scheduled callbacks that allocate voices.
 */
export function withContext<T>(ctx: Tone.BaseContext, fn: () => T): T {
  const prev = Tone.getContext();
  if (prev === ctx) return fn();
  Tone.setContext(ctx);
  try {
    return fn();
  } finally {
    Tone.setContext(prev);
  }
}
