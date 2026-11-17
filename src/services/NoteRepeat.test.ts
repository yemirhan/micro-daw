import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  FREE_TICK_MS,
  NoteRepeat,
  nextFreeLine,
  nextGridTick,
  planRepeat,
  repeatIntervalTicks,
  type NoteRepeatClock,
} from './NoteRepeat';

const PPQ = 192;
const BPM = 120; // 0.5 s per beat → 1/16 = 48 ticks = 125 ms

describe('note repeat scheduling math', () => {
  it('converts rates to ticks', () => {
    expect(repeatIntervalTicks('1/4', PPQ)).toBe(192);
    expect(repeatIntervalTicks('1/8', PPQ)).toBe(96);
    expect(repeatIntervalTicks('1/8T', PPQ)).toBe(64);
    expect(repeatIntervalTicks('1/16', PPQ)).toBe(48);
    expect(repeatIntervalTicks('1/16T', PPQ)).toBe(32);
    expect(repeatIntervalTicks('1/32', PPQ)).toBe(24);
  });

  it('finds the next grid line (a line itself counts)', () => {
    expect(nextGridTick(0, 48)).toBe(0);
    expect(nextGridTick(1, 48)).toBe(48);
    expect(nextGridTick(48, 48)).toBe(48);
    expect(nextGridTick(95.9, 48)).toBe(96);
  });

  it('starts at least half a step after the press (closer lines would quantize onto the initial hit)', () => {
    // Pressed just before a line: that line is skipped.
    expect(planRepeat(90, 90, 48).startTick).toBe(144);
    // Pressed just after a line: the next line is far enough.
    expect(planRepeat(50, 50, 48).startTick).toBe(96);
    // Exactly half a step away is allowed.
    expect(planRepeat(72, 72, 48).startTick).toBe(96);
  });

  it('lists the lines the scheduler already passed (look-ahead) for manual firing', () => {
    // Press at 50; the scheduler is already at tick 150 (look-ahead) → line 96 must be fired by hand.
    expect(planRepeat(50, 150, 48)).toEqual({ startTick: 96, catchUpTicks: [96, 144] });
    // A line exactly at the scheduler position is left to the transport.
    expect(planRepeat(50, 144, 48).catchUpTicks).toEqual([96]);
    expect(planRepeat(50, 60, 48).catchUpTicks).toEqual([]);
  });
});

class FakeClock implements NoteRepeatClock {
  running = true;
  press = 0;
  scheduled = 0;
  time = 10;
  nextId = 1;
  events = new Map<number, { cb: (time: number) => void; interval: number }>();
  stateCbs = new Set<() => void>();
  isRunning = () => this.running;
  ppq = () => PPQ;
  bpm = () => BPM;
  pressTick = () => this.press;
  scheduledTick = () => this.scheduled;
  scheduleRepeat = vi.fn((cb: (time: number) => void, interval: number) => {
    const id = this.nextId++;
    this.events.set(id, { cb, interval });
    return id;
  });
  clear = vi.fn((id: number) => {
    this.events.delete(id);
  });
  now = () => this.time;
  onStateChange = (cb: () => void) => {
    this.stateCbs.add(cb);
    return () => this.stateCbs.delete(cb);
  };
  timers = new Set<() => void>();
  every = vi.fn((cb: () => void) => {
    this.timers.add(cb);
    return () => this.timers.delete(cb);
  });
  /** Advance the audio clock in free-scheduler wake-ups. */
  advance(sec: number) {
    const steps = Math.round((sec * 1000) / FREE_TICK_MS);
    for (let i = 0; i < steps; i++) {
      this.time += FREE_TICK_MS / 1000;
      for (const cb of [...this.timers]) cb();
    }
  }
  /** Fire every repeat event for grid tick `tick` (its audio time relative to the press). */
  fireTick(tick: number) {
    const secPerTick = 60 / (BPM * PPQ);
    const t = this.time + (tick - this.press) * secPerTick;
    for (const { cb } of [...this.events.values()]) cb(t);
  }
  setRunning(on: boolean) {
    this.running = on;
    for (const cb of this.stateCbs) cb();
  }
}

describe('nextFreeLine', () => {
  it('starts one step after the first hit and joins an existing grid', () => {
    expect(nextFreeLine(10, 10, 0.125)).toBeCloseTo(10.125);
    // grid through 10.125, pressed at 10.2 (0.075 s after a line, more than half a step to the next)
    expect(nextFreeLine(10.2, 10.125, 0.125)).toBeCloseTo(10.375);
    expect(nextFreeLine(10.13, 10.125, 0.125)).toBeCloseTo(10.25);
  });
});

describe('NoteRepeat', () => {
  let clock: FakeClock;
  let nr: NoteRepeat;
  let hits: [number, number][];
  let times: number[];
  const secPerTick = 60 / (BPM * PPQ);

  beforeEach(() => {
    clock = new FakeClock();
    nr = new NoteRepeat(clock);
    hits = [];
    times = [];
    nr.setTrigger((pad, velocity, time) => {
      hits.push([pad, velocity]);
      times.push(time);
    });
  });
  afterEach(() => {
    nr.dispose();
  });

  it('does nothing while disabled', () => {
    nr.press(3, 100);
    expect(clock.scheduleRepeat).not.toHaveBeenCalled();
    expect(clock.every).not.toHaveBeenCalled();
  });

  it('retriggers on the transport grid at the event time, with the initial velocity, until release', () => {
    nr.setEnabled(true);
    nr.setRate('1/16');
    clock.press = 50;
    clock.scheduled = 50;
    nr.press(2, 0.7);
    expect(clock.scheduleRepeat).toHaveBeenCalledWith(expect.any(Function), 48);

    clock.fireTick(96);
    clock.fireTick(144);
    expect(hits).toEqual([[2, 0.7], [2, 0.7]]);
    // Handed out ahead of time with the transport's audio time (sample accurate), not played "now".
    expect(times[0]).toBeCloseTo(10 + 46 * secPerTick);
    expect(times[1]).toBeCloseTo(10 + 94 * secPerTick);

    nr.release(2);
    expect(clock.events.size).toBe(0);
    clock.fireTick(192);
    expect(hits).toHaveLength(2);
  });

  it('skips grid lines closer than half a step to the initial hit', () => {
    nr.setEnabled(true);
    clock.press = 90;
    clock.scheduled = 90;
    nr.press(1, 1);
    clock.fireTick(96); // 6 ticks after the press: same step as the initial hit
    expect(hits).toHaveLength(0);
    clock.fireTick(144);
    expect(hits).toHaveLength(1);
  });

  it('hands out lines inside the scheduler look-ahead immediately, at their exact times', () => {
    nr.setEnabled(true);
    clock.press = 50;
    clock.scheduled = 120; // line 96 is already behind the scheduler
    nr.press(4, 0.5);
    expect(hits).toEqual([[4, 0.5]]);
    expect(times[0]).toBeCloseTo(10 + 46 * secPerTick);
  });

  it('stops retriggering after release', () => {
    nr.setEnabled(true);
    clock.press = 50;
    clock.scheduled = 50;
    nr.press(0, 1);
    nr.release(0);
    clock.fireTick(96);
    expect(hits).toHaveLength(0);
  });

  it('repeats with the transport stopped on the audio clock, aligned on the first hit', () => {
    nr.setEnabled(true);
    clock.running = false;
    nr.press(3, 0.9); // at t = 10
    expect(clock.scheduleRepeat).not.toHaveBeenCalled();
    expect(nr.isRepeating(3)).toBe(true);
    clock.advance(0.5);
    // 1/16 at 120 BPM = 125 ms: 10.125, 10.25, 10.375, 10.5 and (look-ahead) 10.625
    expect(times.slice(0, 4).map((t) => +t.toFixed(3))).toEqual([10.125, 10.25, 10.375, 10.5]);
    for (let i = 1; i < times.length; i++) expect(times[i] - times[i - 1]).toBeCloseTo(0.125);
    // Never handed out further ahead than the look-ahead window.
    expect(times.at(-1)!).toBeLessThanOrEqual(clock.time + 0.1 + 1e-9);

    // A second pad joins the same grid.
    nr.press(5, 0.4);
    clock.advance(0.5);
    const padFive = times.filter((_, i) => hits[i][0] === 5);
    for (const t of padFive) expect(((t - 10) / 0.125) % 1).toBeCloseTo(0, 5);

    nr.release(3);
    nr.release(5);
    expect(clock.timers.size).toBe(0);
    const n = hits.length;
    clock.advance(0.5);
    expect(hits).toHaveLength(n);
  });

  it('moves held pads between the free clock and the transport when it starts and stops', () => {
    clock.running = false;
    nr.press(5, 0.9);
    nr.setEnabled(true);
    expect(clock.scheduleRepeat).not.toHaveBeenCalled();
    expect(clock.timers.size).toBe(1);

    clock.setRunning(true);
    expect(clock.scheduleRepeat).toHaveBeenCalledTimes(1);
    expect(clock.timers.size).toBe(0);
    expect(nr.isRepeating(5)).toBe(true);

    clock.setRunning(false);
    expect(clock.events.size).toBe(0);
    expect(clock.timers.size).toBe(1);
    expect(nr.isRepeating(5)).toBe(true);
  });

  it('reschedules held pads when the rate changes and stops everything when disabled', () => {
    nr.setEnabled(true);
    nr.press(0, 1);
    nr.setRate('1/8T');
    expect(clock.scheduleRepeat).toHaveBeenLastCalledWith(expect.any(Function), 64);
    expect(clock.events.size).toBe(1);
    nr.setEnabled(false);
    expect(clock.events.size).toBe(0);
  });

  it('notifies subscribers of setting changes', () => {
    const cb = vi.fn();
    nr.subscribe(cb);
    nr.setEnabled(true);
    nr.setRate('1/32');
    nr.setRate('1/32');
    nr.toggle();
    expect(cb).toHaveBeenCalledTimes(3);
    expect(nr.isEnabled()).toBe(false);
    expect(nr.getRate()).toBe('1/32');
  });
});
