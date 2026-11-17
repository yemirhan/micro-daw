import * as Tone from 'tone';
import { arrangementEngine } from '@/services/ArrangementEngine';
import type { NoteRepeatRate, PadIndex } from '@/types/sampler';

/**
 * Note repeat (MPC-style): while enabled and the transport runs, a held pad
 * retriggers on the transport grid at the chosen rate, with the velocity of
 * the hit that started it, until it is released.
 *
 * Retriggers go through the registered trigger (useMidiNotes' pad-hit path),
 * so they sound, flash and are captured by the loop recorder exactly like a
 * finger hit. The initial hit is the caller's normal hit; repeats start at the
 * next grid line that is at least half a step away from it (a line closer
 * than that would quantize onto the same step as the initial hit).
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
}

export type NoteRepeatTrigger = (pad: PadIndex, velocity: number) => void;

interface HeldPad {
  velocity: number;
  /** Transport repeat event, while repeating. */
  eventId: number | null;
  timers: Set<ReturnType<typeof setTimeout>>;
}

export class NoteRepeat {
  private enabled = false;
  private rate: NoteRepeatRate = DEFAULT_NOTE_REPEAT_RATE;
  private trigger: NoteRepeatTrigger | null = null;
  private held = new Map<PadIndex, HeldPad>();
  private listeners = new Set<() => void>();
  private wasRunning = false;
  private unsubClock: (() => void) | null = null;

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

  /** Where retriggers are sent (the normal pad-hit path). */
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
    const held: HeldPad = { velocity, eventId: null, timers: new Set() };
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

  isRepeating(pad: PadIndex): boolean {
    return this.held.get(pad)?.eventId != null;
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
    if (!this.clock.isRunning() || held.eventId !== null) return;
    const interval = repeatIntervalTicks(this.rate, this.clock.ppq());
    const pressTick = this.clock.pressTick();
    const plan = planRepeat(pressTick, this.clock.scheduledTick(), interval);
    const secPerTick = 60 / (this.clock.bpm() * this.clock.ppq());
    const pressTime = this.clock.now();
    /** Grid lines before the first planned repeat (too close to the initial hit) are skipped. */
    const firstTime = pressTime + (plan.startTick - pressTick) * secPerTick - FIRST_LINE_SLACK_SEC;

    for (const tick of plan.catchUpTicks) {
      this.fireAfter(pad, held, (tick - pressTick) * secPerTick * 1000);
    }
    held.eventId = this.clock.scheduleRepeat((time) => {
      if (time < firstTime) return;
      this.fireAfter(pad, held, (time - this.clock.now()) * 1000);
    }, interval);
  }

  /** Retrigger at the event's audio time (the transport callback runs a look-ahead early). */
  private fireAfter(pad: PadIndex, held: HeldPad, ms: number): void {
    const timer = setTimeout(() => {
      held.timers.delete(timer);
      if (this.held.get(pad) !== held || !this.enabled) return;
      this.trigger?.(pad, held.velocity);
    }, Math.max(0, ms));
    held.timers.add(timer);
  }

  private stop(held: HeldPad): void {
    if (held.eventId !== null) this.clock.clear(held.eventId);
    held.eventId = null;
    for (const t of held.timers) clearTimeout(t);
    held.timers.clear();
  }

  private stopAll(): void {
    for (const held of this.held.values()) this.stop(held);
  }

  private restartAll(): void {
    this.stopAll();
    if (!this.enabled) return;
    for (const [pad, held] of this.held) this.start(pad, held);
  }

  /** Start repeats for held pads when the transport starts; drop them when it stops. */
  private watchClock(): void {
    if (this.unsubClock) return;
    this.wasRunning = this.clock.isRunning();
    this.unsubClock = this.clock.onStateChange(() => {
      const running = this.clock.isRunning();
      if (running === this.wasRunning) return;
      this.wasRunning = running;
      if (running) this.restartAll();
      else this.stopAll();
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
};

export const noteRepeat = new NoteRepeat(toneNoteRepeatClock);
