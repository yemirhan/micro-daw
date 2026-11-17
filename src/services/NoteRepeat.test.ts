import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  NoteRepeat,
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

describe('NoteRepeat', () => {
  let clock: FakeClock;
  let nr: NoteRepeat;
  let hits: [number, number][];

  beforeEach(() => {
    vi.useFakeTimers();
    clock = new FakeClock();
    nr = new NoteRepeat(clock);
    hits = [];
    nr.setTrigger((pad, velocity) => hits.push([pad, velocity]));
  });
  afterEach(() => {
    nr.dispose();
    vi.useRealTimers();
  });

  it('does nothing while disabled', () => {
    nr.press(3, 100);
    expect(clock.scheduleRepeat).not.toHaveBeenCalled();
  });

  it('does not repeat while the transport is stopped', () => {
    nr.setEnabled(true);
    clock.running = false;
    nr.press(3, 100);
    expect(clock.scheduleRepeat).not.toHaveBeenCalled();
  });

  it('retriggers on the grid at the rate with the initial velocity until release', () => {
    nr.setEnabled(true);
    nr.setRate('1/16');
    clock.press = 50;
    clock.scheduled = 50;
    nr.press(2, 0.7);
    expect(clock.scheduleRepeat).toHaveBeenCalledWith(expect.any(Function), 48);

    clock.fireTick(96);
    vi.advanceTimersByTime(1000);
    clock.fireTick(144);
    vi.advanceTimersByTime(1000);
    expect(hits).toEqual([[2, 0.7], [2, 0.7]]);

    nr.release(2);
    expect(clock.events.size).toBe(0);
    clock.fireTick(192);
    vi.advanceTimersByTime(1000);
    expect(hits).toHaveLength(2);
  });

  it('waits until the event time (the transport calls back a look-ahead early)', () => {
    nr.setEnabled(true);
    clock.press = 50;
    clock.scheduled = 50;
    nr.press(0, 1);
    clock.fireTick(96); // 46 ticks ahead at 120 bpm ≈ 120 ms
    vi.advanceTimersByTime(100);
    expect(hits).toHaveLength(0);
    vi.advanceTimersByTime(30);
    expect(hits).toHaveLength(1);
  });

  it('skips grid lines closer than half a step to the initial hit', () => {
    nr.setEnabled(true);
    clock.press = 90;
    clock.scheduled = 90;
    nr.press(1, 1);
    clock.fireTick(96); // 6 ticks after the press: same step as the initial hit
    vi.advanceTimersByTime(1000);
    expect(hits).toHaveLength(0);
    clock.fireTick(144);
    vi.advanceTimersByTime(1000);
    expect(hits).toHaveLength(1);
  });

  it('fires lines inside the scheduler look-ahead by hand', () => {
    nr.setEnabled(true);
    clock.press = 50;
    clock.scheduled = 120; // line 96 is already behind the scheduler
    nr.press(4, 0.5);
    vi.advanceTimersByTime(110);
    expect(hits).toHaveLength(0);
    vi.advanceTimersByTime(20);
    expect(hits).toEqual([[4, 0.5]]);
  });

  it('cancels pending retriggers on release', () => {
    nr.setEnabled(true);
    clock.press = 50;
    clock.scheduled = 50;
    nr.press(0, 1);
    clock.fireTick(96);
    nr.release(0);
    vi.advanceTimersByTime(1000);
    expect(hits).toHaveLength(0);
  });

  it('starts repeating a held pad when enabled or when the transport starts, and stops with it', () => {
    clock.running = false;
    nr.press(5, 0.9);
    nr.setEnabled(true);
    expect(clock.scheduleRepeat).not.toHaveBeenCalled();

    clock.setRunning(true);
    expect(clock.scheduleRepeat).toHaveBeenCalledTimes(1);
    expect(nr.isRepeating(5)).toBe(true);

    clock.setRunning(false);
    expect(nr.isRepeating(5)).toBe(false);
    expect(clock.events.size).toBe(0);
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
