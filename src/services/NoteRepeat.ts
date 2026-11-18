import * as Tone from 'tone';
import { arrangementEngine } from '@/services/ArrangementEngine';
import type { NoteRepeatRate, PadIndex } from '@/types/sampler';

/**
 * Note repeat (MPC-style): while enabled, a held pad retriggers at the chosen
 * rate, with the velocity of the hit that started it, until it is released.
 *
 * Every retrigger is handed to the registered trigger WITH its AudioContext
 * time, ahead of time, so the sampler plays it sample-accurately and the loop
 * recorder places it exactly (useMidiNotes' pad-hit path: sound, flash,
 * recorder, event bus).
 *
 * - Transport running: repeats follow the transport grid (a Tone.Transport
 *   repeat event; its callback `time` is used as is). The initial hit is the
 *   caller's normal hit; repeats start at the next grid line that is at least
 *   half a step away from it (a closer line would quantize onto the same step).
 * - Transport stopped: a free-running look-ahead scheduler on the audio clock
 *   at the current BPM, aligned on the first held pad's hit (pads pressed
 *   later join the same grid).
 */

export const NOTE_REPEAT_RATES: readonly NoteRepeatRate[] = ['1/4', '1/8', '1/8T', '1/16', '1/16T', '1/32'];
export const DEFAULT_NOTE_REPEAT_RATE: NoteRepeatRate = '1/16';

/** Beats per repeat step for each rate (quarter note = 1 beat). */
const RATE_BEATS: Record<NoteRepeatRate, number> = {
  '1/4': 1,
  '1/8': 1 / 2,
  '1/8T': 1 / 3,
  '1/16': 1 / 4,
  '1/16T': 1 / 6,
  '1/32': 1 / 8,
};

export function isNoteRepeatRate(v: unknown): v is NoteRepeatRate {
  return typeof v === 'string' && (NOTE_REPEAT_RATES as readonly string[]).includes(v);
}

/** Beats (quarter notes) per repeat step. */
export function repeatStepBeats(rate: NoteRepeatRate): number {
  return RATE_BEATS[rate];
}

/** Repeat interval in transport ticks (`ppq` ticks per quarter note). */
export function repeatIntervalTicks(rate: NoteRepeatRate, ppq: number): number {
  return RATE_BEATS[rate] * ppq;
}

const EPS = 1e-6;
/** Tolerance when comparing a transport event time with the first planned repeat. */
const FIRST_LINE_SLACK_SEC = 0.001;

/** First grid line (multiple of `interval`) at or after `tick`. */
export function nextGridTick(tick: number, interval: number): number {
  if (!(interval > 0)) return tick;
  return Math.ceil(tick / interval - EPS) * interval + 0; // + 0: no -0
}

export interface RepeatPlan {
  /** Grid tick of the first repeat. */
  startTick: number;
  /**
   * Grid lines from `startTick` up to (excluding) the tick the transport has
   * already scheduled up to (its look-ahead). A repeat event created now would
   * skip them, so they are fired by hand; the event takes over from there.
   */
  catchUpTicks: number[];
}

/**
 * Where repeats of a pad pressed at `pressTick` start.
 * @param pressTick transport ticks at the moment of the press (what is heard)
 * @param scheduledTick transport ticks the scheduler has reached (press + look-ahead)
 * @param interval repeat interval in ticks
 */
export function planRepeat(pressTick: number, scheduledTick: number, interval: number): RepeatPlan {
  const startTick = nextGridTick(pressTick + interval / 2, interval);
  const catchUpTicks: number[] = [];
  for (let t = startTick; t < scheduledTick - EPS && catchUpTicks.length < 64; t += interval) catchUpTicks.push(t);
  return { startTick, catchUpTicks };
}

/**
 * Free-running repeat times (transport stopped): the first line of a grid of
 * `step` seconds through `ref` that is at least half a step after `press`.
 */
export function nextFreeLine(press: number, ref: number, step: number): number {
  if (!(step > 0)) return press;
  const k = Math.ceil((press + step / 2 - ref) / step - EPS);
  return ref + k * step;
}

/** The transport side of note repeat (injected so the scheduling is testable). */
export interface NoteRepeatClock {
  /** Transport running (playing, recording or counting in). */
  isRunning(): boolean;
  ppq(): number;
  bpm(): number;
  /** Transport ticks at the current (heard) time. */
  pressTick(): number;
  /** Transport ticks at the scheduler's look-ahead time. */
  scheduledTick(): number;
  /**
   * Call `cb(time)` on every multiple of `intervalTicks` (grid anchored at tick 0,
   * so it stays on the grid across loop wraps); returns an id for `clear`.
   */
  scheduleRepeat(cb: (time: number) => void, intervalTicks: number): number;
  clear(id: number): void;
  /** Current AudioContext time (what is heard now). */
  now(): number;
  /** Subscribe to transport state changes. */
  onStateChange(cb: () => void): () => void;
  /** Call `cb` every `ms` milliseconds (free-running scheduler); returns a stop function. */
  every(cb: () => void, ms: number): () => void;
}

/** Retrigger `pad` at AudioContext time `time` (in the near future). */
export type NoteRepeatTrigger = (pad: PadIndex, velocity: number, time: number) => void;

/**
 * Free-running scheduler: how far ahead hits are handed out, and how often it
 * wakes up. Kept short: a hit handed out is only cancellable until it starts,
 * and the wake-up period (+ timer jitter) must stay well inside the window.
 */
export const FREE_LOOKAHEAD_SEC = 0.04;
export const FREE_TICK_MS = 15;
/** A free step this late (timer stalled) is skipped instead of played late. */
const FREE_LATE_SEC = 0.03;

interface HeldPad {
  velocity: number;
  /** Transport repeat event, while repeating on the transport. */
  eventId: number | null;
  /** Next free-running repeat time, while repeating with the transport stopped. */
  freeNext: number | null;
}

export class NoteRepeat {
  private enabled = false;
  private rate: NoteRepeatRate = DEFAULT_NOTE_REPEAT_RATE;
  private trigger: NoteRepeatTrigger | null = null;
  private held = new Map<PadIndex, HeldPad>();
  private listeners = new Set<() => void>();
  private wasRunning = false;
  private unsubClock: (() => void) | null = null;
  private stopFreeTimer: (() => void) | null = null;

  constructor(private readonly clock: NoteRepeatClock) {}

  // --- Settings ------------------------------------------------------------

  isEnabled(): boolean {
    return this.enabled;
  }

  setEnabled(on: boolean): void {
    if (this.enabled === on) return;
    this.enabled = on;
    if (on) {
      this.watchClock();
      this.restartAll();
    } else {
      this.stopAll();
    }
    this.emit();
  }

  toggle(): void {
    this.setEnabled(!this.enabled);
  }

  getRate(): NoteRepeatRate {
    return this.rate;
  }

  setRate(rate: NoteRepeatRate): void {
    if (!isNoteRepeatRate(rate) || rate === this.rate) return;
    this.rate = rate;
    this.restartAll();
    this.emit();
  }

  /** Where retriggers are sent (the normal pad-hit path, with the audio time to play at). */
  setTrigger(trigger: NoteRepeatTrigger | null): void {
    this.trigger = trigger;
  }

  subscribe(cb: () => void): () => void {
    this.listeners.add(cb);
    return () => {
      this.listeners.delete(cb);
    };
  }

  // --- Pads ---------------------------------------------------------------

  /** A pad went down (after its normal hit was played). */
  press(pad: PadIndex, velocity: number): void {
    this.release(pad);
    const held: HeldPad = { velocity, eventId: null, freeNext: null };
    this.held.set(pad, held);
    if (this.enabled) {
      this.watchClock();
      this.start(pad, held);
    }
  }

  /** A pad came up: stop its repeats. */
  release(pad: PadIndex): void {
    const held = this.held.get(pad);
    if (!held) return;
    this.held.delete(pad);
    this.stop(held);
  }

  releaseAll(): void {
    for (const pad of [...this.held.keys()]) this.release(pad);
  }

  isHeld(pad: PadIndex): boolean {
    return this.held.has(pad);
  }

  isRepeating(pad: PadIndex): boolean {
    const held = this.held.get(pad);
    return !!held && (held.eventId !== null || held.freeNext !== null);
  }

  dispose(): void {
    this.releaseAll();
    this.unsubClock?.();
    this.unsubClock = null;
    this.listeners.clear();
    this.trigger = null;
  }

  // --- Scheduling -----------------------------------------------------------

  private start(pad: PadIndex, held: HeldPad): void {
    if (held.eventId !== null || held.freeNext !== null) return;
    if (this.clock.isRunning()) this.startOnTransport(pad, held);
    else this.startFree(held);
  }

  private startOnTransport(pad: PadIndex, held: HeldPad): void {
    const interval = repeatIntervalTicks(this.rate, this.clock.ppq());
    const pressTick = this.clock.pressTick();
    const plan = planRepeat(pressTick, this.clock.scheduledTick(), interval);
    const secPerTick = 60 / (this.clock.bpm() * this.clock.ppq());
    const pressTime = this.clock.now();
    /** Grid lines before the first planned repeat (too close to the initial hit) are skipped. */
    const firstTime = pressTime + (plan.startTick - pressTick) * secPerTick - FIRST_LINE_SLACK_SEC;

    held.eventId = this.clock.scheduleRepeat((time) => {
      if (time < firstTime) return;
      this.fire(pad, held, time);
    }, interval);
    // Lines the transport scheduler already passed: hand them out now, at their exact times.
    for (const tick of plan.catchUpTicks) this.fire(pad, held, pressTime + (tick - pressTick) * secPerTick);
  }

  private stepSeconds(): number {
    const bpm = this.clock.bpm();
    return (60 / (bpm > 0 ? bpm : 120)) * RATE_BEATS[this.rate];
  }

  private startFree(held: HeldPad): void {
    const now = this.clock.now();
    const step = this.stepSeconds();
    // Join the grid of a pad that is already repeating; otherwise the grid starts at this hit.
    let ref = now;
    for (const other of this.held.values()) {
      if (other !== held && other.freeNext !== null) {
        ref = other.freeNext;
        break;
      }
    }
    held.freeNext = nextFreeLine(now, ref, step);
    if (!this.stopFreeTimer) this.stopFreeTimer = this.clock.every(() => this.pumpFree(), FREE_TICK_MS);
    this.pumpFree();
  }

  /** Hand out every free-running repeat inside the look-ahead window. */
  private pumpFree(): void {
    const now = this.clock.now();
    const horizon = now + FREE_LOOKAHEAD_SEC;
    let any = false;
    for (const [pad, held] of [...this.held]) {
      if (held.freeNext === null) continue;
      any = true;
      let guard = 0;
      while (held.freeNext !== null && held.freeNext < horizon && guard++ < 256) {
        const t: number = held.freeNext;
        held.freeNext = t + this.stepSeconds();
        if (t >= now - FREE_LATE_SEC) this.fire(pad, held, Math.max(t, now));
      }
    }
    if (!any) this.stopFree();
  }

  private stopFree(): void {
    this.stopFreeTimer?.();
    this.stopFreeTimer = null;
  }

  private fire(pad: PadIndex, held: HeldPad, time: number): void {
    if (this.held.get(pad) !== held || !this.enabled) return;
    this.trigger?.(pad, held.velocity, time);
  }

  private stop(held: HeldPad): void {
    if (held.eventId !== null) this.clock.clear(held.eventId);
    held.eventId = null;
    held.freeNext = null;
    if (![...this.held.values()].some((h) => h.freeNext !== null)) this.stopFree();
  }

  private stopAll(): void {
    for (const held of this.held.values()) this.stop(held);
    this.stopFree();
  }

  private restartAll(): void {
    this.stopAll();
    if (!this.enabled) return;
    for (const [pad, held] of this.held) this.start(pad, held);
  }

  /** Move held pads between the transport grid and the free-running clock when the transport starts/stops. */
  private watchClock(): void {
    if (this.unsubClock) return;
    this.wasRunning = this.clock.isRunning();
    this.unsubClock = this.clock.onStateChange(() => {
      const running = this.clock.isRunning();
      if (running === this.wasRunning) return;
      this.wasRunning = running;
      this.restartAll();
    });
  }

  private emit(): void {
    for (const cb of [...this.listeners]) cb();
  }
}

/** The app's transport: Tone.Transport + arrangementEngine's state. */
export const toneNoteRepeatClock: NoteRepeatClock = {
  isRunning: () => arrangementEngine.getState() !== 'stopped',
  ppq: () => Tone.getTransport().PPQ,
  bpm: () => arrangementEngine.getBpm(),
  pressTick: () => {
    const transport = Tone.getTransport();
    try {
      const ticks = transport.getTicksAtTime(Tone.immediate());
      if (Number.isFinite(ticks)) return ticks;
    } catch {
      // fall back to the look-ahead position
    }
    return transport.ticks;
  },
  scheduledTick: () => Tone.getTransport().ticks,
  scheduleRepeat: (cb, intervalTicks) => Tone.getTransport().scheduleRepeat(cb, `${Math.round(intervalTicks)}i`, 0),
  clear: (id) => {
    Tone.getTransport().clear(id);
  },
  now: () => Tone.immediate(),
  onStateChange: (cb) => arrangementEngine.onStateChange(cb),
  every: (cb, ms) => {
    const id = setInterval(cb, ms);
    return () => clearInterval(id);
  },
};

export const noteRepeat = new NoteRepeat(toneNoteRepeatClock);
